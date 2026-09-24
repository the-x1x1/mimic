//! The import pipeline: a connector's stream of conversations becomes rows.
//!
//! Shape, and the reasons for it:
//!
//! * One conversation at a time, inserted in batches inside a transaction, so
//!   memory is bounded by the largest thread rather than the export.
//! * Participants are resolved through a cache, because the alternative is one
//!   `SELECT` per message and a mailbox has a million of them.
//! * Direction is decided by matching the author against the user's declared
//!   identifiers, and nothing else. An author that matches nothing is
//!   `unknown`, never a guess.
//! * Cancellation is checked between conversations, so stopping an import
//!   leaves a consistent database with fewer conversations in it, not a
//!   half-written one.
//! * Re-importing is safe: `(source_id, external_id)` is unique, so the second
//!   run inserts nothing and reports duplicates.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::db::{Db, Direction, ImportCounts, NewMessage};
use crate::jobs::{JobContext, JobError, JobExecutor, JobFuture};
use crate::sources::{self, AuthorRef, DiscoveredConversation, SourceError};

pub const JOB_KIND: &str = "import_source";

/// How many messages are written per transaction.
const BATCH: usize = 500;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub conversations: usize,
    pub inserted: usize,
    pub duplicates: usize,
    pub empty: usize,
    /// Messages whose author matched one of the user's identifiers.
    pub from_self: usize,
    /// Messages whose author could not be matched to anyone.
    pub unattributed: usize,
    pub participants_created: usize,
}

/// Import one already-registered source. `on_progress` is called with
/// `(conversations_done, messages_done)`; returning `Err` from `should_stop`
/// aborts between conversations.
pub fn import_source(
    db: &Db,
    source_id: &str,
    on_progress: &mut dyn FnMut(usize, usize),
    should_stop: &dyn Fn() -> bool,
) -> Result<ImportSummary, ImportError> {
    let source = db.get_source(source_id)?.ok_or_else(|| ImportError::NoSuchSource(source_id.into()))?;
    let location = source.location.clone().ok_or(ImportError::NoLocation)?;
    let connector = sources::by_connector(&source.connector)?;
    let path = Path::new(&location);

    let user_ids = db.user_identifier_set()?;
    if user_ids.is_empty() {
        return Err(ImportError::NoIdentity);
    }

    db.set_source_status(source_id, "importing", None)?;
    let mut state = ImportState {
        db,
        source_id: source_id.to_string(),
        default_channel: source.channel.clone(),
        joins_by_message_id: joins_by_message_id(&source.connector),
        user_ids,
        participants: HashMap::new(),
        summary: ImportSummary::default(),
        messages_done: 0,
    };

    let result = connector.import(path, &mut |convo| {
        if should_stop() {
            return Err(SourceError::Aborted("canceled".into()));
        }
        state.take(convo).map_err(|e| SourceError::Aborted(e.to_string()))?;
        on_progress(state.summary.conversations, state.messages_done);
        Ok(())
    });

    match result {
        Ok(()) => {}
        Err(SourceError::Aborted(msg)) if msg == "canceled" => {
            // Everything written so far stays; the source goes back to ready
            // so the user can resume by running the import again.
            db.refresh_source_counts(source_id)?;
            db.set_source_status(source_id, "ready", None)?;
            return Err(ImportError::Canceled);
        }
        Err(e) => {
            db.set_source_status(source_id, "failed", Some(&json!({"message": e.to_string()})))?;
            return Err(e.into());
        }
    }

    db.refresh_source_counts(source_id)?;
    db.set_source_status(source_id, "imported", None)?;
    // Anything derived from messages is now out of date.
    db.mark_profiles_stale(None)?;
    Ok(state.summary)
}

/// An import in progress, for sources that are not a file a connector walks —
/// a mailbox on a server hands conversations over a few at a time, and each
/// goes through exactly the same attribution, dedupe and threading as a file
/// import does.
pub struct Importer<'a> {
    state: ImportState<'a>,
}

impl<'a> Importer<'a> {
    /// Start importing into an existing source. Refuses, like a file import,
    /// when no identity has been declared, because direction would be
    /// undecidable.
    pub fn begin(db: &'a Db, source_id: &str) -> Result<Self, ImportError> {
        let source = db.get_source(source_id)?.ok_or_else(|| ImportError::NoSuchSource(source_id.into()))?;
        let user_ids = db.user_identifier_set()?;
        if user_ids.is_empty() {
            return Err(ImportError::NoIdentity);
        }
        Ok(Self {
            state: ImportState {
                db,
                source_id: source_id.to_string(),
                default_channel: source.channel.clone(),
                joins_by_message_id: joins_by_message_id(&source.connector),
                user_ids,
                participants: HashMap::new(),
                summary: ImportSummary::default(),
                messages_done: 0,
            },
        })
    }

    /// Import one conversation — new, or more messages for one that already
    /// exists, which is joined exactly as a file import would join it.
    pub fn take(&mut self, convo: DiscoveredConversation) -> Result<(), ImportError> {
        self.state.take(convo)
    }

    pub fn summary(&self) -> &ImportSummary {
        &self.state.summary
    }

    /// Recount the source and mark anything derived from messages stale, as
    /// a finished file import does. Only when something was inserted: a check
    /// that found nothing new invalidates nothing.
    pub fn finish(self) -> Result<ImportSummary, ImportError> {
        let db = self.state.db;
        if self.state.summary.inserted > 0 {
            // "Last import" is when mail last came in, so a check that found
            // nothing new does not move it.
            db.refresh_source_counts(&self.state.source_id)?;
            db.mark_profiles_stale(None)?;
        }
        Ok(self.state.summary)
    }
}

/// Sources whose message ids are real RFC 5322 Message-IDs, unique across
/// every mailbox in the world, and so safe to join threads and drop
/// duplicates by across sources. A `mimic_json` export may say `email` and
/// number its messages "a1", "a2"; those ids mean nothing outside the file.
pub const MESSAGE_ID_CONNECTORS: [&str; 2] = ["mbox", "imap"];

fn joins_by_message_id(connector: &str) -> bool {
    MESSAGE_ID_CONNECTORS.contains(&connector)
}

struct ImportState<'a> {
    db: &'a Db,
    source_id: String,
    default_channel: String,
    joins_by_message_id: bool,
    user_ids: HashSet<String>,
    /// Author identity key -> participant id (or `None` for the user).
    participants: HashMap<String, Option<String>>,
    summary: ImportSummary,
    messages_done: usize,
}

impl ImportState<'_> {
    fn take(&mut self, convo: DiscoveredConversation) -> Result<(), ImportError> {
        let channel = if crate::db::channel_is_known(&convo.channel) {
            convo.channel.clone()
        } else {
            self.default_channel.clone()
        };
        // Email carries a Message-ID that is unique across every mailbox in
        // the world, so a thread is joined wherever it already is: an earlier
        // check of the same mailbox, or an export imported as another source.
        // Without this a reply arriving separately from what it answers
        // starts a conversation of its own, and the one it answered looks
        // unanswered forever.
        let is_email = channel == "email" && self.joins_by_message_id;
        let ids: Vec<String> = convo.messages.iter().map(|m| m.external_id.clone()).collect();
        let joined = if is_email {
            let mut wanted = ids.clone();
            for m in &convo.messages {
                if let Some(refs) = m.metadata.get("refs").and_then(|r| r.as_array()) {
                    wanted.extend(refs.iter().filter_map(|r| r.as_str().map(str::to_string)));
                }
            }
            self.db.email_conversation_holding(&wanted)?
        } else {
            None
        };
        let conversation_id = match &joined {
            Some(id) => id.clone(),
            None => {
                self.db.upsert_conversation(&self.source_id, &convo.external_id, &channel, convo.subject.as_deref())?
            }
        };
        // The same message imported through another source (an export and
        // the connected mailbox it came from) is one message, not two: two
        // copies would count the user's own writing twice.
        let elsewhere =
            if is_email { self.db.email_ids_in_other_sources(&self.source_id, &ids)? } else { HashSet::new() };
        // What this source has stored already — an earlier import of the same
        // file, an earlier check of the same mailbox — is known before anyone
        // is attributed for it: a copy whose author reads differently this
        // time (a new name, another address) makes up nobody.
        let already = self.db.ids_in_source(&self.source_id, &ids)?;
        // A conversation already here that is not joined by Message-ID — a
        // later export of the same chat — gets its new messages after the
        // ones it has: numbered from zero, they would sit among the old ones,
        // and the wrong message would decide the thread.
        let after = if joined.is_none() { self.db.last_position(&conversation_id)? } else { None };
        let first = after.map_or(0, |last| last + 1);

        let mut batch: Vec<NewMessage> = Vec::with_capacity(convo.messages.len().min(BATCH));
        let mut counts = ImportCounts::default();
        let mut seen: HashSet<&str> = HashSet::new();
        for (i, raw) in convo.messages.iter().enumerate() {
            // Kept once: a copy another source has, or a second copy here —
            // the same message read from two folders, where the connector
            // puts the copy to keep first. Nobody is made up for the other.
            if elsewhere.contains(&raw.external_id)
                || already.contains(&raw.external_id)
                || !seen.insert(raw.external_id.as_str())
            {
                counts.duplicates += 1;
                continue;
            }
            let (direction, participant_id) = self.attribute(&raw.author)?;
            if let Some(pid) = &participant_id {
                self.db.link_conversation_participant(&conversation_id, pid)?;
            }
            match direction {
                Direction::Self_ => self.summary.from_self += 1,
                Direction::Unknown => self.summary.unattributed += 1,
                Direction::Other => {}
            }
            batch.push(NewMessage {
                conversation_id: conversation_id.clone(),
                source_id: self.source_id.clone(),
                participant_id,
                external_id: raw.external_id.clone(),
                direction: direction.as_str().to_string(),
                channel: channel.clone(),
                sent_at: raw.sent_at.clone(),
                sequence_index: first + i as i64,
                body: raw.body.clone(),
                reply_to_external_id: None,
                metadata: raw.metadata.clone(),
            });
            if batch.len() >= BATCH {
                counts = add(counts, self.db.insert_messages(&batch)?);
                batch.clear();
            }
        }
        if !batch.is_empty() {
            counts = add(counts, self.db.insert_messages(&batch)?);
        }
        if joined.is_some() && counts.inserted > 0 {
            // Messages numbered from zero have just joined a conversation that
            // already had some; put it back in time order, because "who spoke
            // last" is read from that order. A re-import that added nothing
            // leaves the order alone.
            self.db.resequence_by_time(&conversation_id)?;
        } else if after.is_some() && counts.inserted > 0 && self.db.every_message_timed(&conversation_id)? {
            // New messages went after the old; when every message says when
            // it was sent in a form that reads as a time, time decides
            // instead. Otherwise the order given stands: a message with no
            // time would go first, and a time read as text can be wrong.
            self.db.resequence_by_time(&conversation_id)?;
        } else if joined.is_none() {
            self.db.link_replies(&conversation_id)?;
            self.db.refresh_conversation_stats(&conversation_id)?;
        }

        self.summary.conversations += 1;
        self.summary.inserted += counts.inserted;
        self.summary.duplicates += counts.duplicates;
        self.summary.empty += counts.empty;
        self.messages_done += counts.inserted + counts.duplicates + counts.empty;
        Ok(())
    }

    /// Decide who wrote a message. The user's own messages carry no
    /// participant: they are not someone the user talks to.
    fn attribute(&mut self, author: &AuthorRef) -> Result<(Direction, Option<String>), ImportError> {
        if !author.is_usable() {
            return Ok((Direction::Unknown, None));
        }
        let key =
            author.identifiers.iter().filter(|i| !i.normalized().is_empty()).map(|i| i.key()).min().unwrap_or_default();
        if let Some(cached) = self.participants.get(&key) {
            return Ok(match cached {
                Some(id) => (Direction::Other, Some(id.clone())),
                None => (Direction::Self_, None),
            });
        }
        let is_self = author.identifiers.iter().any(|i| self.user_ids.contains(&i.key()));
        if is_self {
            self.participants.insert(key, None);
            return Ok((Direction::Self_, None));
        }
        let before = self.db.count_participants()?;
        let id = self.db.resolve_participant(&author.display_name, &author.identifiers, false)?;
        if self.db.count_participants()? > before {
            self.summary.participants_created += 1;
        }
        self.participants.insert(key, Some(id.clone()));
        Ok((Direction::Other, Some(id)))
    }
}

fn add(a: ImportCounts, b: ImportCounts) -> ImportCounts {
    ImportCounts {
        inserted: a.inserted + b.inserted,
        duplicates: a.duplicates + b.duplicates,
        empty: a.empty + b.empty,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("db: {0}")]
    Db(#[from] crate::db::DbError),
    #[error("source: {0}")]
    Source(#[from] SourceError),
    #[error("no source with id {0}")]
    NoSuchSource(String),
    #[error("this source has no location to read from")]
    NoLocation,
    #[error("tell Mimic which addresses are yours before importing, or every message will import as 'unknown'")]
    NoIdentity,
    #[error("canceled")]
    Canceled,
}

impl From<ImportError> for JobError {
    fn from(e: ImportError) -> Self {
        match e {
            ImportError::Canceled => JobError::Canceled,
            ImportError::Db(d) => JobError::Db(d),
            ImportError::Source(s) => JobError::Source(s),
            other => JobError::Failed(other.to_string()),
        }
    }
}

/// Job wrapper. Resumable, because re-running an interrupted import skips
/// everything already written.
pub struct ImportExecutor;

impl ImportExecutor {
    pub fn shared() -> Arc<dyn JobExecutor> {
        Arc::new(ImportExecutor)
    }
}

impl JobExecutor for ImportExecutor {
    fn kinds(&self) -> &'static [&'static str] {
        &[JOB_KIND]
    }

    fn resumable(&self, _kind: &str) -> bool {
        true
    }

    fn execute(&self, ctx: JobContext) -> JobFuture {
        Box::pin(async move {
            let source_id = ctx.job.payload["sourceId"]
                .as_str()
                .ok_or_else(|| JobError::Failed("import job needs a sourceId".into()))?
                .to_string();
            let db = ctx.db.clone();
            let progress_ctx = ctx.clone();
            // Total is unknown until the connector has been walked, so
            // progress reports items done against a moving total rather than
            // a fabricated percentage.
            let mut on_progress = move |conversations: usize, messages: usize| {
                progress_ctx.progress(messages as i64, 0, &format!("{conversations} conversations"));
            };
            let cancel_ctx = ctx.clone();
            let should_stop = move || cancel_ctx.check_cancel().is_err();
            let summary =
                tokio::task::block_in_place(|| import_source(&db, &source_id, &mut on_progress, &should_stop))?;
            Ok(serde_json::to_value(summary).unwrap_or(serde_json::Value::Null))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{IdentifierKind, NewSource};

    const EXPORT: &str = r#"{
      "channel": "chat",
      "conversations": [
        { "id": "t1", "subject": "Lunch", "messages": [
          { "id": "m1", "sentAt": "2026-02-03T09:14:00Z", "from": {"name":"Ada","handle":"@ada"}, "body": "Does Tuesday work?" },
          { "id": "m2", "sentAt": "2026-02-03T09:20:00Z", "from": {"name":"C","handle":"@c"}, "body": "yeah tuesday's good" },
          { "id": "m3", "sentAt": "2026-02-03T09:21:00Z", "from": {"name":"Ada","handle":"@ADA"}, "body": "great" },
          { "id": "m4", "from": {"name":"A ghost"}, "body": "who wrote this" }
        ]},
        { "id": "t2", "subject": "Numbers", "messages": [
          { "id": "m5", "sentAt": "2026-02-04T09:00:00Z", "from": {"handle":"@bob"}, "body": "numbers?" },
          { "id": "m6", "sentAt": "2026-02-04T09:01:00Z", "from": {"handle":"@c"}, "body": "sending" }
        ]}
      ]
    }"#;

    fn setup() -> (tempfile::TempDir, Db, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("export.json");
        std::fs::write(&path, EXPORT).unwrap();
        let db = Db::open_in_memory().unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::Handle, "@c").unwrap();
        let source = db
            .create_source(&NewSource {
                connector: "mimic_json".into(),
                name: "Chat".into(),
                channel: "chat".into(),
                location: Some(path.to_string_lossy().into()),
                config: serde_json::Value::Null,
            })
            .unwrap();
        (dir, db, source.id)
    }

    fn run(db: &Db, source_id: &str) -> Result<ImportSummary, ImportError> {
        import_source(db, source_id, &mut |_, _| {}, &|| false)
    }

    #[test]
    fn an_import_attributes_every_message_and_creates_the_people() {
        let (_d, db, source_id) = setup();
        let s = run(&db, &source_id).unwrap();
        assert_eq!(s.conversations, 2);
        assert_eq!(s.inserted, 6);
        assert_eq!(s.from_self, 2, "@c is the user, in either conversation");
        assert_eq!(s.unattributed, 1, "the author with no address stays unknown");
        assert_eq!(s.participants_created, 2, "Ada and Bob; @ADA is Ada again");

        assert_eq!(db.count_self_messages(None, None).unwrap(), 2);
        assert_eq!(db.get_source(&source_id).unwrap().unwrap().status, "imported");
        assert_eq!(db.get_source(&source_id).unwrap().unwrap().message_count, 6);

        let people = db.list_participants(10).unwrap();
        assert_eq!(people.len(), 2);
        let ada = people.iter().find(|p| p.participant.display_name == "Ada").unwrap();
        assert_eq!(ada.message_count, 4, "every message in the thread she is part of");
        assert_eq!(ada.sent_by_user, 1);
        assert_eq!(ada.channels, vec!["chat"]);
    }

    #[test]
    fn importing_twice_changes_nothing() {
        let (_d, db, source_id) = setup();
        run(&db, &source_id).unwrap();
        let second = run(&db, &source_id).unwrap();
        assert_eq!(second.inserted, 0);
        assert_eq!(second.duplicates, 6);
        assert_eq!(second.participants_created, 0);
        assert_eq!(db.get_source(&source_id).unwrap().unwrap().message_count, 6);
        assert_eq!(db.list_participants(10).unwrap().len(), 2);
    }

    /// The order a conversation's messages are in, by position.
    fn order(db: &Db, conversation: &str) -> Vec<String> {
        let conn = db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT m.external_id FROM messages m JOIN conversations c ON c.id = m.conversation_id
                 WHERE c.external_id = ?1 ORDER BY m.sequence_index, m.id",
            )
            .unwrap();
        let mut ids = Vec::new();
        for id in stmt.query_map([conversation], |r| r.get::<_, String>(0)).unwrap() {
            ids.push(id.unwrap());
        }
        ids
    }

    #[test]
    fn a_later_export_goes_after_what_was_there_and_the_newest_message_decides() {
        let (dir, db, source_id) = setup();
        // Fixed dates: open the waiting window to any age.
        db.set_setting(crate::db::WITHIN_DAYS_SETTING, &0).unwrap();
        run(&db, &source_id).unwrap();
        assert!(db.threads_awaiting_reply(10).unwrap().iter().all(|t| t.last_message != "numbers?"), "answered");

        // A later export of the same chats, holding only what is new: Bob
        // writes again, and someone adds to the lunch thread with no date.
        std::fs::write(
            dir.path().join("export.json"),
            r#"{ "channel": "chat", "conversations": [
              { "id": "t2", "messages": [
                { "id": "m7", "sentAt": "2026-02-05T10:00:00Z", "from": {"handle":"@bob"}, "body": "and the totals?" } ]},
              { "id": "t1", "messages": [
                { "id": "m8", "from": {"name":"Ada","handle":"@ada"}, "body": "one more thing" } ]}
            ]}"#,
        )
        .unwrap();
        let s = run(&db, &source_id).unwrap();
        assert_eq!((s.inserted, s.duplicates), (2, 0));
        assert_eq!(order(&db, "t2"), ["m5", "m6", "m7"], "after what was there, and in time order");
        assert_eq!(order(&db, "t1"), ["m1", "m2", "m3", "m4", "m8"], "undated: after what was there, as given");
        let waiting: Vec<String> = db.threads_awaiting_reply(10).unwrap().into_iter().map(|t| t.last_message).collect();
        assert!(waiting.contains(&"and the totals?".to_string()), "Bob's new question decides his thread: {waiting:?}");
        assert!(waiting.contains(&"one more thing".to_string()), "{waiting:?}");
    }

    #[test]
    fn times_are_read_as_times_and_times_that_cannot_be_read_leave_the_order_given() {
        let (dir, db, source_id) = setup();
        let path = dir.path().join("export.json");
        // t3's times are a person's dates, not ones a clock can read; t4's
        // are real times with different offsets, where text order is wrong:
        // 10:00+01:00 is 09:00Z, before 09:30Z.
        std::fs::write(
            &path,
            r#"{ "channel": "chat", "conversations": [
              { "id": "t3", "messages": [
                { "id": "a1", "sentAt": "03/02/2026 09:14", "from": {"handle":"@ada"}, "body": "lunch?" },
                { "id": "a2", "sentAt": "03/02/2026 10:02", "from": {"handle":"@c"}, "body": "yes" } ]},
              { "id": "t4", "messages": [
                { "id": "b1", "sentAt": "2026-02-03T10:00:00+01:00", "from": {"handle":"@bob"}, "body": "call?" },
                { "id": "b2", "sentAt": "2026-02-03T09:30:00Z", "from": {"handle":"@c"}, "body": "sure" } ]}
            ]}"#,
        )
        .unwrap();
        run(&db, &source_id).unwrap();
        std::fs::write(
            &path,
            r#"{ "channel": "chat", "conversations": [
              { "id": "t3", "messages": [
                { "id": "a3", "sentAt": "03/02/2026 10:05", "from": {"handle":"@ada"}, "body": "where?" } ]},
              { "id": "t4", "messages": [
                { "id": "b3", "sentAt": "2026-02-03T09:45:00Z", "from": {"handle":"@bob"}, "body": "now?" } ]}
            ]}"#,
        )
        .unwrap();
        run(&db, &source_id).unwrap();
        assert_eq!(order(&db, "t3"), ["a1", "a2", "a3"], "kept as given, the new one after");
        assert_eq!(order(&db, "t4"), ["b1", "b2", "b3"], "in time order, read as times");
    }

    #[test]
    fn a_message_read_again_with_its_author_written_differently_makes_up_nobody() {
        let (dir, db, source_id) = setup();
        run(&db, &source_id).unwrap();
        let links = |db: &Db| -> i64 {
            db.conn().query_row("SELECT COUNT(*) FROM conversation_participants", [], |r| r.get(0)).unwrap()
        };
        let before = links(&db);
        // The same message in a later export, its author now shown by a new handle.
        std::fs::write(
            dir.path().join("export.json"),
            r#"{ "channel": "chat", "conversations": [
              { "id": "t1", "messages": [
                { "id": "m1", "sentAt": "2026-02-03T09:14:00Z", "from": {"name":"Ada (new phone)","handle":"@ada.new"}, "body": "Does Tuesday work?" } ]}
            ]}"#,
        )
        .unwrap();
        let s = run(&db, &source_id).unwrap();
        assert_eq!((s.inserted, s.duplicates, s.participants_created), (0, 1, 0));
        assert_eq!(db.list_participants(10).unwrap().len(), 2, "Ada and Bob, and nobody new");
        assert_eq!(links(&db), before);
    }

    #[test]
    fn importing_without_an_identity_is_refused_rather_than_guessed() {
        let (_d, _db, _s) = setup();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("export.json");
        std::fs::write(&path, EXPORT).unwrap();
        let db = Db::open_in_memory().unwrap();
        let source = db
            .create_source(&NewSource {
                connector: "mimic_json".into(),
                name: "Chat".into(),
                channel: "chat".into(),
                location: Some(path.to_string_lossy().into()),
                config: serde_json::Value::Null,
            })
            .unwrap();
        assert!(matches!(run(&db, &source.id), Err(ImportError::NoIdentity)));
        assert_eq!(db.count_self_messages(None, None).unwrap(), 0, "nothing was written");
    }

    #[test]
    fn cancelling_keeps_what_was_written_and_leaves_the_source_resumable() {
        let (_d, db, source_id) = setup();
        let seen = std::cell::Cell::new(0);
        let err = import_source(&db, &source_id, &mut |_, _| {}, &|| {
            let n = seen.get();
            seen.set(n + 1);
            n >= 1
        })
        .unwrap_err();
        assert!(matches!(err, ImportError::Canceled));
        assert_eq!(db.get_source(&source_id).unwrap().unwrap().status, "ready");
        // The first conversation is in; the second never started.
        assert_eq!(db.get_source(&source_id).unwrap().unwrap().message_count, 4);
        // Resuming finishes the job without duplicating anything.
        let s = run(&db, &source_id).unwrap();
        assert_eq!((s.inserted, s.duplicates), (2, 4));
    }

    #[test]
    fn a_missing_file_fails_the_source_with_a_readable_reason() {
        let db = Db::open_in_memory().unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::Handle, "@c").unwrap();
        let source = db
            .create_source(&NewSource {
                connector: "mimic_json".into(),
                name: "Gone".into(),
                channel: "chat".into(),
                location: Some("/nope/missing.json".into()),
                config: serde_json::Value::Null,
            })
            .unwrap();
        assert!(run(&db, &source.id).is_err());
        let s = db.get_source(&source.id).unwrap().unwrap();
        assert_eq!(s.status, "failed");
        assert!(s.last_error.unwrap()["message"].as_str().unwrap().contains("io:"));
    }

    #[test]
    fn importing_marks_derived_profiles_stale() {
        let (_d, db, source_id) = setup();
        db.put_voice_profile(
            crate::db::VoiceLayer::Global,
            "",
            None,
            &json!({}),
            &json!({}),
            1,
            crate::version::ANALYSIS_VERSION,
        )
        .unwrap();
        run(&db, &source_id).unwrap();
        let profiles = db.list_voice_profiles(crate::version::ANALYSIS_VERSION).unwrap();
        assert!(profiles[0].stale, "new messages invalidate what was computed before them");
    }
}
