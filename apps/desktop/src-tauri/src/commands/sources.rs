//! Source connectors: what Mimic can read, and importing from it.

use serde::Serialize;
use serde_json::json;
use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorInfo {
    pub connector: String,
    pub display_name: String,
    pub channel: String,
    pub description: String,
    pub location_kind: String,
    pub extensions: Vec<String>,
}

#[tauri::command]
pub async fn list_connectors() -> CommandResult<Vec<ConnectorInfo>> {
    Ok(mimic_core::sources::all()
        .into_iter()
        .map(|s| {
            let m = s.metadata();
            ConnectorInfo {
                connector: m.connector.into(),
                display_name: m.display_name.into(),
                channel: m.channel.into(),
                description: m.description.into(),
                location_kind: match m.location_kind {
                    mimic_core::sources::LocationKind::File => "file".into(),
                    mimic_core::sources::LocationKind::Folder => "folder".into(),
                },
                extensions: m.extensions.iter().map(|e| e.to_string()).collect(),
            }
        })
        .collect())
}

#[tauri::command]
pub async fn list_sources(state: State<'_, SharedState>) -> CommandResult<Vec<mimic_core::db::Source>> {
    Ok(state.db.list_sources()?)
}

/// Read a file without importing it, so the user can see what Mimic found
/// before committing.
#[tauri::command]
pub async fn validate_source_file(
    connector: String,
    location: String,
) -> CommandResult<mimic_core::sources::ValidationReport> {
    let source = mimic_core::sources::by_connector(&connector)?;
    Ok(source.validate(std::path::Path::new(&location))?)
}

#[tauri::command]
pub async fn create_source(
    state: State<'_, SharedState>,
    connector: String,
    name: String,
    channel: String,
    location: Option<String>,
) -> CommandResult<mimic_core::db::Source> {
    // Fail here rather than at import time, when the user has walked away.
    mimic_core::sources::by_connector(&connector)?;
    let source = state.db.create_source(&mimic_core::db::NewSource {
        connector,
        name,
        channel,
        location,
        config: serde_json::Value::Null,
    })?;
    state.db.set_source_status(&source.id, "ready", None)?;
    Ok(state.db.get_source(&source.id)?.unwrap_or(source))
}

#[tauri::command]
pub async fn start_source_import(
    state: State<'_, SharedState>,
    source_id: String,
) -> CommandResult<mimic_core::db::Job> {
    if state.db.user_identifier_set()?.is_empty() {
        return Err(CommandError::new(
            "no_identity",
            "Tell Mimic which addresses are yours first — otherwise every message imports as 'unknown' and none of it can teach it how you write.",
        ));
    }
    if state.db.get_source(&source_id)?.is_none() {
        return Err(CommandError::new("not_found", "That source no longer exists."));
    }
    Ok(state.jobs.enqueue(mimic_core::import::JOB_KIND, json!({ "sourceId": source_id }))?)
}

/// Remove a source and everything imported through it.
#[tauri::command]
pub async fn delete_source(
    state: State<'_, SharedState>,
    source_id: String,
) -> CommandResult<mimic_core::privacy::DeletionReport> {
    Ok(state.db.delete_source_and_contents(&source_id)?)
}

#[tauri::command]
pub async fn pick_source_file(
    app: tauri::AppHandle,
    title: Option<String>,
    extensions: Vec<String>,
) -> CommandResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let exts: Vec<&str> = extensions.iter().map(String::as_str).collect();
    app.dialog()
        .file()
        .set_title(title.unwrap_or_else(|| "Choose a file".into()))
        .add_filter("Supported exports", &exts)
        .pick_file(move |p| {
            let _ = tx.send(p.map(|f| f.to_string()));
        });
    Ok(rx.await.unwrap_or(None))
}
