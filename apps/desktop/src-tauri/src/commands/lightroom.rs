use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LightroomStatus {
    pub bridge: mimic_core::bridge::BridgeStatus,
    pub last_known: Option<mimic_core::db::LightroomConnection>,
    pub plugin_installed_path: String,
    pub plugin_installed: bool,
    pub plugin_source_available: bool,
}

#[tauri::command]
pub async fn get_lightroom_status(state: State<'_, SharedState>) -> CommandResult<LightroomStatus> {
    let install = state.paths.plugin_install_dir();
    Ok(LightroomStatus {
        bridge: state.bridge.status(),
        last_known: state.db.latest_lightroom_connection()?,
        plugin_installed: install.join("Info.lua").is_file(),
        plugin_installed_path: install.to_string_lossy().to_string(),
        plugin_source_available: state.plugin_source_dir().is_some(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSetup {
    pub plugin_path: String,
    pub bridge_file: String,
    pub steps: Vec<String>,
    pub plugin_version: String,
}

#[tauri::command]
pub async fn get_plugin_setup(state: State<'_, SharedState>) -> CommandResult<PluginSetup> {
    Ok(PluginSetup {
        plugin_path: state.paths.plugin_install_dir().to_string_lossy().to_string(),
        bridge_file: state.paths.bridge_discovery_file().to_string_lossy().to_string(),
        plugin_version: mimic_core::APP_VERSION.to_string(),
        steps: vec![
            "In Lightroom Classic open File › Plug-in Manager.".into(),
            "Click Add and choose the Mimic.lrplugin folder at the path below.".into(),
            "Keep Mimic running; the plugin connects on its own within a few seconds.".into(),
            "Select a photo in Lightroom and press Test Connection here to complete the capability probe.".into(),
        ],
    })
}

#[tauri::command]
pub async fn install_lightroom_plugin(state: State<'_, SharedState>) -> CommandResult<PluginSetup> {
    let src = state
        .plugin_source_dir()
        .ok_or_else(|| CommandError::new("plugin_missing", "The plugin files are not available in this build."))?;
    crate::plugin_install::sync_plugin(&src, &state.paths.plugin_install_dir())?;
    let _ = state.db.log_event(&mimic_core::db::NewEvent::info(
        "lightroom",
        "plugin_installed",
        json!({"path": state.paths.plugin_install_dir()}),
    ));
    get_plugin_setup(state).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTest {
    pub connected: bool,
    pub round_trip_ms: Option<u128>,
    pub message: String,
    pub connection: Option<mimic_core::bridge::ConnectionInfo>,
}

#[tauri::command]
pub async fn test_lightroom_connection(state: State<'_, SharedState>) -> CommandResult<ConnectionTest> {
    if state.bridge.connection().is_none() {
        return Ok(ConnectionTest {
            connected: false,
            round_trip_ms: None,
            message: "Open Lightroom Classic and make sure the Mimic plugin is enabled.".into(),
            connection: None,
        });
    }
    let start = std::time::Instant::now();
    match state
        .bridge
        .send_command(mimic_core::bridge::CommandType::Ping, json!({"echo": "mimic"}), Duration::from_secs(10))
        .await
    {
        Ok(_) => {
            // Refresh capabilities with whatever is selected now.
            if let Ok(probe) = state
                .bridge
                .send_command(mimic_core::bridge::CommandType::GetCapabilities, json!({}), Duration::from_secs(20))
                .await
            {
                if let Ok(probe) = serde_json::from_value::<mimic_core::capability::CapabilityProbe>(probe) {
                    let _ = state.bridge.update_capabilities_from_probe(probe);
                }
            }
            let conn = state.bridge.connection();
            if let Some(c) = &conn {
                let _ = state.db.record_lightroom_connection(
                    &c.catalog_fingerprint,
                    Some(&c.lightroom_version),
                    c.sdk_version.as_deref(),
                    Some(&c.plugin_version),
                    &serde_json::to_value(&c.capabilities).unwrap_or_default(),
                    "connected",
                );
            }
            let probe_note = match &conn {
                Some(c) if !c.capabilities.probe_had_photo => {
                    " Select a photo in Lightroom and test again to complete the capability probe."
                }
                _ => "",
            };
            Ok(ConnectionTest {
                connected: true,
                round_trip_ms: Some(start.elapsed().as_millis()),
                message: format!("Connected to Lightroom Classic.{probe_note}"),
                connection: conn,
            })
        }
        Err(e) => {
            Ok(ConnectionTest { connected: false, round_trip_ms: None, message: e.to_string(), connection: None })
        }
    }
}

#[tauri::command]
pub async fn start_lightroom_ingest(
    state: State<'_, SharedState>,
    library_id: Option<String>,
    name: Option<String>,
    scope: Option<String>,
) -> CommandResult<mimic_core::db::Job> {
    let conn = state
        .bridge
        .connection()
        .ok_or_else(|| CommandError::new("lightroom_not_connected", "Lightroom is not connected."))?;
    if !state.engine.is_ready() {
        return Err(CommandError::new("engine_not_running", "The analysis engine is not running."));
    }
    let library = match library_id {
        Some(id) => state.db.get_library(&id)?.ok_or_else(|| CommandError::new("not_found", "library not found"))?,
        None => {
            let name = name.unwrap_or_else(|| {
                format!("Lightroom — {}", conn.catalog_name.clone().unwrap_or_else(|| "catalog".into()))
            });
            state.db.create_library(&name, "lightroom_catalog", None, Some(&conn.catalog_fingerprint))?
        }
    };
    let scope = scope.unwrap_or_else(|| "selection".into());
    if !matches!(scope.as_str(), "selection" | "collection" | "folder" | "catalog") {
        return Err(CommandError::new("invalid", "scope must be selection, collection, folder or catalog"));
    }
    Ok(state
        .jobs
        .enqueue(mimic_core::ingest::JOB_INGEST_LIGHTROOM, json!({"libraryId": library.id, "scope": scope}))?)
}

#[tauri::command]
pub async fn get_capability_matrix(state: State<'_, SharedState>) -> CommandResult<Value> {
    if let Some(c) = state.bridge.connection() {
        return Ok(json!({"live": true, "matrix": c.capabilities}));
    }
    if let Some(last) = state.db.latest_lightroom_connection()? {
        return Ok(json!({"live": false, "matrix": last.capabilities, "lastSeenAt": last.last_seen_at}));
    }
    Ok(json!({"live": false, "matrix": Value::Null}))
}
