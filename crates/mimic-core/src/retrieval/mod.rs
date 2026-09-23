//! Retrieval of past exchanges, filtered by metadata first and ranked second.
//!
//! The order matters. A semantically similar message written to a different
//! person on a different channel is the wrong example — it will teach the
//! prompt the wrong register. So the filter (participant, channel,
//! relationship, situation, date range, conversation, source) is applied as a
//! `WHERE` clause, and ranking only ever reorders what survived it.
//!
//! V1 ranks lexically: how much of the incoming message's vocabulary appears
//! in the message being answered, weighted so that rare words count for more
//! than common ones. This is a real signal and it is honest about being a
//! shallow one. Embedding-backed ranking replaces the scorer behind the same
//! `retrieve` signature in Phase 2; nothing above this module changes.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::db::{Db, DbError};

/// Which past messages may be considered. Every field narrows; `None` means
/// "do not narrow on this".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RetrievalFilter {
    pub participant_id: Option<String>,
    pub channel: Option<String>,
    pub relationship: Option<String>,
    pub situation_id: Option<String>,
    pub conversation_id: Option<String>,
    pub source_id: Option<String>,
    /// Inclusive RFC 3339 bounds.
    pub since: Option<String>,
    pub until: Option<String>,
    /// Conversations whose messages may not be used. Measuring the drafts
    /// hides the conversations it holds out, so no reply it is trying to
    /// predict can be shown to the model as an example of how to write it.
    pub exclude_conversations: Vec<String>,
}

/// One retrieved exchange: something someone said, and how the user answered.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievedExchange {
    pub reply_message_id: String,
    /// What the user wrote. This is the example.
    pub reply: String,
    /// What they were answering, when the reply was linked to one.
    pub incoming: Option<String>,
    /// Which message that was. Measuring the drafts keeps a case only while
    /// every message it was written from is still there; nothing shows it.
    #[serde(skip_serializing, default)]
    pub incoming_message_id: Option<String>,
    pub participant_id: Option<String>,
    pub channel: String,
    pub sent_at: Option<String>,
    pub score: f64,
    /// Plain-language reason, shown in the Compose evidence panel.
    pub reason: String,
}

/// Retrieve the user's most relevant past replies.
///
/// `incoming` is the message being answered; pass an empty string when the
/// user is starting a conversation, in which case ranking falls back to
/// recency, which is the only signal there is.
pub fn retrieve(
    db: &Db,
    incoming: &str,
    filter: &RetrievalFilter,
    limit: usize,
) -> Result<Vec<RetrievedExchange>, DbError> {
    let candidates = db.candidate_exchanges(filter, CANDIDATE_POOL)?;
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let query = tokenize(incoming);
    let idf = inverse_document_frequency(&candidates);

    let mut scored: Vec<RetrievedExchange> = candidates
        .into_iter()
        .map(|c| {
            let (score, reason) = if query.is_empty() {
                (0.0, "most recent, with nothing to match against".to_string())
            } else {
                let haystack = tokenize(c.incoming.as_deref().unwrap_or(&c.reply));
                let overlap: f64 =
                    query.iter().filter(|t| haystack.contains(*t)).map(|t| idf.get(t).copied().unwrap_or(1.0)).sum();
                let total: f64 = query.iter().map(|t| idf.get(t).copied().unwrap_or(1.0)).sum();
                let s = if total > 0.0 { overlap / total } else { 0.0 };
                let shared: Vec<&str> =
                    query.iter().filter(|t| haystack.contains(*t)).map(String::as_str).take(3).collect();
                let reason = if shared.is_empty() {
                    "same person and channel".to_string()
                } else {
                    format!("similar wording: {}", shared.join(", "))
                };
                (s, reason)
            };
            RetrievedExchange {
                reply_message_id: c.reply_message_id,
                reply: c.reply,
                incoming: c.incoming,
                incoming_message_id: c.incoming_message_id,
                participant_id: c.participant_id,
                channel: c.channel,
                sent_at: c.sent_at,
                score: (score * 1000.0).round() / 1000.0,
                reason,
            }
        })
        .collect();

    // Ties break on recency, then id: two runs over the same data return the
    // same examples in the same order.
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.sent_at.cmp(&a.sent_at))
            .then(a.reply_message_id.cmp(&b.reply_message_id))
    });
    scored.truncate(limit);
    Ok(scored)
}

/// How many rows the filter may return before ranking. Bounded so a filter
/// that matches a hundred thousand messages does not load them all.
const CANDIDATE_POOL: usize = 400;

/// Words that carry no signal for matching. Kept short on purpose: an
/// aggressive stoplist throws away the short function words that actually
/// distinguish "can you" from "could you".
const STOP: [&str; 14] = ["the", "a", "an", "and", "or", "of", "to", "in", "on", "at", "for", "is", "it", "that"];

fn tokenize(text: &str) -> HashSet<String> {
    text.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase())
        .filter(|w| w.len() > 1 && !STOP.contains(&w.as_str()))
        .collect()
}

/// Rare words count for more. Without this, "the meeting" and "the deck"
/// score the same as "the meeting" and "the meeting".
fn inverse_document_frequency(candidates: &[CandidateExchange]) -> HashMap<String, f64> {
    let n = candidates.len() as f64;
    let mut df: HashMap<String, f64> = HashMap::new();
    for c in candidates {
        for t in tokenize(c.incoming.as_deref().unwrap_or(&c.reply)) {
            *df.entry(t).or_default() += 1.0;
        }
    }
    df.into_iter().map(|(t, count)| (t, (n / count).ln().max(0.1))).collect()
}

/// A row from the metadata filter, before ranking.
#[derive(Debug, Clone)]
pub struct CandidateExchange {
    pub reply_message_id: String,
    pub reply: String,
    pub incoming: Option<String>,
    pub incoming_message_id: Option<String>,
    pub participant_id: Option<String>,
    pub channel: String,
    pub sent_at: Option<String>,
}

impl Db {
    /// The user's own messages that pass a filter, newest first, each paired
    /// with the message it answered.
    pub fn candidate_exchanges(
        &self,
        filter: &RetrievalFilter,
        limit: usize,
    ) -> Result<Vec<CandidateExchange>, DbError> {
        let mut sql = String::from(
            "SELECT m.id, m.body, prev.body, cp.participant_id, m.channel, m.sent_at, prev.id
             FROM messages m
             LEFT JOIN messages prev ON prev.id = m.reply_to_message_id
             LEFT JOIN conversation_participants cp ON cp.conversation_id = m.conversation_id
             WHERE m.direction = 'self'",
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(p) = &filter.participant_id {
            sql.push_str(" AND cp.participant_id = ?");
            args.push(Box::new(p.clone()));
        }
        if let Some(c) = &filter.channel {
            sql.push_str(" AND m.channel = ?");
            args.push(Box::new(c.clone()));
        }
        if let Some(c) = &filter.conversation_id {
            sql.push_str(" AND m.conversation_id = ?");
            args.push(Box::new(c.clone()));
        }
        if let Some(s) = &filter.source_id {
            sql.push_str(" AND m.source_id = ?");
            args.push(Box::new(s.clone()));
        }
        if let Some(r) = &filter.relationship {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM participants p WHERE p.id = cp.participant_id AND p.relationship = ?)",
            );
            args.push(Box::new(r.clone()));
        }
        if let Some(s) = &filter.situation_id {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM message_situations ms WHERE ms.message_id = m.id AND ms.situation_id = ?)",
            );
            args.push(Box::new(s.clone()));
        }
        if let Some(since) = &filter.since {
            sql.push_str(" AND m.sent_at >= ?");
            args.push(Box::new(since.clone()));
        }
        if let Some(until) = &filter.until {
            sql.push_str(" AND m.sent_at <= ?");
            args.push(Box::new(until.clone()));
        }
        if !filter.exclude_conversations.is_empty() {
            // One parameter however many there are: a held-out set of a
            // large mailbox runs to thousands of conversations.
            sql.push_str(" AND m.conversation_id NOT IN (SELECT value FROM json_each(?))");
            args.push(Box::new(serde_json::to_string(&filter.exclude_conversations).unwrap_or_else(|_| "[]".into())));
        }
        sql.push_str(&format!(" ORDER BY m.sent_at DESC, m.id LIMIT {limit}"));

        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())), |r| {
            Ok(CandidateExchange {
                reply_message_id: r.get(0)?,
                reply: r.get(1)?,
                incoming: r.get(2)?,
                incoming_message_id: r.get(6)?,
                participant_id: r.get(3)?,
                channel: r.get(4)?,
                sent_at: r.get(5)?,
            })
        })?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{IdentifierInput, IdentifierKind, NewMessage, NewSource};

    struct Fixture {
        db: Db,
        ada: String,
        bob: String,
    }

    fn fixture() -> Fixture {
        let db = Db::open_in_memory().unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::Handle, "@c").unwrap();
        let email = db
            .create_source(&NewSource {
                connector: "t".into(),
                name: "mail".into(),
                channel: "email".into(),
                location: None,
                config: serde_json::Value::Null,
            })
            .unwrap();
        let chat = db
            .create_source(&NewSource {
                connector: "t".into(),
                name: "chat".into(),
                channel: "chat".into(),
                location: None,
                config: serde_json::Value::Null,
            })
            .unwrap();
        let ada =
            db.resolve_participant("Ada", &[IdentifierInput::new(IdentifierKind::Handle, "@ada")], false).unwrap();
        let bob =
            db.resolve_participant("Bob", &[IdentifierInput::new(IdentifierKind::Handle, "@bob")], false).unwrap();
        db.set_participant_relationship(&ada, Some("colleague")).unwrap();
        db.set_participant_relationship(&bob, Some("friend")).unwrap();

        let mut seq = 0;
        let mut add = |source: &str, convo: &str, who: &str, pairs: &[(&str, &str, &str)]| {
            let cid =
                db.upsert_conversation(source, convo, if source == email.id { "email" } else { "chat" }, None).unwrap();
            db.link_conversation_participant(&cid, who).unwrap();
            let channel = if source == email.id { "email" } else { "chat" };
            let mut batch = Vec::new();
            for (date, incoming, reply) in pairs {
                batch.push(NewMessage {
                    conversation_id: cid.clone(),
                    source_id: source.into(),
                    participant_id: Some(who.into()),
                    external_id: format!("in{seq}"),
                    direction: "other".into(),
                    channel: channel.into(),
                    sent_at: Some(format!("{date}T09:00:00Z")),
                    sequence_index: seq,
                    body: (*incoming).into(),
                    reply_to_external_id: None,
                    metadata: serde_json::Value::Null,
                });
                seq += 1;
                batch.push(NewMessage {
                    conversation_id: cid.clone(),
                    source_id: source.into(),
                    participant_id: None,
                    external_id: format!("out{seq}"),
                    direction: "self".into(),
                    channel: channel.into(),
                    sent_at: Some(format!("{date}T09:05:00Z")),
                    sequence_index: seq,
                    body: (*reply).into(),
                    reply_to_external_id: None,
                    metadata: serde_json::Value::Null,
                });
                seq += 1;
            }
            db.insert_messages(&batch).unwrap();
            db.link_replies(&cid).unwrap();
            db.refresh_conversation_stats(&cid).unwrap();
        };
        add(
            &email.id,
            "t-ada",
            &ada,
            &[
                ("2026-01-05", "can you send the quarterly deck", "Sending the deck this afternoon."),
                ("2026-01-06", "are you free for a call tuesday", "Tuesday works, 2pm suits me."),
            ],
        );
        add(&chat.id, "t-bob", &bob, &[("2026-01-07", "pub friday?", "yeah im in")]);
        Fixture { db, ada, bob }
    }

    #[test]
    fn the_filter_runs_before_the_ranking() {
        let f = fixture();
        // A message about the pub, but filtered to Ada: Bob's reply must not
        // appear however well it matches.
        let hits = retrieve(
            &f.db,
            "pub friday?",
            &RetrievalFilter { participant_id: Some(f.ada.clone()), ..Default::default() },
            5,
        )
        .unwrap();
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|h| h.participant_id.as_deref() == Some(f.ada.as_str())));
        assert!(hits.iter().all(|h| h.reply != "yeah im in"));
    }

    #[test]
    fn similar_wording_outranks_recency() {
        let f = fixture();
        let hits = retrieve(&f.db, "could you send the quarterly deck over?", &RetrievalFilter::default(), 5).unwrap();
        assert_eq!(hits[0].reply, "Sending the deck this afternoon.", "{hits:#?}");
        assert!(hits[0].score > 0.0);
        assert!(hits[0].reason.starts_with("similar wording"), "{}", hits[0].reason);
        assert_eq!(hits[0].incoming.as_deref(), Some("can you send the quarterly deck"));
    }

    #[test]
    fn with_nothing_to_match_the_order_is_recency() {
        let f = fixture();
        let hits = retrieve(&f.db, "", &RetrievalFilter::default(), 5).unwrap();
        assert_eq!(hits[0].reply, "yeah im in", "the newest reply comes first");
        assert!(hits.iter().all(|h| h.score == 0.0));
        assert!(hits[0].reason.contains("nothing to match"));
    }

    #[test]
    fn every_filter_dimension_narrows() {
        let f = fixture();
        let by = |filter: RetrievalFilter| retrieve(&f.db, "", &filter, 20).unwrap().len();
        assert_eq!(by(RetrievalFilter::default()), 3);
        assert_eq!(by(RetrievalFilter { channel: Some("chat".into()), ..Default::default() }), 1);
        assert_eq!(by(RetrievalFilter { relationship: Some("colleague".into()), ..Default::default() }), 2);
        assert_eq!(by(RetrievalFilter { relationship: Some("nemesis".into()), ..Default::default() }), 0);
        assert_eq!(by(RetrievalFilter { participant_id: Some(f.bob.clone()), ..Default::default() }), 1);
        assert_eq!(by(RetrievalFilter { since: Some("2026-01-06T00:00:00Z".into()), ..Default::default() }), 2);
        assert_eq!(by(RetrievalFilter { until: Some("2026-01-05T23:59:59Z".into()), ..Default::default() }), 1);
        // A conversation held out is not there at all, however well it matches.
        let ada_thread = f.db.latest_conversation_with(&f.ada).unwrap().unwrap().id;
        assert_eq!(by(RetrievalFilter { exclude_conversations: vec![ada_thread.clone()], ..Default::default() }), 1);
        assert_eq!(
            by(RetrievalFilter {
                participant_id: Some(f.ada.clone()),
                exclude_conversations: vec!["not-a-thread".into(), ada_thread],
                ..Default::default()
            }),
            0
        );
        assert_eq!(
            by(RetrievalFilter {
                since: Some("2026-01-06T00:00:00Z".into()),
                until: Some("2026-01-06T23:59:59Z".into()),
                ..Default::default()
            }),
            1
        );
    }

    #[test]
    fn retrieval_is_deterministic_and_bounded() {
        let f = fixture();
        let a = retrieve(&f.db, "send the deck", &RetrievalFilter::default(), 2).unwrap();
        let b = retrieve(&f.db, "send the deck", &RetrievalFilter::default(), 2).unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(
            a.iter().map(|h| &h.reply_message_id).collect::<Vec<_>>(),
            b.iter().map(|h| &h.reply_message_id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn an_empty_corpus_retrieves_nothing_rather_than_failing() {
        let db = Db::open_in_memory().unwrap();
        assert!(retrieve(&db, "anything", &RetrievalFilter::default(), 5).unwrap().is_empty());
    }
}
