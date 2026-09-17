use tauri::State;

use crate::error::CommandResult;
use crate::SharedState;

#[tauri::command]
pub async fn get_diagnostics_bundle(
    state: State<'_, SharedState>,
) -> CommandResult<mimic_core::diagnostics::DiagnosticsBundle> {
    let include_paths = state.db.get_setting::<bool>("diagnostics.includePaths")?.unwrap_or(false);
    Ok(mimic_core::diagnostics::build_bundle(
        &state.db,
        serde_json::to_value(state.engine.status())?,
        serde_json::to_value(state.bridge.status())?,
        include_paths,
    )?)
}

#[tauri::command]
pub async fn get_recent_events(
    state: State<'_, SharedState>,
    limit: Option<usize>,
    min_level: Option<String>,
) -> CommandResult<Vec<mimic_core::db::EventRow>> {
    Ok(state.db.recent_events(limit.unwrap_or(100).min(1000), min_level.as_deref())?)
}

#[tauri::command]
pub async fn restart_engine(state: State<'_, SharedState>) -> CommandResult<mimic_core::engine::EngineStatus> {
    state.engine.reset_restart_budget();
    let status = state.engine.restart().await?;
    let cfg = serde_json::json!({
        "dbPath": state.db.path().map(|p| p.to_string_lossy().to_string()),
        "previewsDir": state.paths.previews_cache(),
        "embeddingsDir": state.paths.embeddings_cache(),
        "encodersDir": state.paths.encoders_dir(),
        "stylesDir": state.paths.styles_dir(),
        "manifestsDir": state.manifests_dir(),
    });
    state.engine.call("engine.configure", cfg).await?;
    Ok(status)
}

#[tauri::command]
pub async fn open_logs_folder(app: tauri::AppHandle, state: State<'_, SharedState>) -> CommandResult<String> {
    use tauri_plugin_opener::OpenerExt;
    let dir = state.log_dir.clone();
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| crate::error::CommandError::new("open_failed", e.to_string()))?;
    Ok(dir.to_string_lossy().to_string())
}
