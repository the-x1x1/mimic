use serde::Serialize;
use serde_json::{json, Value};
use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

#[tauri::command]
pub async fn list_libraries(state: State<'_, SharedState>) -> CommandResult<Vec<LibrarySummary>> {
    let libs = state.db.list_libraries()?;
    let mut out = Vec::with_capacity(libs.len());
    for l in libs {
        let assets = state.db.count_assets(Some(&l.id))?;
        let pairs = state.db.count_assets_with_edits(&l.id)?;
        out.push(LibrarySummary { library: l, asset_count: assets, valid_pair_count: pairs });
    }
    Ok(out)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySummary {
    #[serde(flatten)]
    pub library: mimic_core::db::Library,
    pub asset_count: i64,
    pub valid_pair_count: i64,
}

#[tauri::command]
pub async fn create_library(
    state: State<'_, SharedState>,
    name: String,
    source_type: String,
    root_path: Option<String>,
) -> CommandResult<mimic_core::db::Library> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CommandError::new("invalid", "Library name is required"));
    }
    if source_type == "folder_sidecars" {
        let Some(root) = root_path.as_deref().filter(|p| !p.trim().is_empty()) else {
            return Err(CommandError::new("invalid", "Choose a folder for a sidecar library"));
        };
        if !std::path::Path::new(root).is_dir() {
            return Err(CommandError::new("invalid", format!("{root} is not a folder Mimic can read")));
        }
    }
    let catalog = if source_type == "lightroom_catalog" {
        state.bridge.connection().map(|c| c.catalog_fingerprint)
    } else {
        None
    };
    Ok(state.db.create_library(name, &source_type, root_path.as_deref(), catalog.as_deref())?)
}

#[tauri::command]
pub async fn delete_library(state: State<'_, SharedState>, library_id: String) -> CommandResult<()> {
    Ok(state.db.delete_library(&library_id)?)
}

#[tauri::command]
pub async fn start_library_scan(
    state: State<'_, SharedState>,
    library_id: String,
) -> CommandResult<mimic_core::db::Job> {
    let lib = state.db.get_library(&library_id)?.ok_or_else(|| CommandError::new("not_found", "library not found"))?;
    if !state.engine.is_ready() {
        return Err(CommandError::new(
            "engine_not_running",
            "The analysis engine is not running. Check Settings › Diagnostics.",
        ));
    }
    let kind = match lib.source_type.as_str() {
        "folder_sidecars" | "demo" => mimic_core::ingest::JOB_SCAN_LIBRARY,
        "lightroom_catalog" => mimic_core::ingest::JOB_INGEST_LIGHTROOM,
        other => return Err(CommandError::new("invalid", format!("unknown library type {other}"))),
    };
    Ok(state.jobs.enqueue(kind, json!({"libraryId": library_id, "scope": "selection"}))?)
}

#[tauri::command]
pub async fn get_data_quality_report(
    state: State<'_, SharedState>,
    library_id: String,
) -> CommandResult<mimic_core::ingest::DataQualityReport> {
    Ok(mimic_core::ingest::data_quality_report(&state.db, &library_id)?)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetRow {
    #[serde(flatten)]
    pub asset: mimic_core::db::Asset,
    pub has_edits: bool,
    pub has_features: bool,
    pub preview_path: Option<String>,
    pub edit_source: Option<String>,
}

#[tauri::command]
pub async fn list_library_assets(
    state: State<'_, SharedState>,
    library_id: String,
    limit: Option<usize>,
    offset: Option<usize>,
) -> CommandResult<Vec<AssetRow>> {
    let assets = state.db.list_assets(Some(&library_id), limit.unwrap_or(200).min(1000), offset.unwrap_or(0))?;
    let mut out = Vec::with_capacity(assets.len());
    for a in assets {
        let snap = state.db.latest_observed_snapshot(&a.id)?;
        let feats = state.db.visual_features(&a.id, mimic_core::ingest::FEATURE_VERSION)?;
        out.push(AssetRow {
            has_edits: snap.is_some(),
            edit_source: snap.map(|s| s.source),
            has_features: feats.is_some(),
            preview_path: feats.and_then(|f| f.preview_path),
            asset: a,
        });
    }
    Ok(out)
}

#[tauri::command]
pub async fn get_asset_detail(state: State<'_, SharedState>, asset_id: String) -> CommandResult<Value> {
    let asset = state.db.get_asset(&asset_id)?.ok_or_else(|| CommandError::new("not_found", "asset not found"))?;
    let sidecars = state.db.sidecars_for_asset(&asset_id)?;
    let snapshots = state.db.snapshots_for_asset(&asset_id)?;
    let features = state.db.visual_features(&asset_id, mimic_core::ingest::FEATURE_VERSION)?;
    let edit_dna = snapshots.last().map(|s| mimic_core::edit_dna::build_edit_dna(&asset, features.as_ref(), s));
    Ok(json!({
        "asset": asset,
        "sidecars": sidecars,
        "snapshots": snapshots,
        "features": features,
        "editDna": edit_dna,
    }))
}
