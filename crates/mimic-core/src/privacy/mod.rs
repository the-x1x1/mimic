//! Deletion that actually deletes.
//!
//! "Delete this person" is the promise a local-first product lives or dies on,
//! and the easy version of it — one `DELETE FROM participants` and a shrug at
//! the foreign keys — leaves the person's words behind in three places: the
//! messages of conversations they were part of that were written by someone
//! else, the representative examples chosen from those messages, and every
//! aggregate computed while they were still in the corpus.
//!
//! So deletion here is explicit, reported, and followed by a rebuild. What it
//! removes is enumerated in the returned `DeletionReport`, which the UI shows
//! before asking for confirmation and again afterwards.

use serde::{Deserialize, Serialize};

use crate::db::{Db, DbError};

/// What a deletion did, or — from `preview` — what it would do.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionReport {
    pub participants: usize,
    pub identifiers: usize,
    /// Conversations removed entirely, because this person was the only other
    /// party to them.
    pub conversations: usize,
    /// Messages removed: theirs, plus everything in a conversation that went
    /// with them — including the user's own side of it.
    pub messages: usize,
    pub own_messages: usize,
    pub embeddings: usize,
    pub representative_examples: usize,
    pub voice_profiles: usize,
    pub voice_preferences: usize,
    pub drafts: usize,
    /// Profiles marked for recomputation because they were derived from
    /// material that has now gone.
    pub profiles_invalidated: usize,
    /// Conversations kept because other people are in them, with this
    /// person's own messages removed from them.
    pub conversations_kept: usize,
    /// Measurements of the drafts removed. Every one goes: what was written
    /// for a case was written from the conversation before it and from
    /// examples across the user's mail, so any of it may be theirs.
    pub evaluations: usize,
}

impl Db {
    /// What `delete_participant` would remove. Runs no writes.
    pub fn preview_participant_deletion(&self, participant_id: &str) -> Result<DeletionReport, DbError> {
        self.participant_deletion(participant_id, false)
    }

    /// Remove a person and everything derived from them, then invalidate the
    /// aggregates they contributed to.
    pub fn delete_participant(&self, participant_id: &str) -> Result<DeletionReport, DbError> {
        self.participant_deletion(participant_id, true)
    }

    fn participant_deletion(&self, participant_id: &str, commit: bool) -> Result<DeletionReport, DbError> {
        if self.get_participant(participant_id)?.is_none() {
            return Err(DbError::NotFound(participant_id.into()));
        }
        // Conversations that exist only because of this person go entirely;
        // group conversations survive, minus this person's messages.
        let solo: Vec<String> = {
            let conn = self.conn();
            let mut stmt = conn.prepare(
                "SELECT cp.conversation_id FROM conversation_participants cp
                 WHERE cp.participant_id = ?1
                   AND (SELECT COUNT(*) FROM conversation_participants x WHERE x.conversation_id = cp.conversation_id) <= 1",
            )?;
            let rows = stmt.query_map([participant_id], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<_, _>>()?
        };
        let shared: Vec<String> = {
            let conn = self.conn();
            let mut stmt = conn.prepare(
                "SELECT cp.conversation_id FROM conversation_participants cp
                 WHERE cp.participant_id = ?1
                   AND (SELECT COUNT(*) FROM conversation_participants x WHERE x.conversation_id = cp.conversation_id) > 1",
            )?;
            let rows = stmt.query_map([participant_id], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<_, _>>()?
        };

        let mut report = DeletionReport {
            participants: 1,
            conversations: solo.len(),
            conversations_kept: shared.len(),
            ..Default::default()
        };
        {
            let conn = self.conn();
            let solo_list = sql_list(&solo);
            report.identifiers =
                count(&conn, "SELECT COUNT(*) FROM participant_identifiers WHERE participant_id = ?1", participant_id)?;
            report.messages = count(
                &conn,
                &format!("SELECT COUNT(*) FROM messages WHERE participant_id = ?1 OR conversation_id IN ({solo_list})"),
                participant_id,
            )?;
            report.own_messages = count(
                &conn,
                &format!(
                    "SELECT COUNT(*) FROM messages WHERE direction = 'self' AND conversation_id IN ({solo_list}) AND ?1 = ?1"
                ),
                participant_id,
            )?;
            report.embeddings = count(
                &conn,
                &format!(
                    "SELECT COUNT(*) FROM message_embeddings WHERE message_id IN
                     (SELECT id FROM messages WHERE participant_id = ?1 OR conversation_id IN ({solo_list}))"
                ),
                participant_id,
            )?;
            report.representative_examples = count(
                &conn,
                &format!(
                    "SELECT COUNT(*) FROM representative_examples WHERE participant_id = ?1 OR message_id IN
                     (SELECT id FROM messages WHERE participant_id = ?1 OR conversation_id IN ({solo_list}))"
                ),
                participant_id,
            )?;
            report.voice_profiles = count(
                &conn,
                "SELECT COUNT(*) FROM voice_profiles WHERE participant_id = ?1 OR scope_key = ?1",
                participant_id,
            )?;
            report.voice_preferences =
                count(&conn, "SELECT COUNT(*) FROM voice_preferences WHERE scope_key = ?1", participant_id)?;
            report.drafts = count(&conn, "SELECT COUNT(*) FROM drafts WHERE participant_id = ?1", participant_id)?;
            report.profiles_invalidated = count(
                &conn,
                "SELECT COUNT(*) FROM voice_profiles WHERE participant_id IS NOT ?1 AND scope_key <> ?1",
                participant_id,
            )?;
            report.evaluations = count0(&conn, "SELECT COUNT(*) FROM evaluations")?;
        }
        if !commit {
            return Ok(report);
        }

        let solo_list = sql_list(&solo);
        self.transaction(|tx| {
            // Order matters only for the rows that do not cascade.
            tx.execute(
                &format!(
                    "DELETE FROM representative_examples WHERE participant_id = ?1 OR message_id IN
                          (SELECT id FROM messages WHERE participant_id = ?1 OR conversation_id IN ({solo_list}))"
                ),
                [participant_id],
            )?;
            tx.execute("DELETE FROM voice_profiles WHERE participant_id = ?1 OR scope_key = ?1", [participant_id])?;
            tx.execute("DELETE FROM voice_preferences WHERE scope_key = ?1", [participant_id])?;
            // Every measurement of the drafts: their mail may be in any of it.
            tx.execute("DELETE FROM evaluations", [])?;
            // Conversations that were only with this person, and everything in
            // them, including the user's own half of the exchange.
            tx.execute(&format!("DELETE FROM conversations WHERE id IN ({solo_list})"), [])?;
            // The participant row takes their messages, identifiers, drafts,
            // conversation links and message embeddings with it by cascade.
            tx.execute("DELETE FROM participants WHERE id = ?1", [participant_id])?;
            Ok(())
        })?;

        // Whatever is left was computed over a corpus that no longer exists.
        self.mark_profiles_stale(None)?;
        for id in shared {
            self.refresh_conversation_stats(&id)?;
        }
        Ok(report)
    }

    /// Remove a source and everything imported through it.
    pub fn delete_source_and_contents(&self, source_id: &str) -> Result<DeletionReport, DbError> {
        let conn_counts = {
            let conn = self.conn();
            DeletionReport {
                // Threads that also hold another source's mail are handed to
                // that source rather than removed, so they are not counted.
                conversations: count(
                    &conn,
                    "SELECT COUNT(*) FROM conversations c WHERE c.source_id = ?1
                     AND NOT EXISTS (SELECT 1 FROM messages m WHERE m.conversation_id = c.id AND m.source_id <> ?1)",
                    source_id,
                )?,
                messages: count(&conn, "SELECT COUNT(*) FROM messages WHERE source_id = ?1", source_id)?,
                own_messages: count(
                    &conn,
                    "SELECT COUNT(*) FROM messages WHERE source_id = ?1 AND direction = 'self'",
                    source_id,
                )?,
                embeddings: count(
                    &conn,
                    "SELECT COUNT(*) FROM message_embeddings WHERE message_id IN (SELECT id FROM messages WHERE source_id = ?1)",
                    source_id,
                )?,
                representative_examples: count(
                    &conn,
                    "SELECT COUNT(*) FROM representative_examples WHERE message_id IN (SELECT id FROM messages WHERE source_id = ?1)",
                    source_id,
                )?,
                evaluations: count0(&conn, "SELECT COUNT(*) FROM evaluations")?,
                ..Default::default()
            }
        };
        // A thread can hold mail from more than one source: an mbox export
        // and the connected mailbox it came from join by Message-ID. Removing
        // one source removes its messages — and only its messages. A thread
        // this source owns but which also holds another source's mail is
        // handed to that source first, so deleting the conversation row does
        // not cascade into mail the user did not ask to remove; and a thread
        // this source only contributed to is tidied once its messages go.
        let affected: Vec<String> = {
            let conn = self.conn();
            conn.execute(
                "UPDATE OR IGNORE conversations SET source_id = (
                     SELECT m.source_id FROM messages m
                     WHERE m.conversation_id = conversations.id AND m.source_id <> ?1
                     ORDER BY m.sent_at, m.id LIMIT 1)
                 WHERE source_id = ?1
                   AND EXISTS (SELECT 1 FROM messages m WHERE m.conversation_id = conversations.id AND m.source_id <> ?1)",
                [source_id],
            )?;
            let mut stmt = conn.prepare(
                "SELECT DISTINCT conversation_id FROM messages WHERE source_id = ?1
                 AND conversation_id IN (SELECT id FROM conversations WHERE source_id <> ?1)",
            )?;
            let ids = stmt.query_map([source_id], |r| r.get::<_, String>(0))?;
            ids.collect::<Result<_, _>>()?
        };
        {
            let conn = self.conn();
            // Every measurement of the drafts: this mail may be in any of it.
            conn.execute("DELETE FROM evaluations", [])?;
            conn.execute("DELETE FROM messages WHERE source_id = ?1", [source_id])?;
        }
        self.delete_source(source_id)?;
        for conversation_id in &affected {
            {
                let conn = self.conn();
                // Someone who only wrote in the removed messages is no longer
                // part of this thread.
                conn.execute(
                    "DELETE FROM conversation_participants WHERE conversation_id = ?1
                     AND NOT EXISTS (SELECT 1 FROM messages m WHERE m.conversation_id = ?1
                                     AND m.participant_id = conversation_participants.participant_id)",
                    [conversation_id],
                )?;
                conn.execute(
                    "DELETE FROM conversations WHERE id = ?1
                     AND NOT EXISTS (SELECT 1 FROM messages WHERE conversation_id = ?1)",
                    [conversation_id],
                )?;
            }
            if self.get_conversation(conversation_id)?.is_some() {
                self.resequence_by_time(conversation_id)?;
            }
        }
        self.mark_profiles_stale(None)?;
        // A person Mimic only ever saw through this source is now a name with
        // no messages behind it; remove them rather than leaving an empty card
        // in the People list.
        let orphans = self.conn().execute(
            "DELETE FROM participants WHERE is_self = 0
             AND NOT EXISTS (SELECT 1 FROM conversation_participants cp WHERE cp.participant_id = participants.id)",
            [],
        )?;
        Ok(DeletionReport { participants: orphans, ..conn_counts })
    }

    /// Remove everything the user has imported, keeping their settings,
    /// identity and provider configuration.
    pub fn delete_all_communication_data(&self) -> Result<DeletionReport, DbError> {
        let report = {
            let conn = self.conn();
            DeletionReport {
                participants: count0(&conn, "SELECT COUNT(*) FROM participants")?,
                identifiers: count0(&conn, "SELECT COUNT(*) FROM participant_identifiers")?,
                conversations: count0(&conn, "SELECT COUNT(*) FROM conversations")?,
                messages: count0(&conn, "SELECT COUNT(*) FROM messages")?,
                own_messages: count0(&conn, "SELECT COUNT(*) FROM messages WHERE direction = 'self'")?,
                embeddings: count0(&conn, "SELECT COUNT(*) FROM message_embeddings")?,
                representative_examples: count0(&conn, "SELECT COUNT(*) FROM representative_examples")?,
                voice_profiles: count0(&conn, "SELECT COUNT(*) FROM voice_profiles")?,
                voice_preferences: count0(&conn, "SELECT COUNT(*) FROM voice_preferences")?,
                drafts: count0(&conn, "SELECT COUNT(*) FROM drafts")?,
                evaluations: count0(&conn, "SELECT COUNT(*) FROM evaluations")?,
                ..Default::default()
            }
        };
        self.transaction(|tx| {
            for table in [
                "representative_examples",
                "voice_profiles",
                "voice_preferences",
                "drafts",
                "message_situations",
                "message_embeddings",
                "messages",
                "conversation_participants",
                "conversations",
                "participant_identifiers",
                "participants",
                "sources",
                "analysis_runs",
                "evaluation_cases",
                "evaluations",
            ] {
                tx.execute(&format!("DELETE FROM {table}"), [])?;
            }
            Ok(())
        })?;
        Ok(report)
    }
}

fn count(conn: &rusqlite::Connection, sql: &str, arg: &str) -> Result<usize, DbError> {
    Ok(conn.query_row(sql, [arg], |r| r.get::<_, i64>(0))? as usize)
}

fn count0(conn: &rusqlite::Connection, sql: &str) -> Result<usize, DbError> {
    Ok(conn.query_row(sql, [], |r| r.get::<_, i64>(0))? as usize)
}

/// Inline a list of ids as a SQL literal list. Ids are UUIDs generated by this
/// crate, never user input, and the empty case has to produce a valid list.
fn sql_list(ids: &[String]) -> String {
    if ids.is_empty() {
        return "SELECT NULL WHERE 0".into();
    }
    ids.iter().map(|id| format!("'{}'", id.replace('\'', "''"))).collect::<Vec<_>>().join(",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{IdentifierInput, IdentifierKind, NewDraft, NewMessage, NewSource, VoiceLayer};
    use crate::voice;
    use serde_json::json;

    struct World {
        db: Db,
        ada: String,
        bob: String,
        source: String,
        group: String,
    }

    /// Ada in a one-to-one thread, Bob in another, and both in a group thread.
    fn world() -> World {
        let db = Db::open_in_memory().unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::Handle, "@c").unwrap();
        let source = db
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

        let mut seq = 0i64;
        let mut thread = |external: &str, people: &[&str], n: usize| {
            let cid = db.upsert_conversation(&source.id, external, "chat", None).unwrap();
            for p in people {
                db.link_conversation_participant(&cid, p).unwrap();
            }
            let mut batch = Vec::new();
            for i in 0..n {
                batch.push(NewMessage {
                    conversation_id: cid.clone(),
                    source_id: source.id.clone(),
                    participant_id: Some(people[i % people.len()].to_string()),
                    external_id: format!("{external}-in{i}"),
                    direction: "other".into(),
                    channel: "chat".into(),
                    sent_at: Some(format!("2026-02-{:02}T09:00:00Z", (i % 27) + 1)),
                    sequence_index: seq,
                    body: "something they said".into(),
                    reply_to_external_id: None,
                    metadata: serde_json::Value::Null,
                });
                seq += 1;
                batch.push(NewMessage {
                    conversation_id: cid.clone(),
                    source_id: source.id.clone(),
                    participant_id: None,
                    external_id: format!("{external}-out{i}"),
                    direction: "self".into(),
                    channel: "chat".into(),
                    sent_at: Some(format!("2026-02-{:02}T09:05:00Z", (i % 27) + 1)),
                    sequence_index: seq,
                    body: format!("what I wrote back number {i}"),
                    reply_to_external_id: None,
                    metadata: serde_json::Value::Null,
                });
                seq += 1;
            }
            db.insert_messages(&batch).unwrap();
            db.refresh_conversation_stats(&cid).unwrap();
            cid
        };
        thread("with-ada", &[&ada], 25);
        thread("with-bob", &[&bob], 25);
        let group = thread("group", &[&ada, &bob], 10);

        voice::analyze(&db, &mut |_, _| {}).unwrap();
        db.set_voice_preference(VoiceLayer::Relationship, &ada, "signOff", &json!("— C"), None).unwrap();
        db.create_draft(&NewDraft {
            participant_id: Some(ada.clone()),
            conversation_id: None,
            channel: "chat".into(),
            situation_id: None,
            incoming_message: None,
            incoming_message_id: None,
            intent: None,
            generated_text: "a draft to Ada".into(),
            provider: "mock".into(),
            model: "m".into(),
            context: json!({}),
            prompt_hash: "h".into(),
            evidence: json!({}),
        })
        .unwrap();
        World { db, ada, bob, source: source.id, group }
    }

    #[test]
    fn a_preview_changes_nothing_and_matches_what_deletion_does() {
        let w = world();
        let before = w.db.count_self_messages(None, None).unwrap();
        let preview = w.db.preview_participant_deletion(&w.ada).unwrap();
        assert_eq!(w.db.count_self_messages(None, None).unwrap(), before, "a preview is read-only");
        assert!(w.db.get_participant(&w.ada).unwrap().is_some());

        let done = w.db.delete_participant(&w.ada).unwrap();
        assert_eq!(preview, done, "the preview is the same report the deletion produces");
    }

    #[test]
    fn deleting_a_person_removes_their_words_and_the_users_half_of_the_exchange() {
        let w = world();
        let report = w.db.delete_participant(&w.ada).unwrap();
        assert_eq!(report.participants, 1);
        assert_eq!(report.conversations, 1, "the one-to-one thread goes");
        assert_eq!(report.conversations_kept, 1, "the group thread stays");
        // 25 of hers + 25 of the user's from the solo thread, plus her 5 in the group.
        assert_eq!(report.messages, 55, "{report:?}");
        assert_eq!(report.own_messages, 25, "the user's side of a deleted thread goes too");
        assert!(w.db.get_participant(&w.ada).unwrap().is_none());
        assert!(w.db.get_participant(&w.bob).unwrap().is_some(), "deleting one person does not touch another");

        // Nothing of hers is left anywhere.
        let left: i64 =
            w.db.conn()
                .query_row("SELECT COUNT(*) FROM messages WHERE participant_id = ?1", [&w.ada], |r| r.get(0))
                .unwrap();
        assert_eq!(left, 0);
        let ids: i64 = w
            .db
            .conn()
            .query_row("SELECT COUNT(*) FROM participant_identifiers WHERE participant_id = ?1", [&w.ada], |r| r.get(0))
            .unwrap();
        assert_eq!(ids, 0);
    }

    #[test]
    fn deleting_a_person_removes_everything_derived_from_them() {
        let w = world();
        assert!(w
            .db
            .get_voice_profile(VoiceLayer::Relationship, &w.ada, crate::version::ANALYSIS_VERSION)
            .unwrap()
            .is_some());
        assert!(!w.db.representative_examples(VoiceLayer::Relationship, &w.ada, 10).unwrap().is_empty());

        let report = w.db.delete_participant(&w.ada).unwrap();
        assert!(report.voice_profiles >= 1);
        assert_eq!(report.voice_preferences, 1);
        assert_eq!(report.drafts, 1);
        assert!(report.representative_examples > 0);

        assert!(w
            .db
            .get_voice_profile(VoiceLayer::Relationship, &w.ada, crate::version::ANALYSIS_VERSION)
            .unwrap()
            .is_none());
        assert!(w.db.representative_examples(VoiceLayer::Relationship, &w.ada, 10).unwrap().is_empty());
        assert!(w.db.get_voice_preference(VoiceLayer::Relationship, &w.ada, "signOff").unwrap().is_none());
        assert!(w.db.recent_drafts(10).unwrap().is_empty());
    }

    #[test]
    fn the_aggregates_that_included_them_are_invalidated_not_left_standing() {
        let w = world();
        assert!(
            !w.db.get_voice_profile(VoiceLayer::Global, "", crate::version::ANALYSIS_VERSION).unwrap().unwrap().stale
        );
        let report = w.db.delete_participant(&w.ada).unwrap();
        assert!(report.profiles_invalidated > 0);
        let global = w.db.get_voice_profile(VoiceLayer::Global, "", crate::version::ANALYSIS_VERSION).unwrap().unwrap();
        assert!(global.stale, "a global profile computed with her messages in it is no longer valid");
        // And recomputing produces a smaller, honest number.
        voice::analyze(&w.db, &mut |_, _| {}).unwrap();
        let global = w.db.get_voice_profile(VoiceLayer::Global, "", crate::version::ANALYSIS_VERSION).unwrap().unwrap();
        assert!(!global.stale);
        assert_eq!(global.sample_size, w.db.count_self_messages(None, None).unwrap());
    }

    #[test]
    fn a_group_conversation_survives_minus_the_deleted_person() {
        let w = world();
        let before = w.db.get_conversation(&w.group).unwrap().unwrap().message_count;
        w.db.delete_participant(&w.ada).unwrap();
        let after = w.db.get_conversation(&w.group).unwrap().unwrap();
        assert!(after.message_count < before, "her messages left the thread");
        assert!(after.message_count > 0, "the thread itself stays: Bob is still in it");
        let hers: i64 =
            w.db.conn()
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1 AND participant_id = ?2",
                    rusqlite::params![w.group, w.ada],
                    |r| r.get(0),
                )
                .unwrap();
        assert_eq!(hers, 0);
    }

    #[test]
    fn deleting_a_source_takes_its_import_and_the_people_it_introduced() {
        let w = world();
        let report = w.db.delete_source_and_contents(&w.source).unwrap();
        assert_eq!(report.conversations, 3);
        assert_eq!(report.messages, 120);
        assert_eq!(report.own_messages, 60);
        assert_eq!(report.participants, 2, "Ada and Bob were only known through this source");
        assert_eq!(w.db.count_self_messages(None, None).unwrap(), 0);
        assert!(w.db.list_participants(10).unwrap().is_empty());
        assert!(w.db.list_sources().unwrap().is_empty());
    }

    #[test]
    fn deleting_everything_keeps_the_settings_and_the_identity() {
        let w = world();
        w.db.set_setting("general.theme", &"dark").unwrap();
        let report = w.db.delete_all_communication_data().unwrap();
        assert_eq!(report.participants, 2);
        assert_eq!(report.messages, 120);
        assert!(report.voice_profiles > 0);

        assert_eq!(w.db.count_self_messages(None, None).unwrap(), 0);
        assert!(w.db.list_sources().unwrap().is_empty());
        assert!(w.db.list_voice_profiles(crate::version::ANALYSIS_VERSION).unwrap().is_empty());
        assert_eq!(w.db.get_setting::<String>("general.theme").unwrap().as_deref(), Some("dark"));
        assert_eq!(w.db.user_identity().unwrap().unwrap().display_name, "C", "who you are is not imported data");
    }

    #[test]
    fn deleting_someone_who_is_not_there_is_an_error_not_a_silent_success() {
        let w = world();
        assert!(w.db.delete_participant("nobody").is_err());
        assert!(w.db.preview_participant_deletion("nobody").is_err());
    }

    /// Two sources sharing a thread (an export and the mailbox it came from):
    /// removing either takes exactly its own messages, the report says so,
    /// and the other source's mail — and the thread — survive.
    #[test]
    fn removing_one_of_two_sources_in_a_thread_takes_only_its_own_mail() {
        use crate::db::{IdentifierKind, NewMessage, NewSource};
        let db = Db::open_in_memory().unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::Email, "c@example.com").unwrap();
        let mk = |connector: &str| {
            db.create_source(&NewSource {
                connector: connector.into(),
                name: connector.into(),
                channel: "email".into(),
                location: None,
                config: serde_json::json!({}),
            })
            .unwrap()
            .id
        };
        let export = mk("mbox");
        let mailbox = mk("imap");
        let ada = db
            .resolve_participant(
                "Ada",
                &[crate::db::IdentifierInput::new(IdentifierKind::Email, "ada@example.com")],
                false,
            )
            .unwrap();
        let bob = db
            .resolve_participant(
                "Bob",
                &[crate::db::IdentifierInput::new(IdentifierKind::Email, "bob@example.com")],
                false,
            )
            .unwrap();
        // The export owns the thread; the mailbox added Bob's later message to it.
        let convo = db.upsert_conversation(&export, "q@x", "email", Some("Offsite")).unwrap();
        db.link_conversation_participant(&convo, &ada).unwrap();
        db.link_conversation_participant(&convo, &bob).unwrap();
        let msg = |source: &str, id: &str, who: Option<&str>, dir: &str, at: &str| NewMessage {
            conversation_id: convo.clone(),
            source_id: source.to_string(),
            participant_id: who.map(str::to_string),
            external_id: id.into(),
            direction: dir.into(),
            channel: "email".into(),
            sent_at: Some(at.into()),
            sequence_index: 0,
            body: format!("body of {id}"),
            reply_to_external_id: None,
            metadata: serde_json::Value::Null,
        };
        db.insert_messages(&[
            msg(&export, "q@x", Some(&ada), "other", "2026-03-01T09:00:00Z"),
            msg(&export, "r@x", None, "self", "2026-03-01T10:00:00Z"),
            msg(&mailbox, "b@x", Some(&bob), "other", "2026-03-02T09:00:00Z"),
        ])
        .unwrap();
        db.resequence_by_time(&convo).unwrap();

        // Remove the export: its two messages go, Bob's stays, in a thread
        // the mailbox now owns, and the report counts exactly what went.
        let report = db.delete_source_and_contents(&export).unwrap();
        assert_eq!(report.messages, 2);
        assert_eq!(report.conversations, 0, "the thread was handed on, not removed");
        assert_eq!(db.count_messages().unwrap(), 1, "the mailbox's message survived");
        let c = db.get_conversation(&convo).unwrap().expect("the thread survives with the mail that is left");
        assert_eq!(c.source_id, mailbox);
        assert_eq!(c.message_count, 1);
        assert!(db.get_participant(&ada).unwrap().is_none(), "Ada was only in the export");
        assert!(db.get_participant(&bob).unwrap().is_some());

        // Remove the mailbox too: now nothing is left, and no empty thread.
        db.delete_source_and_contents(&mailbox).unwrap();
        assert_eq!(db.count_messages().unwrap(), 0);
        assert_eq!(db.count_conversations().unwrap(), 0);
    }

    #[test]
    fn removing_a_source_that_only_joined_a_thread_tidies_the_thread() {
        use crate::db::{IdentifierKind, NewMessage, NewSource};
        let db = Db::open_in_memory().unwrap();
        // Dated March 2026; the waiting window is measured against the
        // clock, so it is opened to any age here.
        db.set_setting(crate::db::WITHIN_DAYS_SETTING, &0).unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::Email, "c@example.com").unwrap();
        let mk = |connector: &str| {
            db.create_source(&NewSource {
                connector: connector.into(),
                name: connector.into(),
                channel: "email".into(),
                location: None,
                config: serde_json::json!({}),
            })
            .unwrap()
            .id
        };
        let export = mk("mbox");
        let mailbox = mk("imap");
        let ada = db
            .resolve_participant(
                "Ada",
                &[crate::db::IdentifierInput::new(IdentifierKind::Email, "ada@example.com")],
                false,
            )
            .unwrap();
        let bob = db
            .resolve_participant(
                "Bob",
                &[crate::db::IdentifierInput::new(IdentifierKind::Email, "bob@example.com")],
                false,
            )
            .unwrap();
        let convo = db.upsert_conversation(&export, "q@x", "email", None).unwrap();
        db.link_conversation_participant(&convo, &ada).unwrap();
        db.link_conversation_participant(&convo, &bob).unwrap();
        let msg = |source: &str, id: &str, who: &str, at: &str| NewMessage {
            conversation_id: convo.clone(),
            source_id: source.to_string(),
            participant_id: Some(who.to_string()),
            external_id: id.into(),
            direction: "other".into(),
            channel: "email".into(),
            sent_at: Some(at.into()),
            sequence_index: 0,
            body: format!("body of {id}"),
            reply_to_external_id: None,
            metadata: serde_json::Value::Null,
        };
        db.insert_messages(&[
            msg(&export, "q@x", &ada, "2026-03-01T09:00:00Z"),
            msg(&mailbox, "b@x", &bob, "2026-03-02T09:00:00Z"),
        ])
        .unwrap();
        db.resequence_by_time(&convo).unwrap();

        let report = db.delete_source_and_contents(&mailbox).unwrap();
        assert_eq!(report.messages, 1);
        let c = db.get_conversation(&convo).unwrap().unwrap();
        assert_eq!((c.source_id.as_str(), c.message_count), (export.as_str(), 1), "stats refreshed");
        assert!(db.get_participant(&bob).unwrap().is_none(), "Bob was only in the mailbox's message");
        let waiting = db.threads_awaiting_reply(10).unwrap();
        assert_eq!(waiting[0].last_message, "body of q@x", "the thread's last message is the one that is left");
    }
}
