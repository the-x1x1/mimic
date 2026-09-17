//! Wire types for `/bridge/v1/*`. Golden fixtures live in `fixtures/bridge/`
//! and are asserted against these types in `tests/bridge_protocol.rs`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::capability::CapabilityProbe;

/// Every command type the desktop may enqueue for the plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandType {
    Ping,
    GetCatalogInfo,
    GetSelectedPhotos,
    GetPhotoMetadata,
    GetDevelopSettings,
    CreateBeforeSnapshot,
    ApplySettingsAsPluginPreset,
    ReadBackDevelopSettings,
    CollectCorrectionState,
    GetCapabilities,
}

impl CommandType {
    pub const ALL: &'static [CommandType] = &[
        CommandType::Ping,
        CommandType::GetCatalogInfo,
        CommandType::GetSelectedPhotos,
        CommandType::GetPhotoMetadata,
        CommandType::GetDevelopSettings,
        CommandType::CreateBeforeSnapshot,
        CommandType::ApplySettingsAsPluginPreset,
        CommandType::ReadBackDevelopSettings,
        CommandType::CollectCorrectionState,
        CommandType::GetCapabilities,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            CommandType::Ping => "ping",
            CommandType::GetCatalogInfo => "get_catalog_info",
            CommandType::GetSelectedPhotos => "get_selected_photos",
            CommandType::GetPhotoMetadata => "get_photo_metadata",
            CommandType::GetDevelopSettings => "get_develop_settings",
            CommandType::CreateBeforeSnapshot => "create_before_snapshot",
            CommandType::ApplySettingsAsPluginPreset => "apply_settings_as_plugin_preset",
            CommandType::ReadBackDevelopSettings => "read_back_develop_settings",
            CommandType::CollectCorrectionState => "collect_correction_state",
            CommandType::GetCapabilities => "get_capabilities",
        }
    }

    /// Commands that mutate the catalog. These are never retried automatically.
    pub fn is_mutating(&self) -> bool {
        matches!(self, CommandType::CreateBeforeSnapshot | CommandType::ApplySettingsAsPluginPreset)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeRequest {
    pub protocol_version: u32,
    pub plugin_version: String,
    pub lightroom_version: String,
    #[serde(default)]
    pub sdk_version: Option<String>,
    pub catalog_fingerprint: String,
    #[serde(default)]
    pub catalog_name: Option<String>,
    pub capabilities: CapabilityProbe,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeResponse {
    pub ok: bool,
    pub protocol_version: u32,
    pub app_version: String,
    pub session_id: String,
    pub poll_interval_ms: u64,
    pub max_batch_size: usize,
    pub accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandEnvelope {
    pub command_id: String,
    pub command_type: CommandType,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandResultBody {
    pub command_id: String,
    pub ok: bool,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<BridgeErrorBody>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeErrorBody {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginEvent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub at: Option<String>,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventsBody {
    pub events: Vec<PluginEvent>,
}

/// Per-item result of a batch apply, as the plugin reports it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyItemResult {
    pub photo_id: i64,
    pub prediction_id: String,
    /// `applied` | `failed` | `skipped`
    pub status: String,
    #[serde(default)]
    pub snapshot_name: Option<String>,
    #[serde(default)]
    pub before: Option<serde_json::Map<String, Value>>,
    #[serde(default)]
    pub read_back: Option<serde_json::Map<String, Value>>,
    #[serde(default)]
    pub error: Option<BridgeErrorBody>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyBatchResult {
    pub items: Vec<ApplyItemResult>,
    #[serde(default)]
    pub canceled: bool,
}

/// Discovery file the plugin reads (`bridge/bridge.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryFile {
    pub protocol_version: u32,
    pub app_version: String,
    pub base_url: String,
    pub token: String,
    pub pid: u32,
    pub written_at: String,
}
