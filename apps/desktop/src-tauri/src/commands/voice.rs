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
