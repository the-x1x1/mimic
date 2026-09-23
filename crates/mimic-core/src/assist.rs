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
//! * **It is bounded.** At most `MAX_PER_RUN` threads per run, newest first,
//!   and never a thread that already has an unresolved draft for its last
//!   message. A mailbox import of forty thousand conversations must not turn
//!   into forty thousand provider calls.
//! * **It drafts only for what is waiting.** The same definition as the home
//!   screen (`db::repo_waiting`): mail that looks automated from its headers
//!   and threads the user said need no reply are not drafted for, so a run
//!   spends its ten on people rather than on newsletters.
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

/// How far down the waiting list a run looks for threads without a draft.
/// Past the ones it already wrote, so a second run reaches the next ten
/// instead of finding the first ten done and stopping.
const SCAN_LIMIT: usize = 500;

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

    let want = limit.min(MAX_PER_RUN);
    let mut candidates = Vec::with_capacity(want);
    for thread in db.threads_awaiting_reply(SCAN_LIMIT)? {
        if candidates.len() >= want {
            break;
        }
        // Any draft for the message waiting now — pending, used or turned
        // down — means it has been dealt with. A draft for an earlier message
        // does not.
        if !db.any_draft_for_message(&thread.conversation.id, &thread.last_message_id, &thread.last_message)? {
            candidates.push(thread);
        }
    }
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
            incoming_message_id: Some(thread.last_message_id.clone()),
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
        let waiting = &db.threads_awaiting_reply(1).unwrap()[0];
        assert_eq!(waiting.conversation.id, convo);
        let pending = db
            .pending_draft_for_message(&convo, &waiting.last_message_id, &waiting.last_message)
            .unwrap()
            .expect("a draft for the waiting thread");
        assert_eq!(pending.incoming_message_id.as_deref(), Some(waiting.last_message_id.as_str()));
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

    fn waiting_thread(db: &Db, source: &str, key: &str, body: &str, automated: Option<&str>) -> String {
        let convo = db.upsert_conversation(source, key, "chat", Some(key)).unwrap();
        db.insert_messages(&[NewMessage {
            conversation_id: convo.clone(),
            source_id: source.into(),
            participant_id: None,
            external_id: format!("{key}-1"),
            direction: "other".into(),
            channel: "chat".into(),
            sent_at: Some("2026-09-02T10:00:00Z".into()),
            sequence_index: 0,
            body: body.into(),
            reply_to_external_id: None,
            metadata: automated.map(|a| serde_json::json!({ "automated": a })).unwrap_or(Value::Null),
        }])
        .unwrap();
        convo
    }

    #[test]
    fn automated_mail_and_threads_taken_off_the_list_are_not_drafted_for() {
        let (db, source, _c, _a) = seed();
        db.set_setting(SETTING, &true).unwrap();
        waiting_thread(&db, &source, "news", "this week's deals", Some("newsletter"));
        let fyi = waiting_thread(&db, &source, "fyi", "fyi, minutes attached", None);
        let fyi_message: String =
            db.conn().query_row("SELECT id FROM messages WHERE conversation_id = ?1", [&fyi], |r| r.get(0)).unwrap();
        db.mark_thread(&fyi, &fyi_message, Some(crate::db::ThreadMark::NoReplyNeeded)).unwrap();

        let provider = MockProvider::default();
        let summary = draft_waiting_threads(&db, &provider, 10, &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(summary.considered, 1, "only the person asking for an invoice is waiting");
        assert_eq!(provider.call_count(), 1);
        let drafted = db.pending_drafts(10).unwrap();
        assert_eq!(drafted[0].incoming_message.as_deref(), Some("can you send the invoice today?"));
    }

    #[test]
    fn a_new_message_gets_a_new_draft() {
        let (db, source, convo, ada) = seed();
        db.set_setting(SETTING, &true).unwrap();
        let provider = MockProvider::default();
        draft_waiting_threads(&db, &provider, 10, &mut |_, _| {}, &|| false).unwrap();
        db.insert_messages(&[NewMessage {
            conversation_id: convo.clone(),
            source_id: source,
            participant_id: Some(ada),
            external_id: "m2".into(),
            direction: "other".into(),
            channel: "chat".into(),
            sent_at: Some("2026-09-01T12:00:00Z".into()),
            sequence_index: 1,
            body: "actually, next week is fine".into(),
            reply_to_external_id: None,
            metadata: Value::Null,
        }])
        .unwrap();

        // The first draft answered a question that is no longer the one
        // waiting, so it does not stop a draft for the new one.
        let second = draft_waiting_threads(&db, &provider, 10, &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(second.drafted, 1);
        let waiting = &db.threads_awaiting_reply(1).unwrap()[0];
        assert_eq!(waiting.last_message, "actually, next week is fine");
        assert!(db
            .pending_draft_for_message(&convo, &waiting.last_message_id, &waiting.last_message)
            .unwrap()
            .is_some());
    }

    #[test]
    fn a_draft_the_user_used_or_turned_down_is_not_written_again() {
        let (db, _s, _c, _a) = seed();
        db.set_setting(SETTING, &true).unwrap();
        let provider = MockProvider::default();
        draft_waiting_threads(&db, &provider, 10, &mut |_, _| {}, &|| false).unwrap();
        let draft = db.pending_drafts(10).unwrap().remove(0);
        // Used, but the reply they sent has not been imported yet, so the
        // thread is still waiting.
        db.resolve_draft(&draft.id, "sent_unedited", Some(&draft.generated_text)).unwrap();
        let again = draft_waiting_threads(&db, &provider, 10, &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(again.considered, 0);
        assert_eq!(provider.call_count(), 1);
    }

    #[test]
    fn a_draft_written_after_dropping_one_is_still_offered() {
        let (db, _s, convo, _a) = seed();
        db.set_setting(SETTING, &true).unwrap();
        let provider = MockProvider::default();
        draft_waiting_threads(&db, &provider, 10, &mut |_, _| {}, &|| false).unwrap();
        let prepared = db.pending_drafts(10).unwrap().remove(0);
        db.resolve_draft(&prepared.id, "discarded", None).unwrap();
        let waiting = db.threads_awaiting_reply(1).unwrap().remove(0);
        assert!(db
            .pending_draft_for_message(&convo, &waiting.last_message_id, &waiting.last_message)
            .unwrap()
            .is_none());

        // "Not this one", then the user says what they want and writes their own.
        let request = ComposeRequest {
            conversation_id: Some(convo.clone()),
            channel: "chat".into(),
            incoming_message: Some(waiting.last_message.clone()),
            incoming_message_id: Some(waiting.last_message_id.clone()),
            intent: Some("yes, sending it this afternoon".into()),
            ..Default::default()
        };
        let written = compose(&db, &provider, &request).unwrap();
        let offered = db.pending_draft_for_message(&convo, &waiting.last_message_id, &waiting.last_message).unwrap();
        assert_eq!(offered.map(|d| d.id), Some(written.id), "it survives the screen reloading");
    }

    #[test]
    fn two_messages_with_the_same_words_are_two_questions() {
        let (db, source, convo, ada) = seed();
        db.set_setting(SETTING, &true).unwrap();
        let provider = MockProvider::default();
        draft_waiting_threads(&db, &provider, 10, &mut |_, _| {}, &|| false).unwrap();
        // The user answers, and a week later Ada asks the same thing again.
        for (ext, dir, seq, who, body) in
            [("m2", "self", 1, None, "sent!"), ("m3", "other", 2, Some(ada.clone()), "can you send the invoice today?")]
        {
            db.insert_messages(&[NewMessage {
                conversation_id: convo.clone(),
                source_id: source.clone(),
                participant_id: who,
                external_id: ext.into(),
                direction: dir.into(),
                channel: "chat".into(),
                sent_at: Some(format!("2026-09-0{}T10:00:00Z", seq + 1)),
                sequence_index: seq,
                body: body.into(),
                reply_to_external_id: None,
                metadata: Value::Null,
            }])
            .unwrap();
        }
        let waiting = &db.threads_awaiting_reply(1).unwrap()[0];
        assert!(
            db.pending_draft_for_message(&convo, &waiting.last_message_id, &waiting.last_message).unwrap().is_none(),
            "the draft for the first time she asked is not shown under the second"
        );
        let second = draft_waiting_threads(&db, &provider, 10, &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(second.drafted, 1);
    }

    #[test]
    fn the_next_run_reaches_the_threads_the_last_one_did_not() {
        let (db, source, _c, _a) = seed();
        db.set_setting(SETTING, &true).unwrap();
        for i in 0..11 {
            waiting_thread(&db, &source, &format!("w{i}"), &format!("question {i}"), None);
        }
        let provider = MockProvider::default();
        let first = draft_waiting_threads(&db, &provider, 10, &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(first.drafted, MAX_PER_RUN);
        let second = draft_waiting_threads(&db, &provider, 10, &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(second.drafted, 2, "twelve waiting, ten done, two left");
        let third = draft_waiting_threads(&db, &provider, 10, &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(third.considered, 0);
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
