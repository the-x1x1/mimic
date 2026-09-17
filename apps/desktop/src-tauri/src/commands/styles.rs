use serde::Serialize;
use serde_json::Value;
use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleSummary {
    #[serde(flatten)]
    pub style: mimic_core::db::StyleProfile,
    pub training_examples: i64,
    pub cameras: Vec<String>,
    pub active_version: Option<mimic_core::db::ModelVersion>,
    pub version_count: usize,
}

fn summarize(state: &SharedState, style: mimic_core::db::StyleProfile) -> CommandResult<StyleSummary> {
    let mut examples = 0i64;
    let mut cameras: Vec<String> = Vec::new();
    for lib in &style.library_ids {
        examples += state.db.count_assets_with_edits(lib)?;
        for (cam, _) in state.db.camera_distribution(lib)? {
            if !cameras.contains(&cam) {
                cameras.push(cam);
            }
        }
    }
    let versions = state.db.list_model_versions(&style.id)?;
    Ok(StyleSummary {
        training_examples: examples,
        cameras,
        active_version: versions.iter().find(|v| v.is_active).cloned(),
        version_count: versions.len(),
        style,
    })
}

#[tauri::command]
pub async fn list_styles(state: State<'_, SharedState>) -> CommandResult<Vec<StyleSummary>> {
    state.db.list_style_profiles()?.into_iter().map(|s| summarize(&state, s)).collect()
}

#[tauri::command]
pub async fn create_style(
    state: State<'_, SharedState>,
    name: String,
    description: Option<String>,
    library_id: Option<String>,
) -> CommandResult<StyleSummary> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CommandError::new("invalid", "Style name is required"));
    }
    let style = state.db.create_style_profile(name, description.as_deref())?;
    if let Some(lib) = library_id {
        state.db.link_style_library(&style.id, &lib)?;
    }
    let style =
        state.db.get_style_profile(&style.id)?.ok_or_else(|| CommandError::new("not_found", "style vanished"))?;
    summarize(&state, style)
}

#[tauri::command]
pub async fn delete_style(state: State<'_, SharedState>, style_id: String) -> CommandResult<()> {
    Ok(state.db.delete_style_profile(&style_id)?)
}

#[tauri::command]
pub async fn attach_library_to_style(
    state: State<'_, SharedState>,
    style_id: String,
    library_id: String,
) -> CommandResult<StyleSummary> {
    state.db.link_style_library(&style_id, &library_id)?;
    let style =
        state.db.get_style_profile(&style_id)?.ok_or_else(|| CommandError::new("not_found", "style not found"))?;
    summarize(&state, style)
}

#[tauri::command]
pub async fn get_style_detail(state: State<'_, SharedState>, style_id: String) -> CommandResult<Value> {
    let style =
        state.db.get_style_profile(&style_id)?.ok_or_else(|| CommandError::new("not_found", "style not found"))?;
    let summary = summarize(&state, style.clone())?;
    let mut reports = Vec::new();
    for lib in &style.library_ids {
        reports.push(mimic_core::ingest::data_quality_report(&state.db, lib)?);
    }
    let versions = state.db.list_model_versions(&style.id)?;
    Ok(serde_json::json!({
        "style": summary,
        "libraries": reports,
        "versions": versions,
        "training": {
            "available": false,
            "reason": "Training ships in Mimic 0.2.0. Ingest and data-quality reporting are complete; the trainer is not part of this build."
        }
    }))
}
