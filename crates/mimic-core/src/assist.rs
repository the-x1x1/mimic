//! Assisted drafting: preparing replies for threads that are waiting, without
//! being asked each time.
//!
//! This is the first piece of ASSISTED mode, and it is the one place in Mimic
//! where a model sees a message the user did not personally hand it. Three
//! things follow from that, and they are enforced here rather than in the UI:
//!
//! * **It is off unless the user turned it on.** `assist.autoDraft` is a
//!   setting that defaults to false. A run with it off does nothing and says
//!   so, so an accidental enqueue cannot leak a message to a provider.
//! * **It is bounded.** At most `MAX_PER_RUN` threads per run, oldest waiting
//!   first, and never a thread that already has an unresolved draft. A mailbox
//!   import of forty thousand conversations must not turn into forty thousand
//!   provider calls.
//! * **It drafts; it does not send.** The output is a `drafts` row with no
//!   outcome, exactly like a draft the user asked for by hand. Every reply
//!   still leaves through a person.
//!
//! There is deliberately no intent here. A draft prepared in advance has only
//! the incoming message and how the user writes to go on, which is weaker than
//! a draft the user framed themselves — the dashboard says so next to it.

use std::sync::Arc;

use serde::Serialize;
use serde_json::json;

use crate::db::{Db, DbError};
use crate::generation::{compose, ComposeRequest, GenerationError};
use crate::jobs::{JobContext, JobError, JobExecutor, JobFuture};
use crate::providers::ModelProvider;

pub const JOB_KIND: &str = "assist_drafts";

/// The setting that turns this on. Absent or false means off.
pub const SETTING: &str = "assist.autoDraft";

/// Per run, not per day: the runner is triggered after an import or an
/// analysis, and a bound per run is what keeps a large import from becoming a
/// large bill.
pub const MAX_PER_RUN: usize = 10;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistSummary {
    /// Threads that were waiting and had no draft yet.
    pub considered: usize,
    pub drafted: usize,
    pub failed: usize,
    /// Set when the run did nothing because the setting is off. The UI shows
    /// this instead of reporting a successful run that produced nothing.
    pub disabled: bool,
}

pub fn is_enabled(db: &Db) -> Result<bool, DbError> {
    Ok(db.get_setting::<bool>(SETTING)?.unwrap_or(false))
}

/// Draft replies for the threads that are waiting.
///
/// `should_stop` is checked between threads, so cancelling is immediate in
/// human terms and never leaves a half-written draft.
pub fn draft_waiting_threads(
    db: &Db,
    provider: &dyn ModelProvider,
    limit: usize,
    on_progress: &mut dyn FnMut(usize, usize),
    should_stop: &dyn Fn() -> bool,
) -> Result<AssistSummary, GenerationError> {
    let mut summary = AssistSummary::default();
    if !is_enabled(db)? {
        summary.disabled = true;
        return Ok(summary);
    }

    let waiting = db.threads_awaiting_reply(limit.min(MAX_PER_RUN))?;
    let candidates: Vec<_> = waiting
        .into_iter()
        .filter(|t| matches!(db.pending_draft_for_conversation(&t.conversation.id), Ok(None)))
        .collect();
    summary.considered = candidates.len();
    let total = candidates.len();

    for (i, thread) in candidates.into_iter().enumerate() {
        if should_stop() {
            break;
        }
        on_progress(i, total);
        let request = ComposeRequest {
            participant_id: thread.participant_id.clone(),
            conversation_id: Some(thread.conversation.id.clone()),
            channel: thread.conversation.channel.clone(),
            incoming_message: Some(thread.last_message.clone()),
            // No intent: nobody said what they want to say yet. The prompt
            // assembler already handles this, and the draft records it.
            intent: None,
            situation_id: None,
            adjustment: None,
        };
        match compose(db, provider, &request) {
            Ok(_) => summary.drafted += 1,
            // One unreachable provider or one refused request must not fail
            // the whole run; the count is reported and the thread is simply
            // still waiting.
            Err(_) => summary.failed += 1,
        }
    }
    on_progress(total, total);
    Ok(summary)
}

/// Runs `draft_waiting_threads` as a background job.
pub struct AssistExecutor {
    provider: Arc<dyn Fn() -> Option<Arc<dyn ModelProvider>> + Send + Sync>,
}

impl AssistExecutor {
    /// The provider is resolved at run time rather than captured, because the
    /// user can change it in Settings between runs.
    pub fn shared(provider: Arc<dyn Fn() -> Option<Arc<dyn ModelProvider>> + Send + Sync>) -> Arc<dyn JobExecutor> {
        Arc::new(AssistExecutor { provider })
    }
}

impl JobExecutor for AssistExecutor {
    fn kinds(&self) -> &'static [&'static str] {
        &[JOB_KIND]
    }

    fn resumable(&self, _kind: &str) -> bool {
        // Re-running is free — threads that got a draft are skipped — but it
        // is also not worth resuming automatically after a crash: the user
        // may not be at the machine, and this one spends money.
        false
    }

    fn execute(&self, ctx: JobContext) -> JobFuture {
        let resolve = self.provider.clone();
        Box::pin(async move {
            let db = ctx.db.clone();
            let progress_ctx = ctx.clone();
            let cancel_ctx = ctx.clone();
            let provider = resolve().ok_or_else(|| JobError::Failed("no model provider is configured".to_string()))?;
            let summary = tokio::task::block_in_place(move || {
                let mut on_progress =
                    |done: usize, total: usize| progress_ctx.progress(done as i64, total as i64, "drafting replies");
                let should_stop = || cancel_ctx.check_cancel().is_err();
                draft_waiting_threads(&db, provider.as_ref(), MAX_PER_RUN, &mut on_progress, &should_stop)
            })
            .map_err(|e| JobError::Failed(e.to_string()))?;
            Ok(serde_json::to_value(summary).unwrap_or(json!(null)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::repo_people::IdentifierInput;
    use crate::db::{IdentifierKind, NewMessage, NewSource};
    use crate::providers::mock::MockProvider;
    use serde_json::Value;

    fn seed() -> (Db, String, String, String) {
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
        db.insert_messages(&[NewMessage {
            conversation_id: convo.clone(),
            source_id: source.id.clone(),
            participant_id: Some(ada.clone()),
            external_id: "m1".into(),
            direction: "other".into(),
            channel: "chat".into(),
            sent_at: Some("2026-09-01T10:00:00Z".into()),
            sequence_index: 0,
            body: "can you send the invoice today?".into(),
            reply_to_external_id: None,
            metadata: Value::Null,
        }])
        .unwrap();
        db.refresh_conversation_stats(&convo).unwrap();
        (db, source.id, convo, ada)
    }

    #[test]
    fn nothing_is_drafted_while_the_setting_is_off() {
        let (db, _s, _c, _a) = seed();
        let provider = MockProvider::default();
        let summary = draft_waiting_threads(&db, &provider, 10, &mut |_, _| {}, &|| false).unwrap();
        assert!(summary.disabled, "the run must report why it did nothing");
        assert_eq!(summary.drafted, 0);
        assert!(db.pending_drafts(10).unwrap().is_empty());
        assert_eq!(provider.call_count(), 0, "a disabled run must not reach the provider at all");
    }

    #[test]
    fn a_waiting_thread_gets_one_draft_and_only_one() {
        let (db, _s, convo, _a) = seed();
        db.set_setting(SETTING, &true).unwrap();
        let provider = MockProvider::default();

        let first = draft_waiting_threads(&db, &provider, 10, &mut |_, _| {}, &|| false).unwrap();
        assert!(!first.disabled);
        assert_eq!(first.drafted, 1);
        let pending = db.pending_draft_for_conversation(&convo).unwrap().expect("a draft for the waiting thread");
        assert!(pending.outcome.is_none(), "a prepared draft is unresolved, never pre-approved");
        assert!(pending.intent.is_none(), "nobody stated an intent, so none is invented");

        // A second run leaves it alone rather than piling up drafts.
        let second = draft_waiting_threads(&db, &provider, 10, &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(second.considered, 0);
        assert_eq!(second.drafted, 0);
        assert_eq!(db.pending_drafts(10).unwrap().len(), 1);
        assert_eq!(provider.call_count(), 1);
    }

    #[test]
    fn an_answered_thread_is_not_drafted_for() {
        let (db, source, convo, _a) = seed();
        db.set_setting(SETTING, &true).unwrap();
        db.insert_messages(&[NewMessage {
            conversation_id: convo.clone(),
            source_id: source,
            participant_id: None,
            external_id: "m2".into(),
            direction: "self".into(),
            channel: "chat".into(),
            sent_at: Some("2026-09-01T11:00:00Z".into()),
            sequence_index: 1,
            body: "sent it over".into(),
            reply_to_external_id: None,
            metadata: Value::Null,
        }])
        .unwrap();
        let provider = MockProvider::default();
        let summary = draft_waiting_threads(&db, &provider, 10, &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(summary.considered, 0);
        assert_eq!(provider.call_count(), 0);
    }

    #[test]
    fn cancelling_stops_between_threads() {
        let (db, _s, _c, _a) = seed();
        db.set_setting(SETTING, &true).unwrap();
        let provider = MockProvider::default();
        let summary = draft_waiting_threads(&db, &provider, 10, &mut |_, _| {}, &|| true).unwrap();
        assert_eq!(summary.drafted, 0);
        assert_eq!(provider.call_count(), 0, "a cancelled run reaches no provider");
    }
}
