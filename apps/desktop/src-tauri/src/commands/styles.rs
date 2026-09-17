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
    /// No-Touch Rate of the active version; `None` until a corrections sync has checked applied photos.
    pub no_touch_rate: Option<f64>,
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
    let active_version = versions.iter().find(|v| v.is_active).cloned();
    let no_touch_rate = match &active_version {
        Some(a) => {
            state.db.no_touch_stats(&style.id)?.into_iter().find(|n| n.model_version_id == a.id).and_then(|n| n.rate)
        }
        None => None,
    };
    Ok(StyleSummary {
        training_examples: examples,
        cameras,
        active_version,
        version_count: versions.len(),
        no_touch_rate,
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
        "training": training_availability(&state, &style, &reports),
        "health": mimic_core::corrections::style_health(&state.db, &style.id)?,
    }))
}

#[tauri::command]
pub async fn get_style_health(
    state: State<'_, SharedState>,
    style_id: String,
) -> CommandResult<mimic_core::corrections::StyleHealth> {
    Ok(mimic_core::corrections::style_health(&state.db, &style_id)?)
}

#[tauri::command]
pub async fn list_corrections(
    state: State<'_, SharedState>,
    style_id: String,
    limit: Option<usize>,
) -> CommandResult<Vec<mimic_core::db::CorrectionRow>> {
    Ok(state.db.corrections_for_style(&style_id, limit.unwrap_or(200).min(2000))?)
}

fn training_availability(
    state: &SharedState,
    style: &mimic_core::db::StyleProfile,
    reports: &[mimic_core::ingest::DataQualityReport],
) -> Value {
    let pairs: i64 = reports.iter().map(|r| r.valid_pairs).sum();
    let active_training = state
        .db
        .list_jobs(20, true)
        .map(|jobs| {
            jobs.into_iter()
                .any(|j| j.kind == mimic_core::training::JOB_TRAIN_STYLE && j.payload["styleId"] == style.id)
        })
        .unwrap_or(false);
    let (available, reason) = if style.library_ids.is_empty() {
        (false, "Add training data first.".to_string())
    } else if pairs < mimic_core::ingest::MIN_PAIRS_TO_TRAIN {
        (
            false,
            format!(
                "{pairs} edited examples ingested; at least {} are needed to train.",
                mimic_core::ingest::MIN_PAIRS_TO_TRAIN
            ),
        )
    } else if !state.engine.is_ready() {
        (false, "The analysis engine is not running.".to_string())
    } else if active_training {
        (false, "A training run for this Style is already in progress.".to_string())
    } else {
        (true, format!("{pairs} edited examples ready. Training creates a new immutable version and never changes the current one."))
    };
    serde_json::json!({"available": available, "reason": reason, "pairs": pairs, "minPairs": mimic_core::ingest::MIN_PAIRS_TO_TRAIN, "inProgress": active_training})
}

#[tauri::command]
pub async fn train_style(
    state: State<'_, SharedState>,
    style_id: String,
    config: Option<Value>,
) -> CommandResult<mimic_core::db::Job> {
    let style =
        state.db.get_style_profile(&style_id)?.ok_or_else(|| CommandError::new("not_found", "style not found"))?;
    let mut reports = Vec::new();
    for lib in &style.library_ids {
        reports.push(mimic_core::ingest::data_quality_report(&state.db, lib)?);
    }
    let avail = training_availability(&state, &style, &reports);
    if avail["available"] != true {
        return Err(CommandError::new(
            "training_unavailable",
            avail["reason"].as_str().unwrap_or("training unavailable"),
        ));
    }
    let payload = serde_json::json!({"styleId": style_id, "config": config.unwrap_or_else(|| serde_json::json!({}))});
    Ok(state.jobs.enqueue(mimic_core::training::JOB_TRAIN_STYLE, payload)?)
}

#[tauri::command]
pub async fn activate_model_version(
    state: State<'_, SharedState>,
    model_version_id: String,
) -> CommandResult<mimic_core::db::ModelVersion> {
    Ok(mimic_core::training::activate(&state.db, &model_version_id)?)
}

#[tauri::command]
pub async fn archive_model_version(
    state: State<'_, SharedState>,
    model_version_id: String,
) -> CommandResult<mimic_core::db::ModelVersion> {
    state.db.archive_model_version(&model_version_id)?;
    state
        .db
        .get_model_version(&model_version_id)?
        .ok_or_else(|| CommandError::new("not_found", "model version not found"))
}

#[tauri::command]
pub async fn get_model_version(
    state: State<'_, SharedState>,
    model_version_id: String,
) -> CommandResult<mimic_core::db::ModelVersion> {
    state
        .db
        .get_model_version(&model_version_id)?
        .ok_or_else(|| CommandError::new("not_found", "model version not found"))
}
