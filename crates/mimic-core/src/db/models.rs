//! Row types. Field names mirror columns; JSON columns are `serde_json::Value`
//! so the frontend receives structured data, not double-encoded strings.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub id: String,
    pub name: String,
    pub source_type: String,
    pub root_path: Option<String>,
    pub lightroom_catalog_fingerprint: Option<String>,
    pub created_at: String,
    pub last_scanned_at: Option<String>,
    pub status: String,
    /// `training` (shown in the Libraries UI) or `session` (backs a session).
    pub purpose: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAsset {
    pub library_id: Option<String>,
    pub source_path: String,
    pub file_name: String,
    pub extension: String,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub modified_time: Option<String>,
    pub fast_hash: String,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens: Option<String>,
    pub focal_length: Option<f64>,
    pub iso: Option<i64>,
    pub aperture: Option<f64>,
    pub shutter_speed: Option<f64>,
    pub captured_at: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub orientation: Option<i64>,
    pub lightroom_local_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    pub library_id: Option<String>,
    pub source_path: String,
    pub normalized_path: String,
    pub file_name: String,
    pub extension: String,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub modified_time: Option<String>,
    pub fast_hash: String,
    pub full_hash: Option<String>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens: Option<String>,
    pub focal_length: Option<f64>,
    pub iso: Option<i64>,
    pub aperture: Option<f64>,
    pub shutter_speed: Option<f64>,
    pub captured_at: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub orientation: Option<i64>,
    pub lightroom_local_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sidecar {
    pub id: String,
    pub asset_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub path: String,
    pub modified_time: Option<String>,
    pub hash: Option<String>,
    pub parse_status: String,
    pub parser_version: Option<String>,
    pub raw_metadata: Option<Value>,
    pub warnings: Option<Value>,
    pub detected_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSidecar {
    pub asset_id: String,
    pub kind: String,
    pub path: String,
    pub modified_time: Option<String>,
    pub hash: Option<String>,
    pub parse_status: String,
    pub parser_version: Option<String>,
    pub raw_metadata: Option<Value>,
    pub warnings: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditSnapshot {
    pub id: String,
    pub asset_id: String,
    pub source: String,
    pub process_version: Option<String>,
    pub normalized_settings: Value,
    pub raw_settings: Value,
    pub unknown_settings: Value,
    pub mapping_version: String,
    pub capability_schema_version: Option<String>,
    pub observed_at: String,
    pub provenance: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewEditSnapshot {
    pub asset_id: String,
    pub source: String,
    pub process_version: Option<String>,
    pub normalized_settings: Value,
    pub raw_settings: Value,
    pub unknown_settings: Value,
    pub mapping_version: String,
    pub capability_schema_version: Option<String>,
    pub provenance: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualFeatures {
    pub asset_id: String,
    pub feature_version: String,
    pub histogram: Value,
    pub luminance: Value,
    pub color: Value,
    pub sharpness: Option<f64>,
    pub noise_estimate: Option<f64>,
    pub clipping: Value,
    pub scene_labels: Value,
    pub embedding_artifact_id: Option<String>,
    pub preview_path: Option<String>,
    pub computed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub status: String,
    pub payload: Value,
    pub progress_current: i64,
    pub progress_total: i64,
    pub phase: Option<String>,
    pub resumable: bool,
    pub created_at: String,
    pub started_at: Option<String>,
    pub heartbeat_at: Option<String>,
    pub completed_at: Option<String>,
    pub result: Option<Value>,
    pub error: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventRow {
    pub id: i64,
    pub level: String,
    pub category: String,
    pub event_type: String,
    pub entity_type: Option<String>,
    pub entity_id: Option<String>,
    pub payload_json: String,
    pub created_at: String,
}

pub struct NewEvent<'a> {
    pub level: &'a str,
    pub category: &'a str,
    pub event_type: &'a str,
    pub entity_type: Option<&'a str>,
    pub entity_id: Option<&'a str>,
    pub payload: Value,
}

impl<'a> NewEvent<'a> {
    pub fn info(category: &'a str, event_type: &'a str, payload: Value) -> Self {
        Self { level: "info", category, event_type, entity_type: None, entity_id: None, payload }
    }
    pub fn warn(category: &'a str, event_type: &'a str, payload: Value) -> Self {
        Self { level: "warn", category, event_type, entity_type: None, entity_id: None, payload }
    }
    pub fn error(category: &'a str, event_type: &'a str, payload: Value) -> Self {
        Self { level: "error", category, event_type, entity_type: None, entity_id: None, payload }
    }
    pub fn entity(mut self, entity_type: &'a str, entity_id: &'a str) -> Self {
        self.entity_type = Some(entity_type);
        self.entity_id = Some(entity_id);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LightroomConnection {
    pub id: String,
    pub catalog_fingerprint: String,
    pub lightroom_version: Option<String>,
    pub sdk_version: Option<String>,
    pub plugin_version: Option<String>,
    pub capabilities: Value,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    pub current_version: String,
    pub latest_seen_version: Option<String>,
    pub staged_version: Option<String>,
    pub channel: String,
    pub last_checked_at: Option<String>,
    pub last_update_result: Option<String>,
    pub update_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleProfile {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub active_model_version_id: Option<String>,
    pub status: String,
    pub library_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainingSet {
    pub id: String,
    pub style_profile_id: String,
    pub source_query: Value,
    pub asset_count: i64,
    pub valid_pair_count: i64,
    pub train_count: i64,
    pub validation_count: i64,
    pub holdout_count: i64,
    pub split_strategy: String,
    pub fingerprint: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelVersion {
    pub id: String,
    pub style_profile_id: String,
    pub semantic_version: String,
    pub model_type: String,
    pub feature_schema_version: String,
    pub edit_schema_version: String,
    pub training_set_id: Option<String>,
    pub training_config: Value,
    pub metrics: Value,
    pub artifact_manifest: Value,
    pub created_at: String,
    pub status: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub name: String,
    pub source_path: Option<String>,
    pub source_library_id: Option<String>,
    pub captured_start: Option<String>,
    pub captured_end: Option<String>,
    pub status: String,
    pub active_style_profile_id: Option<String>,
    pub created_at: String,
    pub asset_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionAsset {
    pub session_id: String,
    pub asset_id: String,
    pub sequence_index: i64,
    pub cluster_id: Option<String>,
    pub burst_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneCluster {
    pub id: String,
    pub session_id: String,
    pub label: String,
    pub centroid_artifact_id: Option<String>,
    pub feature_summary: Value,
    pub created_at: String,
    pub asset_count: i64,
    /// Photographer-chosen reference photo for the consistency policy.
    pub reference_asset_id: Option<String>,
    /// Last manual rename/merge/split/move; `None` when untouched since grouping.
    pub edited_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prediction {
    pub id: String,
    pub session_id: String,
    pub asset_id: String,
    pub model_version_id: String,
    pub predicted_settings: Value,
    pub raw_model_output: Value,
    pub confidence: f64,
    pub confidence_components: Value,
    pub nearest_examples: Value,
    pub created_at: String,
    pub status: String,
    pub capability_schema_version: Option<String>,
    pub cluster_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewPrediction {
    pub session_id: String,
    pub asset_id: String,
    pub model_version_id: String,
    pub predicted_settings: Value,
    pub raw_model_output: Value,
    pub confidence: f64,
    pub confidence_components: Value,
    pub nearest_examples: Value,
    #[serde(default)]
    pub capability_schema_version: Option<String>,
    #[serde(default)]
    pub cluster_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyBatch {
    pub id: String,
    pub session_id: String,
    pub lightroom_catalog_fingerprint: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub status: String,
    pub applied_count: i64,
    pub failed_count: i64,
    pub rollback_available: bool,
    pub error: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedEdit {
    pub id: String,
    pub apply_batch_id: String,
    pub prediction_id: String,
    pub asset_id: String,
    pub before_settings: Option<Value>,
    pub applied_settings: Value,
    pub lightroom_snapshot_name: Option<String>,
    pub result: String,
    pub error: Option<Value>,
    pub applied_at: String,
    pub restored_at: Option<String>,
    pub restore_result: Option<String>,
    pub restore_error: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Correction {
    pub id: String,
    pub asset_id: String,
    pub prediction_id: String,
    pub model_version_id: String,
    pub predicted_settings: Value,
    pub corrected_settings: Value,
    pub delta: Value,
    pub correction_magnitude: f64,
    pub observed_at: String,
    pub included_in_training_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionSync {
    pub id: String,
    pub session_id: String,
    pub lightroom_catalog_fingerprint: Option<String>,
    pub synced_at: String,
    pub checked_count: i64,
    pub untouched_count: i64,
    pub corrected_count: i64,
    pub unresolved_count: i64,
}

/// A correction joined with what the UI needs to show it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionRow {
    #[serde(flatten)]
    pub correction: Correction,
    pub session_id: String,
    pub file_name: String,
    pub semantic_version: String,
}

/// No-Touch Rate for one model version: applied photos the photographer left
/// untouched after a corrections sync, over all applied photos in synced sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoTouchStats {
    pub model_version_id: String,
    pub semantic_version: String,
    pub applied_checked: i64,
    pub corrected: i64,
    pub untouched: i64,
    /// `None` until at least one applied photo has been checked by a sync.
    pub rate: Option<f64>,
}

pub(crate) fn json_col(s: Option<String>) -> Option<Value> {
    s.and_then(|s| serde_json::from_str(&s).ok())
}

pub(crate) fn json_col_or_default(s: String) -> Value {
    serde_json::from_str(&s).unwrap_or(Value::Null)
}
