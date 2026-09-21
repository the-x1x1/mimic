//! The dashboard read model: what is waiting on the user, and what Mimic has
//! already written for it.
//!
//! This is the home screen's single query. It exists as one read model rather
//! than five commands so the screen cannot show a half-consistent picture —
//! threads from one moment and drafts from another.
//!
//! Two honesty rules shape it. Nothing here is a live inbox: every message it
//! reports came from an import, and `last_import_at` says when, so the screen
//! can say so rather than implying mail is arriving. And a thread is "awaiting
//! a reply" only because the last message in it came from someone else and was
//! never answered — no heuristic about urgency, no inferred intent.

use serde::Serialize;

use crate::db::{Db, DbError, Draft, DraftOutcomes, Participant};

/// One row of the feed: a conversation waiting on the user, the message it is
/// waiting on, and the draft Mimic has for it, if any.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardThread {
    pub conversation_id: String,
    pub channel: String,
    pub subject: Option<String>,
    pub is_group: bool,
    pub message_count: i64,
    /// The unanswered message, verbatim.
    pub last_message: String,
    pub last_message_at: Option<String>,
    pub last_message_id: String,
    /// Who it is from. `None` when the import could not attribute it; the row
    /// still appears, named as unattributed, rather than being hidden.
    pub participant: Option<Participant>,
    /// True when a relationship-layer voice profile exists for this person, so
    /// the screen can say whether a draft would be shaped by how the user
    /// writes *to them* or only by how they write in general.
    pub has_relationship_profile: bool,
    /// An unresolved draft for this conversation. Present only because one was
    /// generated — never a placeholder.
    pub draft: Option<Draft>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dashboard {
    pub people: i64,
    pub conversations: i64,
    pub messages: i64,
    pub own_messages: i64,
    /// Threads whose last message came from someone else, newest first.
    pub awaiting: Vec<DashboardThread>,
    /// How many such threads exist in total, which may exceed `awaiting.len()`.
    pub awaiting_total: i64,
    /// Unresolved drafts, including any not attached to a conversation.
    pub pending_drafts: Vec<Draft>,
    pub outcomes: DraftOutcomes,
    /// When the most recent import finished. `None` before the first one.
    pub last_import_at: Option<String>,
    /// Whether Mimic is allowed to draft replies without being asked each
    /// time. Off unless the user turned it on.
    pub auto_draft: bool,
}

pub fn dashboard(db: &Db, limit: usize, auto_draft: bool) -> Result<Dashboard, DbError> {
    let awaiting_rows = db.threads_awaiting_reply(limit)?;
    let mut awaiting = Vec::with_capacity(awaiting_rows.len());
    for row in awaiting_rows {
        let participant = match row.participant_id.as_deref() {
            Some(id) => db.get_participant(id)?,
            None => None,
        };
        let has_relationship_profile = match participant.as_ref() {
            Some(p) => db.has_relationship_profile(&p.id)?,
            None => false,
        };
        awaiting.push(DashboardThread {
            conversation_id: row.conversation.id.clone(),
            channel: row.conversation.channel.clone(),
            subject: row.conversation.subject.clone(),
            is_group: row.conversation.is_group,
            message_count: row.conversation.message_count,
            last_message: row.last_message,
            last_message_at: row.last_message_at,
            last_message_id: row.last_message_id,
            participant,
            has_relationship_profile,
            draft: db.pending_draft_for_conversation(&row.conversation.id)?,
        });
    }

    Ok(Dashboard {
        people: db.count_participants()?,
        conversations: db.count_conversations()?,
        messages: db.count_messages()?,
        own_messages: db.count_self_messages(None, None)?,
        awaiting_total: db.count_threads_awaiting_reply()?,
        awaiting,
        pending_drafts: db.pending_drafts(limit)?,
        outcomes: db.measured_draft_outcomes()?,
        last_import_at: db.last_import_at()?,
        auto_draft,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::repo_people::IdentifierInput;
    use crate::db::{IdentifierKind, NewMessage, NewSource};
    use serde_json::Value;

    fn db_with_thread() -> (Db, String, String, String) {
        let db = Db::open_in_memory().unwrap();
        let source = db
            .create_source(&NewSource {
                connector: "test".into(),
                name: "T".into(),
                channel: "chat".into(),
                location: None,
                config: Value::Null,
            })
            .unwrap();
        let convo = db.upsert_conversation(&source.id, "t1", "chat", Some("Lunch")).unwrap();
        let ada =
            db.resolve_participant("Ada", &[IdentifierInput::new(IdentifierKind::Handle, "@ada")], false).unwrap();
        db.link_conversation_participant(&convo, &ada).unwrap();
        (db, source.id, convo, ada)
    }

    fn msg(source: &str, convo: &str, who: Option<&str>, ext: &str, dir: &str, seq: i64, body: &str) -> NewMessage {
        NewMessage {
            conversation_id: convo.into(),
            source_id: source.into(),
            participant_id: who.map(str::to_string),
            external_id: ext.into(),
            direction: dir.into(),
            channel: "chat".into(),
            sent_at: Some(format!("2026-09-{:02}T10:00:00Z", seq + 1)),
            sequence_index: seq,
            body: body.into(),
            reply_to_external_id: None,
            metadata: Value::Null,
        }
    }

    #[test]
    fn a_thread_waits_only_while_its_last_message_is_someone_elses() {
        let (db, source, convo, ada) = db_with_thread();
        db.insert_messages(&[
            msg(&source, &convo, None, "m1", "self", 0, "are we still on for lunch"),
            msg(&source, &convo, Some(&ada), "m2", "other", 1, "can we move it to thursday?"),
        ])
        .unwrap();
        db.refresh_conversation_stats(&convo).unwrap();

        let waiting = db.threads_awaiting_reply(10).unwrap();
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].last_message, "can we move it to thursday?");
        assert_eq!(waiting[0].participant_id.as_deref(), Some(ada.as_str()));

        // Answering it takes the thread off the list; nothing else changed.
        db.insert_messages(&[msg(&source, &convo, None, "m3", "self", 2, "thursday works")]).unwrap();
        assert!(db.threads_awaiting_reply(10).unwrap().is_empty());
        assert_eq!(db.count_threads_awaiting_reply().unwrap(), 0);
    }

    #[test]
    fn a_message_of_unknown_direction_does_not_decide_either_way() {
        let (db, source, convo, ada) = db_with_thread();
        db.insert_messages(&[
            msg(&source, &convo, Some(&ada), "m1", "other", 0, "you around?"),
            // An unattributable message arriving last must not make the thread
            // look answered — direction was never established.
            msg(&source, &convo, None, "m2", "unknown", 1, "(unattributed)"),
        ])
        .unwrap();
        db.refresh_conversation_stats(&convo).unwrap();
        let waiting = db.threads_awaiting_reply(10).unwrap();
        assert_eq!(waiting.len(), 1, "the last message with a known direction is theirs");
        assert_eq!(waiting[0].last_message, "you around?");
    }

    #[test]
    fn the_dashboard_counts_what_is_there_and_claims_nothing_else() {
        let (db, source, convo, ada) = db_with_thread();
        let empty = dashboard(&db, 10, false).unwrap();
        assert_eq!(empty.messages, 0);
        assert!(empty.awaiting.is_empty());
        assert!(empty.pending_drafts.is_empty());
        assert_eq!(empty.last_import_at, None, "nothing has been imported yet and the screen must say so");
        assert_eq!(empty.outcomes.unedited_rate, None, "unmeasured, not zero");
        assert!(!empty.auto_draft);

        db.insert_messages(&[
            msg(&source, &convo, None, "m1", "self", 0, "hello"),
            msg(&source, &convo, Some(&ada), "m2", "other", 1, "hello back"),
        ])
        .unwrap();
        db.refresh_conversation_stats(&convo).unwrap();

        let view = dashboard(&db, 10, true).unwrap();
        assert_eq!(view.messages, 2);
        assert_eq!(view.own_messages, 1);
        assert_eq!(view.conversations, 1);
        assert_eq!(view.people, 1);
        assert_eq!(view.awaiting.len(), 1);
        assert_eq!(view.awaiting_total, 1);
        let thread = &view.awaiting[0];
        assert_eq!(thread.participant.as_ref().unwrap().display_name, "Ada");
        assert!(!thread.has_relationship_profile, "one message is not a profile");
        assert!(thread.draft.is_none(), "no draft exists until one is generated");
        assert!(view.auto_draft);
    }
}
