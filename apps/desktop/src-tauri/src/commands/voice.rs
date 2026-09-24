//! The Voice screen: what Mimic has measured, and recomputing it.

use serde_json::{json, Value};
use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

#[tauri::command]
pub async fn get_voice_overview(state: State<'_, SharedState>) -> CommandResult<mimic_core::voice::VoiceOverview> {
    Ok(mimic_core::voice::overview(&state.db)?)
}

#[tauri::command]
pub async fn start_voice_analysis(state: State<'_, SharedState>) -> CommandResult<mimic_core::db::Job> {
    if state.db.count_self_messages(None, None)? == 0 {
        return Err(CommandError::new(
            "nothing_to_analyze",
            "Import some of your own messages first. Mimic learns from what you have written, not from what you have received.",
        ));
    }
    Ok(state.jobs.enqueue(mimic_core::voice::JOB_KIND, json!({}))?)
}

/// The situation vocabulary, with how many of the user's own messages are
/// filed under each. Always all six, so the screen can show what has nothing
/// filed yet rather than hiding it.
#[tauri::command]
pub async fn list_situations(
    state: State<'_, SharedState>,
) -> CommandResult<Vec<mimic_core::situations::SituationSummary>> {
    Ok(mimic_core::situations::overview(&state.db)?)
}

/// What Mimic has learned from the drafts the user sent, and what they have
/// told it directly.
#[tauri::command]
pub async fn get_learning_overview(
    state: State<'_, SharedState>,
) -> CommandResult<mimic_core::learning::LearningOverview> {
    Ok(mimic_core::learning::overview(&state.db)?)
}

/// The latest measurement of the drafts against what the user wrote, worked
/// out from the cases still here. `None` before the first.
#[tauri::command]
pub async fn get_evaluation(
    state: State<'_, SharedState>,
) -> CommandResult<Option<mimic_core::evaluation::EvaluationView>> {
    Ok(mimic_core::evaluation::latest(&state.db)?)
}

/// Measure the drafts, in the background. Refused up front when it could not
/// run — no engine, or no model to write with — so the button doesn't queue
/// something that is bound to fail.
#[tauri::command]
pub async fn start_evaluation(state: State<'_, SharedState>) -> CommandResult<mimic_core::db::Job> {
    if !state.engine.is_ready() {
        return Err(CommandError::new(
            "engine_unavailable",
            "The part of Mimic that does the measuring isn't running, so this can't be measured right now. Settings → Diagnostics can restart it.",
        ));
    }
    state.active_provider()?;
    // One at a time: a second would spend the same calls again for nothing.
    if state.db.list_jobs(50, true)?.iter().any(|j| j.kind == mimic_core::evaluation::JOB_KIND) {
        return Err(CommandError::new("already_running", "I'm already measuring my drafts."));
    }
    Ok(state.jobs.enqueue(mimic_core::evaluation::JOB_KIND, json!({}))?)
}

/// Stop a measurement of the drafts that is queued or running. Called before
/// anything is deleted: what it has written so far may come from the mail
/// being deleted, and a stopped run records nothing.
pub(crate) fn stop_measuring(state: &SharedState) {
    let Ok(active) = state.db.list_jobs(50, true) else { return };
    for job in active.iter().filter(|j| j.kind == mimic_core::evaluation::JOB_KIND) {
        if let Err(e) = state.jobs.cancel(&job.id) {
            tracing::warn!(target: "jobs", error = %e, "a measurement of the drafts could not be stopped");
        }
    }
}

/// After new messages came in: when how the user writes was measured before
/// and something it was measured over has changed, measure that again — only
/// that, and once, if a measurement is not already waiting to run. Whether
/// one is waiting now.
pub(crate) fn measure_what_changed(db: &mimic_core::db::Db, jobs: &mimic_core::jobs::JobRunner) -> bool {
    let wanted = mimic_core::voice::wants_measuring(db);
    // A measurement of everything waiting to run measures what changed too.
    let waiting = db.list_jobs(50, true).map(|active| {
        active.iter().any(|j| {
            j.status == "queued"
                && (j.kind == mimic_core::voice::CHANGED_JOB_KIND || j.kind == mimic_core::voice::JOB_KIND)
        })
    });
    match (wanted, waiting) {
        (Ok(false), _) => false,
        (Ok(true), Ok(true)) => true,
        (Ok(true), Ok(false)) => match jobs.enqueue(mimic_core::voice::CHANGED_JOB_KIND, json!({})) {
            Ok(_) => true,
            Err(e) => {
                tracing::warn!(target: "voice", error = %e, "could not queue measuring what changed");
                false
            }
        },
        (Err(e), _) | (_, Err(e)) => {
            tracing::warn!(target: "voice", error = %e, "could not tell whether anything wants measuring");
            false
        }
    }
}

/// Have the chosen provider put each measured layer into words, from its
/// numbers alone. Refused before anything is measured; one run at a time.
#[tauri::command]
pub async fn start_describing_voice(state: State<'_, SharedState>) -> CommandResult<mimic_core::db::Job> {
    let measured = mimic_core::voice::overview(&state.db)?.profiles.iter().any(|p| p.measurable);
    if !measured {
        return Err(CommandError::new(
            "nothing_to_describe",
            "Nothing has been measured yet, so there is nothing to put into words.",
        ));
    }
    state.active_provider()?;
    if let Some(active) =
        state.db.list_jobs(50, true)?.into_iter().find(|j| j.kind == mimic_core::voice::describe::JOB_KIND)
    {
        return Ok(active);
    }
    Ok(state.jobs.enqueue(mimic_core::voice::describe::JOB_KIND, json!({}))?)
}

/// What the user's own messages are filed under as doing, and by whom, and
/// the model on this computer that could read them, if there is one.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SituationFiling {
    #[serde(flatten)]
    pub counts: mimic_core::db::FilingCounts,
    /// The provider on this computer that would read them, so the screen can
    /// check that it answers before offering it.
    pub local_provider: Option<String>,
    /// The name of its model.
    pub local_model: Option<String>,
}

#[tauri::command]
pub async fn get_situation_filing(state: State<'_, SharedState>) -> CommandResult<SituationFiling> {
    Ok(situation_filing(&state.db, state.local_provider().map(|p| p.info()))?)
}

fn situation_filing(
    db: &mimic_core::db::Db,
    local: Option<mimic_core::providers::ProviderInfo>,
) -> Result<SituationFiling, mimic_core::db::DbError> {
    Ok(SituationFiling {
        counts: db.filing_counts()?,
        local_provider: local.as_ref().map(|p| p.id.clone()),
        local_model: local.map(|p| p.model),
    })
}

/// Have the model on this computer read what the user's messages are doing.
/// Refused without one: a hosted model is never sent them all. One reading
/// at a time.
#[tauri::command]
pub async fn start_reading_situations(state: State<'_, SharedState>) -> CommandResult<mimic_core::db::Job> {
    if state.local_provider().is_none() {
        return Err(CommandError::new("no_local_model", mimic_core::situations::NOT_LOCAL));
    }
    if let Some(active) =
        state.db.list_jobs(50, true)?.into_iter().find(|j| j.kind == mimic_core::situations::READ_JOB_KIND)
    {
        return Ok(active);
    }
    Ok(state.jobs.enqueue(mimic_core::situations::READ_JOB_KIND, json!({}))?)
}

/// The user says what one of their messages is doing: these situations, or
/// none. What it changed is measured again.
#[tauri::command]
pub async fn decide_situations(
    state: State<'_, SharedState>,
    message_id: String,
    situation_ids: Vec<String>,
) -> CommandResult<mimic_core::db::Filing> {
    let filing = mimic_core::situations::decide(&state.db, &message_id, &situation_ids)?;
    measure_what_changed(&state.db, &state.jobs);
    Ok(filing)
}

/// The user hands one of their messages back to the rules.
#[tauri::command]
pub async fn let_rules_decide(
    state: State<'_, SharedState>,
    message_id: String,
) -> CommandResult<mimic_core::db::Filing> {
    let filing = mimic_core::situations::let_rules_decide(&state.db, &message_id)?;
    measure_what_changed(&state.db, &state.jobs);
    Ok(filing)
}

/// Remember something the user typed, about one person or everyone.
#[tauri::command]
pub async fn add_voice_note(
    state: State<'_, SharedState>,
    participant_id: Option<String>,
    note: String,
) -> CommandResult<mimic_core::db::VoicePreference> {
    Ok(mimic_core::learning::remember_note(&state.db, participant_id.as_deref(), &note)?)
}

#[tauri::command]
pub async fn get_voice_profile(
    state: State<'_, SharedState>,
    layer: String,
    scope_key: String,
) -> CommandResult<Option<mimic_core::db::VoiceProfileRow>> {
    let layer = mimic_core::db::VoiceLayer::parse(&layer)
        .ok_or_else(|| CommandError::new("invalid", format!("{layer:?} is not a voice layer")))?;
    Ok(state.db.get_voice_profile(layer, &scope_key, mimic_core::version::ANALYSIS_VERSION)?)
}

#[tauri::command]
pub async fn list_voice_examples(
    state: State<'_, SharedState>,
    layer: String,
    scope_key: String,
    limit: Option<usize>,
) -> CommandResult<Vec<mimic_core::db::RepresentativeExample>> {
    let layer = mimic_core::db::VoiceLayer::parse(&layer)
        .ok_or_else(|| CommandError::new("invalid", format!("{layer:?} is not a voice layer")))?;
    Ok(state.db.representative_examples(layer, &scope_key, limit.unwrap_or(10).min(50))?)
}

#[tauri::command]
pub async fn set_voice_preference(
    state: State<'_, SharedState>,
    layer: String,
    scope_key: String,
    key: String,
    value: Value,
    note: Option<String>,
) -> CommandResult<mimic_core::db::VoicePreference> {
    let layer = mimic_core::db::VoiceLayer::parse(&layer)
        .ok_or_else(|| CommandError::new("invalid", format!("{layer:?} is not a voice layer")))?;
    Ok(state.db.set_voice_preference(layer, &scope_key, &key, &value, note.as_deref())?)
}

#[tauri::command]
pub async fn list_voice_preferences(
    state: State<'_, SharedState>,
    layer: String,
    scope_key: String,
) -> CommandResult<Vec<mimic_core::db::VoicePreference>> {
    let layer = mimic_core::db::VoiceLayer::parse(&layer)
        .ok_or_else(|| CommandError::new("invalid", format!("{layer:?} is not a voice layer")))?;
    Ok(state.db.voice_preferences_for(&[(layer, scope_key)])?)
}

#[tauri::command]
pub async fn delete_voice_preference(state: State<'_, SharedState>, preference_id: String) -> CommandResult<()> {
    state.db.delete_voice_preference(&preference_id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimic_core::db::{Db, IdentifierKind, NewMessage, NewSource};
    use mimic_core::jobs::JobRunner;

    fn with_messages(n: usize) -> (Db, JobRunner) {
        let db = Db::open_in_memory().unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::Handle, "@c").unwrap();
        let source = db
            .create_source(&NewSource {
                connector: "test".into(),
                name: "T".into(),
                channel: "chat".into(),
                location: None,
                config: Value::Null,
            })
            .unwrap()
            .id;
        let convo = db.upsert_conversation(&source, "t", "chat", None).unwrap();
        let batch: Vec<NewMessage> = (0..n)
            .map(|i| NewMessage {
                conversation_id: convo.clone(),
                source_id: source.clone(),
                participant_id: None,
                external_id: format!("m{i}"),
                direction: "self".into(),
                channel: "chat".into(),
                sent_at: Some(format!("2026-09-{:02}T10:00:00Z", (i % 27) + 1)),
                sequence_index: i as i64,
                body: format!("that works for me, number {i}"),
                reply_to_external_id: None,
                metadata: Value::Null,
            })
            .collect();
        db.insert_messages(&batch).unwrap();
        let jobs = JobRunner::new(
            db.clone(),
            std::sync::Arc::new(mimic_core::jobs::CompositeExecutor::new(vec![
                mimic_core::voice::AnalyzeExecutor::shared(),
            ])),
        );
        (db, jobs)
    }

    fn queued(db: &Db) -> Vec<String> {
        db.list_jobs(10, true).unwrap().into_iter().filter(|j| j.status == "queued").map(|j| j.kind).collect()
    }

    #[test]
    fn how_messages_came_to_be_filed_is_counted_and_matches_its_fixture() {
        let (db, _) = with_messages(3);
        let first = db.page_self_messages(None, None, None, 1).unwrap().remove(0);
        mimic_core::situations::decide(&db, &first.id, &["thanking".to_string()]).unwrap();
        let local = mimic_core::providers::mock::MockProvider::named("local", true);
        use mimic_core::providers::ModelProvider;
        let view = situation_filing(&db, Some(local.info())).unwrap();
        assert_eq!((view.counts.by_rules, view.counts.by_model, view.counts.by_you), (2, 0, 1));
        assert_eq!(view.local_provider.as_deref(), Some("local"));
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["byYou"], 1, "the counts sit beside the model, not under a key");
        crate::fixtures::check_fixture("situation_filing.json", &json);
        assert_eq!(situation_filing(&db, None).unwrap().local_model, None);
    }

    #[test]
    fn new_messages_measure_what_changed_once_and_only_after_a_first_analysis() {
        let (db, jobs) = with_messages(25);
        assert!(!measure_what_changed(&db, &jobs), "the first analysis is the user's to start");
        assert!(queued(&db).is_empty());

        mimic_core::voice::analyze(&db, &mut |_, _| {}).unwrap();
        assert!(!measure_what_changed(&db, &jobs), "nothing changed");

        db.mark_profiles_stale(None).unwrap();
        assert!(measure_what_changed(&db, &jobs));
        assert!(measure_what_changed(&db, &jobs), "one is waiting already");
        assert_eq!(queued(&db), vec![mimic_core::voice::CHANGED_JOB_KIND], "and only one");
    }
}
