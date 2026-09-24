//! Retrieval of past exchanges, filtered by metadata first and ranked second.
//!
//! The order matters. A semantically similar message written to a different
//! person on a different channel is the wrong example — it will teach the
//! prompt the wrong register. So the filter (participant, channel,
//! relationship, situation, date range, conversation, source) is applied as a
//! `WHERE` clause, and ranking only ever reorders what survived it.
//!
//! Ranking is by wording: how much of the incoming message's vocabulary
//! appears in the message being answered, weighted so that rare words count
//! for more than common ones. That is a real signal, and a shallow one. With
//! a sentence encoder downloaded (`crate::encoder`), the message being
//! answered comes with a vector (`QueryVector`), and a candidate with a
//! vector from the same encoder is ranked mostly by how close in meaning the
//! two are — "drinks on friday?" finds "pub friday?" with no word in common —
//! and a little by wording. A candidate without one is ranked by wording, as
//! before.

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
    /// Ranked by meaning as well as wording: the message being answered came
    /// with a vector, and so did what this one answered.
    #[serde(skip_serializing, default)]
    pub by_meaning: bool,
}

/// The message being answered, as a vector made by the sentence encoder
/// named by `version`.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryVector {
    pub version: String,
    pub vector: Vec<f32>,
}

/// How much closeness in meaning counts, against wording, for a candidate
/// that has a vector.
const MEANING_WEIGHT: f64 = 0.75;

/// Closeness in meaning at which the reason says so.
const CLOSE_IN_MEANING: f64 = 0.5;

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
    retrieve_by_meaning(db, incoming, None, filter, limit)
}

/// `retrieve`, ranking by meaning too where `meaning` — the message being
/// answered, as a vector — and a candidate's vector from the same encoder
/// allow.
pub fn retrieve_by_meaning(
    db: &Db,
    incoming: &str,
    meaning: Option<&QueryVector>,
    filter: &RetrievalFilter,
    limit: usize,
) -> Result<Vec<RetrievedExchange>, DbError> {
    let candidates = db.candidate_exchanges(filter, CANDIDATE_POOL)?;
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let query = tokenize(incoming);
    let idf = inverse_document_frequency(&candidates);
    // What each candidate is compared with: the message it answered, or the
    // reply itself when it answered nothing stored.
    let compared = |c: &CandidateExchange| c.incoming_message_id.clone().unwrap_or_else(|| c.reply_message_id.clone());
    let vectors = match meaning {
        Some(q) if !query.is_empty() => {
            db.vectors_for(&candidates.iter().map(compared).collect::<Vec<_>>(), &q.version)?
        }
        _ => HashMap::new(),
    };

    let mut scored: Vec<RetrievedExchange> = candidates
        .into_iter()
        .map(|c| {
            let mut by_meaning = false;
            let (score, reason) = if query.is_empty() {
                (0.0, "most recent, with nothing to match against".to_string())
            } else {
                let haystack = tokenize(c.incoming.as_deref().unwrap_or(&c.reply));
                let overlap: f64 =
                    query.iter().filter(|t| haystack.contains(*t)).map(|t| idf.get(t).copied().unwrap_or(1.0)).sum();
                let total: f64 = query.iter().map(|t| idf.get(t).copied().unwrap_or(1.0)).sum();
                let s = if total > 0.0 { overlap / total } else { 0.0 };
                let mut shared: Vec<&str> =
                    query.iter().filter(|t| haystack.contains(*t)).map(String::as_str).collect();
                shared.sort_unstable();
                shared.truncate(3);
                let close = meaning.zip(vectors.get(&compared(&c))).and_then(|(q, v)| cosine(&q.vector, v));
                by_meaning = close.is_some();
                match close {
                    Some(close) => {
                        let reason = match (close >= CLOSE_IN_MEANING, shared.is_empty()) {
                            (true, true) => "close in meaning".to_string(),
                            (true, false) => format!("close in meaning, and in wording: {}", shared.join(", ")),
                            (false, true) => "same person and channel".to_string(),
                            (false, false) => format!("similar wording: {}", shared.join(", ")),
                        };
                        (MEANING_WEIGHT * close + (1.0 - MEANING_WEIGHT) * s, reason)
                    }
                    None => {
                        let reason = if shared.is_empty() {
                            "same person and channel".to_string()
                        } else {
                            format!("similar wording: {}", shared.join(", "))
                        };
                        (s, reason)
                    }
                }
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
                by_meaning,
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

/// Cosine similarity, clamped to 0..1; none when the vectors cannot be
/// compared (different lengths) or one says nothing (all zero).
fn cosine(a: &[f32], b: &[f32]) -> Option<f64> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    let dot: f64 = a.iter().zip(b).map(|(x, y)| f64::from(*x) * f64::from(*y)).sum();
    let na: f64 = a.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
    let nb: f64 = b.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
    (na > 0.0 && nb > 0.0).then(|| (dot / (na * nb)).clamp(0.0, 1.0))
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
        // One row per reply, however many people its conversation has: the
        // person it is shown against is the one who wrote what it answered,
        // or else the conversation's first.
        let mut sql = String::from(
            "SELECT m.id, m.body, prev.body,
                    COALESCE(prev.participant_id, (SELECT MIN(cp.participant_id) FROM conversation_participants cp
                                                   WHERE cp.conversation_id = m.conversation_id)),
                    m.channel, m.sent_at, prev.id
             FROM messages m
             LEFT JOIN messages prev ON prev.id = m.reply_to_message_id
             WHERE m.direction = 'self'",
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(p) = &filter.participant_id {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM conversation_participants cp
                              WHERE cp.conversation_id = m.conversation_id AND cp.participant_id = ?)",
            );
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
                " AND EXISTS (SELECT 1 FROM conversation_participants cp JOIN participants p ON p.id = cp.participant_id
                              WHERE cp.conversation_id = m.conversation_id AND p.relationship = ?)",
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
                // Asked for one person, it is shown against them.
                participant_id: match &filter.participant_id {
                    Some(p) => Some(p.clone()),
                    None => r.get(3)?,
                },
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
    fn a_reply_in_a_conversation_with_several_people_is_one_candidate() {
        let f = fixture();
        let ada_thread = f.db.latest_conversation_with(&f.ada).unwrap().unwrap().id;
        f.db.link_conversation_participant(&ada_thread, &f.bob).unwrap();
        let all = retrieve(&f.db, "", &RetrievalFilter::default(), 20).unwrap();
        assert_eq!(all.len(), 3, "{all:#?}");
        let mut ids: Vec<&String> = all.iter().map(|h| &h.reply_message_id).collect();
        ids.dedup();
        assert_eq!(ids.len(), 3);
        let bob = RetrievalFilter { participant_id: Some(f.bob.clone()), ..Default::default() };
        let to_bob = retrieve(&f.db, "", &bob, 20).unwrap();
        assert_eq!(to_bob.len(), 3);
        assert!(to_bob.iter().all(|h| h.participant_id.as_deref() == Some(f.bob.as_str())));
        let friends = RetrievalFilter { relationship: Some("friend".into()), ..Default::default() };
        assert_eq!(retrieve(&f.db, "", &friends, 20).unwrap().len(), 3);
    }

    /// The fixture's messages that were answered, with a vector each from the
    /// encoder "v": the deck [1, 0], the call [0.6, 0.8], the pub [0, 1].
    fn with_vectors(f: &Fixture) {
        let id = |body: &str| -> String {
            f.db.conn().query_row("SELECT id FROM messages WHERE body = ?1", [body], |r| r.get(0)).unwrap()
        };
        f.db.put_embeddings(
            "v",
            2,
            &[
                (id("can you send the quarterly deck"), vec![1.0, 0.0]),
                (id("are you free for a call tuesday"), vec![0.6, 0.8]),
                (id("pub friday?"), vec![0.0, 1.0]),
            ],
        )
        .unwrap();
    }

    #[test]
    fn with_an_encoder_meaning_finds_what_wording_cannot() {
        let f = fixture();
        with_vectors(&f);
        let drinks = QueryVector { version: "v".into(), vector: vec![0.1, 0.99] };
        let asked = "drinks at the bar this weekend";
        let hits = retrieve_by_meaning(&f.db, asked, Some(&drinks), &RetrievalFilter::default(), 3).unwrap();
        assert_eq!(hits[0].reply, "yeah im in", "{hits:#?}");
        assert_eq!(hits[0].reason, "close in meaning");
        assert!(hits.iter().all(|h| h.by_meaning));
        assert!(hits[0].score > hits[1].score);
        // By wording alone there is nothing to go on.
        assert!(retrieve(&f.db, asked, &RetrievalFilter::default(), 3).unwrap().iter().all(|h| h.score == 0.0));

        // Another encoder's vectors are not compared with this one's, and a
        // candidate without a vector is ranked by wording as before.
        let other = QueryVector { version: "w".into(), vector: vec![0.1, 0.99] };
        let by_words =
            retrieve_by_meaning(&f.db, "send the deck", Some(&other), &RetrievalFilter::default(), 3).unwrap();
        assert_eq!(by_words[0].reply, "Sending the deck this afternoon.");
        assert!(by_words.iter().all(|h| !h.by_meaning), "no vector of that encoder, so by wording");
        assert!(by_words[0].reason.starts_with("similar wording"), "{}", by_words[0].reason);
    }

    #[test]
    fn meaning_and_wording_together_say_both() {
        let f = fixture();
        with_vectors(&f);
        let deck = QueryVector { version: "v".into(), vector: vec![0.9, 0.1] };
        let hits = retrieve_by_meaning(&f.db, "send the deck", Some(&deck), &RetrievalFilter::default(), 3).unwrap();
        assert_eq!(hits[0].reply, "Sending the deck this afternoon.");
        assert_eq!(hits[0].reason, "close in meaning, and in wording: deck, send");
        assert_eq!(cosine(&[1.0, 0.0], &[0.0, 0.0]), None, "a vector of nothing is not compared");
        assert_eq!(cosine(&[1.0, 0.0], &[1.0]), None);
    }

    #[test]
    fn an_empty_corpus_retrieves_nothing_rather_than_failing() {
        let db = Db::open_in_memory().unwrap();
        assert!(retrieve(&db, "anything", &RetrievalFilter::default(), 5).unwrap().is_empty());
    }
}
