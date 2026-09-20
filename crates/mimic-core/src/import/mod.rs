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

struct ImportState<'a> {
    db: &'a Db,
    source_id: String,
    default_channel: String,
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
        let conversation_id =
            self.db.upsert_conversation(&self.source_id, &convo.external_id, &channel, convo.subject.as_deref())?;

        let mut batch: Vec<NewMessage> = Vec::with_capacity(convo.messages.len().min(BATCH));
        let mut counts = ImportCounts::default();
        for (i, raw) in convo.messages.iter().enumerate() {
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
                sequence_index: i as i64,
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
        self.db.link_replies(&conversation_id)?;
        self.db.refresh_conversation_stats(&conversation_id)?;

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
