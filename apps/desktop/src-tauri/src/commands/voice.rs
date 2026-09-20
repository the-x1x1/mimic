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
