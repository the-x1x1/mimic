//! Sessions: ingest a shoot, group it into scenes, predict with the active
//! Style Brain, apply through the Lightroom bridge with read-back
//! verification, and restore from the recorded before-state (spec §22–§25).
//!
//! Every mutation of a catalog goes through `apply_settings_as_plugin_preset`
//! with a before-snapshot; an item is `applied` only when the plugin's
//! read-back matches what was sent (`edit_dna::verify_readback`). Nothing in
//! this module touches `.lrcat`, source media or XMP.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::bridge::{ApplyBatchResult, BridgeError, BridgeHandle, CommandType, ConnectionInfo};
use crate::db::normalize_path;
use crate::db::{
    AppliedEdit, ApplyBatch, Asset, Db, DbError, NewAppliedEdit, NewEditSnapshot, NewEvent, NewPrediction, Prediction,
    SceneCluster, Session,
};
use crate::edit_dna::{self, ControlValue, MAPPING};
use crate::engine::EngineClient;
use crate::ids::{new_id, now_rfc3339};
use crate::ingest::{self, FEATURE_VERSION};
use crate::jobs::{forward_engine_progress, JobContext, JobError, JobExecutor, JobFuture};

pub const JOB_INGEST_SESSION: &str = "ingest_session";
pub const JOB_GROUP_SESSION: &str = "group_session";
pub const JOB_PREDICT_SESSION: &str = "predict_session";
pub const JOB_APPLY_SESSION: &str = "apply_session";
pub const JOB_RESTORE_BATCH: &str = "restore_batch";

/// Photos per `apply_settings_as_plugin_preset` command (plugin refuses more).
pub const APPLY_BATCH_SIZE: usize = 25;
const APPLY_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const LIST_TIMEOUT: Duration = Duration::from_secs(600);

pub struct SessionExecutor {
    pub engine: EngineClient,
    pub bridge: BridgeHandle,
}

impl SessionExecutor {
    pub const ALL_KINDS: &'static [&'static str] =
        &[JOB_INGEST_SESSION, JOB_GROUP_SESSION, JOB_PREDICT_SESSION, JOB_APPLY_SESSION, JOB_RESTORE_BATCH];
}

impl JobExecutor for SessionExecutor {
    fn kinds(&self) -> &'static [&'static str] {
        Self::ALL_KINDS
    }
    fn resumable(&self, kind: &str) -> bool {
        // Ingest/group/predict are idempotent upserts. Apply and restore mutate
        // a catalog and are never re-run blindly after an interruption.
        matches!(kind, JOB_INGEST_SESSION | JOB_GROUP_SESSION | JOB_PREDICT_SESSION)
    }
    fn execute(&self, ctx: JobContext) -> JobFuture {
        let engine = self.engine.clone();
        let bridge = self.bridge.clone();
        Box::pin(async move {
            match ctx.job.kind.as_str() {
                JOB_INGEST_SESSION => ingest_session(&ctx, &engine, &bridge).await,
                JOB_GROUP_SESSION => group_session(&ctx, &engine).await,
                JOB_PREDICT_SESSION => predict_session(&ctx, &engine, &bridge).await,
                JOB_APPLY_SESSION => apply_session(&ctx, &bridge).await,
                JOB_RESTORE_BATCH => restore_batch(&ctx, &bridge).await,
                other => Err(JobError::Failed(format!("unknown job kind {other}"))),
            }
        })
    }
}

pub fn executor(engine: EngineClient, bridge: BridgeHandle) -> Arc<dyn JobExecutor> {
    Arc::new(SessionExecutor { engine, bridge })
}

fn payload_str(payload: &Value, key: &str) -> Result<String, JobError> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| JobError::Failed(format!("job payload missing {key}")))
}

fn load_session(db: &Db, id: &str) -> Result<Session, JobError> {
    db.get_session(id)?.ok_or_else(|| JobError::Failed(format!("session {id} not found")))
}

// ---------------------------------------------------------------------------
// Creation + ingest
// ---------------------------------------------------------------------------

/// Where a session's photos come from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SessionSource {
    /// A folder on disk (RAW/JPEG + optional sidecars for the before-state).
    Folder { path: String },
    /// The photos currently selected (or the active folder/collection) in the
    /// connected Lightroom catalog.
    Lightroom { scope: String },
}

/// Create the session row and its backing (hidden) library. Returns the session;
/// the caller then enqueues `ingest_session`.
pub fn create_session(
    db: &Db,
    name: &str,
    source: &SessionSource,
    style_id: Option<&str>,
    lightroom: Option<&ConnectionInfo>,
) -> Result<Session, JobError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(JobError::Failed("session name is required".into()));
    }
    let library = match source {
        SessionSource::Folder { path } => {
            if !std::path::Path::new(path).is_dir() {
                return Err(JobError::Failed(format!("folder does not exist: {path}")));
            }
            db.create_library_with_purpose(&format!("Session: {name}"), "folder_sidecars", Some(path), None, "session")?
        }
        SessionSource::Lightroom { scope } => {
            let conn = lightroom.ok_or(BridgeError::NotConnected)?;
            if !matches!(scope.as_str(), "selection" | "folder" | "collection" | "catalog") {
                return Err(JobError::Failed(format!("unknown Lightroom scope {scope}")));
            }
            db.create_library_with_purpose(
                &format!("Session: {name}"),
                "lightroom_catalog",
                None,
                Some(&conn.catalog_fingerprint),
                "session",
            )?
        }
    };
    let source_path = match source {
        SessionSource::Folder { path } => Some(path.as_str()),
        SessionSource::Lightroom { .. } => None,
    };
    let session = db.create_session(name, source_path, Some(&library.id), style_id)?;
    db.set_setting(&format!("session.{}.source", session.id), source)?;
    let _ =
        db.log_event(&NewEvent::info("sessions", "created", json!({"source": source})).entity("session", &session.id));
    Ok(session)
}

pub fn session_source(db: &Db, session_id: &str) -> Result<Option<SessionSource>, DbError> {
    db.get_setting(&format!("session.{session_id}.source"))
}

async fn ingest_session(ctx: &JobContext, engine: &EngineClient, bridge: &BridgeHandle) -> Result<Value, JobError> {
    let session_id = payload_str(&ctx.job.payload, "sessionId")?;
    let session = load_session(&ctx.db, &session_id)?;
    let library_id =
        session.source_library_id.clone().ok_or_else(|| JobError::Failed("session has no backing library".into()))?;
    let source =
        session_source(&ctx.db, &session_id)?.ok_or_else(|| JobError::Failed("session source missing".into()))?;
    ctx.db.set_session_status(&session_id, "ingesting")?;
    let ingest_summary = match &source {
        SessionSource::Folder { .. } => ingest::scan_library_by_id(ctx, engine, &library_id).await,
        SessionSource::Lightroom { scope } => {
            ingest::ingest_lightroom_by_id(ctx, engine, bridge, &library_id, scope).await
        }
    };
    let ingest_summary = match ingest_summary {
        Ok(v) => v,
        Err(e) => {
            ctx.db.set_session_status(&session_id, "ingest_failed")?;
            return Err(e);
        }
    };
    // Capture-ordered membership.
    let assets = ctx.db.list_assets(Some(&library_id), 1_000_000, 0)?;
    for (i, a) in assets.iter().enumerate() {
        ctx.db.add_session_asset(&session_id, &a.id, i as i64)?;
    }
    let times: Vec<&str> = assets.iter().filter_map(|a| a.captured_at.as_deref()).collect();
    ctx.db.set_session_capture_range(&session_id, times.iter().min().copied(), times.iter().max().copied())?;
    ctx.db.set_session_status(&session_id, if assets.is_empty() { "empty" } else { "ingested" })?;
    let summary = json!({
        "sessionId": session_id,
        "photos": assets.len(),
        "withFeatures": assets.len() - ctx.db.assets_missing_features(&library_id, FEATURE_VERSION)?.len(),
        "ingest": ingest_summary,
    });
    let _ = ctx.db.log_event(&NewEvent::info("sessions", "ingested", summary.clone()).entity("session", &session_id));
    Ok(summary)
}

// ---------------------------------------------------------------------------
// Scene grouping
// ---------------------------------------------------------------------------

async fn group_session(ctx: &JobContext, engine: &EngineClient) -> Result<Value, JobError> {
    let session_id = payload_str(&ctx.job.payload, "sessionId")?;
    let session = load_session(&ctx.db, &session_id)?;
    let members = ctx.db.session_assets(&session_id)?;
    if members.is_empty() {
        return Err(JobError::Failed("session has no photos to group".into()));
    }
    let asset_ids: Vec<&str> = members.iter().map(|m| m.asset_id.as_str()).collect();
    ctx.progress(0, members.len() as i64, "grouping");
    let _forwarder = forward_engine_progress(engine, ctx);
    let config = ctx.job.payload.get("config").cloned().unwrap_or_else(|| json!({}));
    let out = engine
        .call_with_timeout(
            "session.group",
            json!({"assetIds": asset_ids, "config": config, "jobId": ctx.job.id}),
            Duration::from_secs(1800),
        )
        .await?;
    // Engine cluster ids are per call; persist them under globally unique ids.
    let mut id_map: HashMap<String, String> = HashMap::new();
    let mut rows: Vec<(String, String, Value)> = Vec::new();
    for c in out.get("clusters").and_then(Value::as_array).cloned().unwrap_or_default() {
        let engine_id = c["id"].as_str().unwrap_or_default().to_string();
        let db_id = new_id();
        id_map.insert(engine_id.clone(), db_id.clone());
        let mut summary = c.get("summary").cloned().unwrap_or_else(|| json!({}));
        summary["timeBlock"] = c["timeBlock"].clone();
        summary["count"] = c["count"].clone();
        summary["engineClusterId"] = json!(engine_id);
        summary["groupingVersion"] = out["version"].clone();
        rows.push((db_id, c["label"].as_str().unwrap_or("Group").to_string(), summary));
    }
    let clusters = ctx.db.replace_scene_clusters(&session_id, &rows)?;
    let assignments = out.get("assignments").and_then(Value::as_object).cloned().unwrap_or_default();
    let bursts = out.get("bursts").and_then(Value::as_object).cloned().unwrap_or_default();
    for m in &members {
        let cluster = assignments.get(&m.asset_id).and_then(Value::as_str).and_then(|c| id_map.get(c)).cloned();
        let burst = bursts.get(&m.asset_id).and_then(Value::as_str).map(str::to_string);
        ctx.db.assign_cluster(&session_id, &m.asset_id, cluster.as_deref(), burst.as_deref())?;
    }
    if matches!(session.status.as_str(), "ingested" | "grouped" | "empty") {
        ctx.db.set_session_status(&session_id, "grouped")?;
    }
    let missing = out.get("missingFeatures").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0);
    let summary = json!({
        "sessionId": session_id,
        "clusters": clusters.len(),
        "timeBlocks": out["timeBlocks"],
        "bursts": bursts.values().collect::<HashSet<_>>().len(),
        "photosWithoutFeatures": missing,
        "groupingVersion": out["version"],
    });
    let _ = ctx.db.log_event(&NewEvent::info("sessions", "grouped", summary.clone()).entity("session", &session_id));
    Ok(summary)
}

// ---------------------------------------------------------------------------
// Prediction
// ---------------------------------------------------------------------------

/// Canonical `global` map from the engine's prediction output.
fn global_from_engine(global: &Value) -> BTreeMap<String, BTreeMap<String, ControlValue>> {
    let mut out: BTreeMap<String, BTreeMap<String, ControlValue>> = BTreeMap::new();
    let Some(families) = global.as_object() else { return out };
    for (family, controls) in families {
        let Some(controls) = controls.as_object() else { continue };
        for (short, cv) in controls {
            let canonical = format!("{family}.{short}");
            let Some(control) = MAPPING.control(&canonical) else { continue };
            let raw = cv.get("raw").cloned().unwrap_or(Value::Null);
            if raw.is_null() {
                continue;
            }
            out.entry(family.clone()).or_default().insert(
                short.clone(),
                ControlValue {
                    raw,
                    value: cv.get("value").and_then(Value::as_f64),
                    source_key: control.lightroom_keys.first().cloned().unwrap_or_default(),
                },
            );
        }
    }
    out
}

fn model_path(mv: &crate::db::ModelVersion) -> Result<String, JobError> {
    mv.artifact_manifest
        .get("artifacts")
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find(|x| x["kind"] == "model"))
        .and_then(|a| a["path"].as_str())
        .map(str::to_string)
        .filter(|p| std::path::Path::new(p).is_file())
        .ok_or_else(|| JobError::Failed(format!("model artifact for v{} is missing on disk", mv.semantic_version)))
}

async fn predict_session(ctx: &JobContext, engine: &EngineClient, bridge: &BridgeHandle) -> Result<Value, JobError> {
    let session_id = payload_str(&ctx.job.payload, "sessionId")?;
    let session = load_session(&ctx.db, &session_id)?;
    let style_id = ctx
        .job
        .payload
        .get("styleId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or(session.active_style_profile_id.clone())
        .ok_or_else(|| JobError::Failed("choose a Style for this session first".into()))?;
    if session.active_style_profile_id.as_deref() != Some(style_id.as_str()) {
        ctx.db.set_session_style(&session_id, Some(&style_id))?;
    }
    let mv = ctx
        .db
        .active_model_version(&style_id)?
        .ok_or_else(|| JobError::Failed("this Style has no active trained version; train it first".into()))?;
    if mv.feature_schema_version != FEATURE_VERSION {
        return Err(JobError::Failed(format!(
            "active version was trained on {} but this build computes {FEATURE_VERSION}; retrain",
            mv.feature_schema_version
        )));
    }
    let path = model_path(&mv)?;
    let members = ctx.db.session_assets(&session_id)?;
    if members.is_empty() {
        return Err(JobError::Failed("session has no photos".into()));
    }
    let groups: Map<String, Value> =
        members.iter().filter_map(|m| m.cluster_id.clone().map(|c| (m.asset_id.clone(), json!(c)))).collect();
    let consistency = ctx.job.payload.get("consistency").and_then(Value::as_bool).unwrap_or(true);
    let capability_version = bridge.connection().map(|c| c.capabilities.schema_version);
    ctx.progress(0, members.len() as i64, "predicting");
    let _forwarder = forward_engine_progress(engine, ctx);
    let params = json!({
        "modelPath": path,
        "assetIds": members.iter().map(|m| m.asset_id.as_str()).collect::<Vec<_>>(),
        "groups": if groups.is_empty() { Value::Null } else { Value::Object(groups) },
        "consistency": consistency,
        "jobId": ctx.job.id,
    });
    let out = engine.call_with_timeout("model.predict", params, Duration::from_secs(3600)).await?;
    let cluster_by_asset: HashMap<&str, Option<String>> =
        members.iter().map(|m| (m.asset_id.as_str(), m.cluster_id.clone())).collect();
    let mut predicted = 0usize;
    let mut failed: Vec<Value> = Vec::new();
    let mut low_confidence = 0usize;
    let mut ood = 0usize;
    for r in out.get("results").and_then(Value::as_array).cloned().unwrap_or_default() {
        ctx.check_cancel()?;
        let Some(asset_id) = r.get("assetId").and_then(Value::as_str).map(str::to_string) else { continue };
        if let Some(err) = r.get("error") {
            failed.push(json!({"assetId": asset_id, "error": err}));
            continue;
        }
        let global = global_from_engine(&r["global"]);
        let confidence = r["confidence"].as_f64().unwrap_or(0.0);
        if confidence < 0.5 {
            low_confidence += 1;
        }
        if r["ood"].as_bool().unwrap_or(false) {
            ood += 1;
        }
        let mut raw_output = r.clone();
        if let Some(o) = raw_output.as_object_mut() {
            o.remove("global");
            o.remove("nearestExamples");
            o.remove("confidenceComponents");
        }
        ctx.db.insert_prediction(&NewPrediction {
            session_id: session_id.clone(),
            asset_id: asset_id.clone(),
            model_version_id: mv.id.clone(),
            predicted_settings: json!({
                "schemaVersion": edit_dna::SCHEMA_VERSION.to_string(),
                "mappingVersion": edit_dna::MAPPING_VERSION.to_string(),
                "global": global,
            }),
            raw_model_output: raw_output,
            confidence,
            confidence_components: r.get("confidenceComponents").cloned().unwrap_or_else(|| json!({})),
            nearest_examples: r.get("nearestExamples").cloned().unwrap_or_else(|| json!([])),
            capability_schema_version: capability_version.clone(),
            cluster_id: cluster_by_asset.get(asset_id.as_str()).cloned().flatten(),
        })?;
        predicted += 1;
    }
    ctx.db.set_session_status(&session_id, "predicted")?;
    let summary = json!({
        "sessionId": session_id,
        "styleId": style_id,
        "modelVersionId": mv.id,
        "semanticVersion": mv.semantic_version,
        "predicted": predicted,
        "failed": failed.len(),
        "failures": failed,
        "lowConfidence": low_confidence,
        "outOfDistribution": ood,
        "consistency": consistency && !cluster_by_asset.values().all(Option::is_none),
        "capabilitySchemaVersion": capability_version,
    });
    let _ = ctx.db.log_event(&NewEvent::info("sessions", "predicted", summary.clone()).entity("session", &session_id));
    Ok(summary)
}

// ---------------------------------------------------------------------------
// Apply
// ---------------------------------------------------------------------------

/// Everything that must hold before Mimic writes to a catalog. Returned to the
/// UI by `apply_preflight` so the confirm dialog can explain refusals.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyPreflight {
    pub ok: bool,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
    pub lightroom_connected: bool,
    pub catalog_fingerprint: Option<String>,
    pub capability_schema_version: Option<String>,
    pub candidate_count: usize,
    pub stale_count: usize,
    pub unresolved_count: usize,
    pub writable_controls: usize,
    pub batch_size: usize,
}

fn candidate_predictions(
    db: &Db,
    session_id: &str,
    only: Option<&HashSet<String>>,
) -> Result<Vec<Prediction>, DbError> {
    Ok(db
        .session_predictions(session_id, false)?
        .into_iter()
        .filter(|p| matches!(p.status.as_str(), "pending" | "reviewed"))
        .filter(|p| only.is_none_or(|set| set.contains(&p.id)))
        .collect())
}

/// Pure precheck shared by the UI preflight and the job.
pub fn apply_preflight(
    db: &Db,
    session_id: &str,
    only: Option<&HashSet<String>>,
    conn: Option<&ConnectionInfo>,
) -> Result<ApplyPreflight, DbError> {
    let mut pf = ApplyPreflight {
        ok: false,
        blockers: Vec::new(),
        warnings: Vec::new(),
        lightroom_connected: conn.is_some(),
        catalog_fingerprint: conn.map(|c| c.catalog_fingerprint.clone()),
        capability_schema_version: conn.map(|c| c.capabilities.schema_version.clone()),
        candidate_count: 0,
        stale_count: 0,
        unresolved_count: 0,
        writable_controls: 0,
        batch_size: APPLY_BATCH_SIZE,
    };
    let session = db.get_session(session_id)?;
    let Some(session) = session else {
        pf.blockers.push("session not found".into());
        return Ok(pf);
    };
    let candidates = candidate_predictions(db, session_id, only)?;
    pf.candidate_count = candidates.len();
    if candidates.is_empty() {
        pf.blockers
            .push("no pending predictions to apply (run Predict, or everything is already applied/rejected)".into());
    }
    let Some(conn) = conn else {
        pf.blockers.push("Lightroom Classic is not connected; open Lightroom with the Mimic plugin enabled".into());
        return Ok(pf);
    };
    let caps = &conn.capabilities;
    if !caps.can_apply {
        pf.blockers.push("this Lightroom build does not expose the plugin-preset apply API".into());
    }
    if !caps.can_snapshot {
        pf.blockers.push(
            "before-snapshots are unavailable in this Lightroom; Mimic never applies without a recovery path".into(),
        );
    }
    pf.writable_controls = caps.writable_keys().len();
    if pf.writable_controls == 0 {
        pf.blockers.push("the capability probe found no writable develop controls".into());
    }
    if let Some(lib_id) = &session.source_library_id {
        if let Some(lib) = db.get_library(lib_id)? {
            if let Some(fp) = lib.lightroom_catalog_fingerprint {
                if fp != conn.catalog_fingerprint {
                    pf.blockers.push(format!(
                        "session was created from catalog {fp} but Lightroom has {} open",
                        conn.catalog_fingerprint
                    ));
                }
            }
        }
    }
    pf.stale_count = candidates
        .iter()
        .filter(|p| p.capability_schema_version.as_deref().is_some_and(|v| v != caps.schema_version))
        .count();
    if pf.stale_count > 0 {
        pf.blockers.push(format!(
            "{} prediction(s) were made against a different Lightroom capability set; run Predict again",
            pf.stale_count
        ));
    }
    let active: HashSet<String> = candidates
        .iter()
        .filter_map(|p| db.get_model_version(&p.model_version_id).ok().flatten())
        .filter(|mv| mv.is_active)
        .map(|mv| mv.id.clone())
        .collect();
    let inactive = candidates.iter().filter(|p| !active.contains(&p.model_version_id)).count();
    if inactive > 0 {
        pf.warnings.push(format!("{inactive} prediction(s) come from a model version that is no longer active"));
    }
    // A running apply is detected from the jobs table by the command layer
    // (a crashed job must never lock a session forever); `applying` is
    // informational only.
    pf.ok = pf.blockers.is_empty();
    Ok(pf)
}

/// Map session assets to Lightroom photo ids: direct when the asset came from
/// this catalog, otherwise by normalized path against the catalog listing.
pub(crate) async fn resolve_photo_ids(
    bridge: &BridgeHandle,
    conn: &ConnectionInfo,
    db: &Db,
    assets: &[Asset],
) -> Result<HashMap<String, i64>, JobError> {
    let mut out = HashMap::new();
    let mut unresolved: Vec<&Asset> = Vec::new();
    for a in assets {
        let same_catalog = a
            .library_id
            .as_deref()
            .and_then(|l| db.get_library(l).ok().flatten())
            .and_then(|l| l.lightroom_catalog_fingerprint)
            .is_some_and(|fp| fp == conn.catalog_fingerprint);
        match a.lightroom_local_id {
            Some(id) if same_catalog => {
                out.insert(a.id.clone(), id);
            }
            _ => unresolved.push(a),
        }
    }
    if unresolved.is_empty() {
        return Ok(out);
    }
    let listing = bridge
        .send_command(CommandType::GetSelectedPhotos, json!({"scope": "catalog", "maxPhotos": 200_000}), LIST_TIMEOUT)
        .await?;
    let by_path: HashMap<String, i64> = listing
        .get("photos")
        .and_then(Value::as_array)
        .map(|photos| {
            photos
                .iter()
                .filter_map(|p| Some((normalize_path(p.get("path")?.as_str()?), p.get("photoId")?.as_i64()?)))
                .collect()
        })
        .unwrap_or_default();
    for a in unresolved {
        if let Some(id) = by_path.get(&a.normalized_path) {
            out.insert(a.id.clone(), *id);
        }
    }
    Ok(out)
}

fn verify_item(intended: &Map<String, Value>, item: &crate::bridge::ApplyItemResult) -> (String, Option<Value>) {
    match item.status.as_str() {
        "applied" => match &item.read_back {
            Some(observed) => {
                let v = edit_dna::verify_readback(intended, observed);
                if v.ok {
                    ("applied".into(), None)
                } else {
                    (
                        "verify_failed".into(),
                        Some(
                            json!({"code": "readback_mismatch", "message": format!("{} of {} controls did not read back as written", v.mismatches.len(), v.compared), "mismatches": v.mismatches}),
                        ),
                    )
                }
            }
            None => (
                "verify_failed".into(),
                Some(json!({"code": "no_readback", "message": "plugin did not return read-back settings"})),
            ),
        },
        "skipped" => ("skipped".into(), item.error.as_ref().map(|e| json!(e))),
        _ => (
            "failed".into(),
            Some(
                item.error
                    .as_ref()
                    .map(|e| json!(e))
                    .unwrap_or_else(|| json!({"code": "apply_failed", "message": "plugin reported failure"})),
            ),
        ),
    }
}

async fn apply_session(ctx: &JobContext, bridge: &BridgeHandle) -> Result<Value, JobError> {
    let session_id = payload_str(&ctx.job.payload, "sessionId")?;
    let only: Option<HashSet<String>> = ctx
        .job
        .payload
        .get("predictionIds")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect());
    let min_confidence = ctx.job.payload.get("minConfidence").and_then(Value::as_f64).unwrap_or(0.0);
    let conn = bridge.connection();
    let pf = apply_preflight(&ctx.db, &session_id, only.as_ref(), conn.as_ref())?;
    if !pf.ok {
        return Err(JobError::Failed(format!("apply refused: {}", pf.blockers.join("; "))));
    }
    let conn = conn.ok_or(BridgeError::NotConnected)?;
    let writable = conn.capabilities.writable_keys();
    let candidates: Vec<Prediction> = candidate_predictions(&ctx.db, &session_id, only.as_ref())?
        .into_iter()
        .filter(|p| p.confidence >= min_confidence)
        .collect();
    let mut assets: Vec<Asset> = Vec::new();
    for p in &candidates {
        if let Some(a) = ctx.db.get_asset(&p.asset_id)? {
            assets.push(a);
        }
    }
    ctx.db.set_session_status(&session_id, "applying")?;
    ctx.progress(0, candidates.len() as i64, "resolving photos");
    let photo_ids = match resolve_photo_ids(bridge, &conn, &ctx.db, &assets).await {
        Ok(m) => m,
        Err(e) => {
            ctx.db.set_session_status(&session_id, "predicted")?;
            return Err(e);
        }
    };
    let batch = ctx.db.create_apply_batch(&session_id, Some(&conn.catalog_fingerprint))?;
    let snapshot_name = format!("Mimic Before — {}", now_rfc3339());
    let asset_by_id: HashMap<&str, &Asset> = assets.iter().map(|a| (a.id.as_str(), a)).collect();

    let mut planned: Vec<Planned> = Vec::new();
    let mut unresolved = 0usize;
    let mut nothing_writable = 0usize;
    for p in candidates {
        let Some(photo_id) = photo_ids.get(&p.asset_id).copied() else {
            unresolved += 1;
            ctx.db.record_applied_edit(&NewAppliedEdit {
                apply_batch_id: batch.id.clone(),
                prediction_id: p.id.clone(),
                asset_id: p.asset_id.clone(),
                before_settings: None,
                applied_settings: json!({}),
                lightroom_snapshot_name: None,
                result: "skipped".into(),
                error: Some(json!({"code": "photo_not_in_catalog", "message": format!("{} is not in the open catalog", asset_by_id.get(p.asset_id.as_str()).map(|a| a.file_name.as_str()).unwrap_or("photo"))})),
            })?;
            continue;
        };
        let global = p
            .predicted_settings
            .get("global")
            .map(|g| serde_json::from_value::<BTreeMap<String, BTreeMap<String, ControlValue>>>(g.clone()))
            .transpose()
            .map_err(|e| JobError::Failed(format!("corrupt prediction {}: {e}", p.id)))?
            .unwrap_or_default();
        let write = edit_dna::to_lightroom_settings(&global, &writable);
        if write.settings.is_empty() {
            nothing_writable += 1;
            ctx.db.record_applied_edit(&NewAppliedEdit {
                apply_batch_id: batch.id.clone(),
                prediction_id: p.id.clone(),
                asset_id: p.asset_id.clone(),
                before_settings: None,
                applied_settings: json!({}),
                lightroom_snapshot_name: None,
                result: "skipped".into(),
                error: Some(json!({"code": "nothing_writable", "message": "none of the predicted controls are writable on this Lightroom", "skipped": write.skipped})),
            })?;
            continue;
        }
        planned.push(Planned { prediction: p, photo_id, settings: write.settings, skipped: write.skipped });
    }

    let total = planned.len();
    let outcome = run_apply_batches(ctx, bridge, &conn, &batch, &snapshot_name, &planned).await;
    let (canceled, bridge_error) = match outcome {
        Ok(v) => v,
        Err(e) => {
            // Unexpected (DB) failure: close the batch as failed with what was
            // recorded so far and never leave the session in `applying`.
            let _ = ctx.db.complete_apply_batch(
                &batch.id,
                false,
                Some(&json!({"code": "internal", "message": e.to_string()})),
            );
            let _ = ctx.db.set_session_status(&session_id, "predicted");
            return Err(e);
        }
    };
    let final_batch = ctx.db.complete_apply_batch(&batch.id, canceled, bridge_error.as_ref())?;
    ctx.db.set_session_status(&session_id, "applied")?;
    let summary = json!({
        "sessionId": session_id,
        "applyBatchId": final_batch.id,
        "status": final_batch.status,
        "planned": total,
        "applied": final_batch.applied_count,
        "failed": final_batch.failed_count,
        "skippedUnresolved": unresolved,
        "skippedNothingWritable": nothing_writable,
        "rollbackAvailable": final_batch.rollback_available,
        "snapshotName": snapshot_name,
        "catalogFingerprint": conn.catalog_fingerprint,
    });
    let _ = ctx
        .db
        .log_event(&NewEvent::info("sessions", "applied", summary.clone()).entity("apply_batch", &final_batch.id));
    if canceled {
        return Err(JobError::Canceled);
    }
    if let Some(e) = bridge_error {
        return Err(JobError::Failed(format!("apply stopped: {}", e["message"].as_str().unwrap_or("bridge error"))));
    }
    Ok(summary)
}

/// One planned write: the prediction, its Lightroom photo id, the flat settings
/// table and the controls the capability set could not accept.
struct Planned {
    prediction: Prediction,
    photo_id: i64,
    settings: Map<String, Value>,
    skipped: Vec<edit_dna::SkippedControl>,
}

/// Send the planned items in batches and record every outcome. Returns
/// (canceled, bridge error). Any `Err` is a database failure; bridge failures
/// are recorded per item and returned in the tuple.
async fn run_apply_batches(
    ctx: &JobContext,
    bridge: &BridgeHandle,
    conn: &ConnectionInfo,
    batch: &ApplyBatch,
    snapshot_name: &str,
    planned: &[Planned],
) -> Result<(bool, Option<Value>), JobError> {
    let total = planned.len();
    let mut done = 0usize;
    let mut canceled = false;
    let mut bridge_error: Option<Value> = None;
    for chunk in planned.chunks(APPLY_BATCH_SIZE) {
        if ctx.check_cancel().is_err() {
            canceled = true;
            break;
        }
        let items: Vec<Value> = chunk
            .iter()
            .map(|pl| {
                json!({"photoId": pl.photo_id, "predictionId": pl.prediction.id, "snapshotName": snapshot_name, "settings": pl.settings})
            })
            .collect();
        let res = bridge
            .send_command(
                CommandType::ApplySettingsAsPluginPreset,
                json!({"items": items, "createSnapshot": true, "readBack": true, "applyBatchId": batch.id}),
                APPLY_TIMEOUT,
            )
            .await;
        let result: ApplyBatchResult = match res.and_then(|v| {
            serde_json::from_value(v).map_err(|e| BridgeError::Plugin {
                code: "bad_apply_result".into(),
                message: e.to_string(),
                details: None,
            })
        }) {
            Ok(r) => r,
            Err(e) => {
                // The plugin may or may not have written these photos. Record
                // the uncertainty per item instead of guessing either way.
                let err = json!({"code": "outcome_unknown", "message": format!("bridge error during apply: {e}; check Lightroom's History panel for these photos")});
                for pl in chunk {
                    ctx.db.record_applied_edit(&NewAppliedEdit {
                        apply_batch_id: batch.id.clone(),
                        prediction_id: pl.prediction.id.clone(),
                        asset_id: pl.prediction.asset_id.clone(),
                        before_settings: None,
                        applied_settings: Value::Object(pl.settings.clone()),
                        lightroom_snapshot_name: Some(snapshot_name.to_string()),
                        result: "failed".into(),
                        error: Some(err.clone()),
                    })?;
                }
                bridge_error = Some(json!({"code": "bridge", "message": e.to_string()}));
                break;
            }
        };
        for pl in chunk {
            let item = result.items.iter().find(|i| i.prediction_id == pl.prediction.id);
            let (outcome, error, before, snapshot) = match item {
                Some(item) => {
                    let (o, e) = verify_item(&pl.settings, item);
                    (o, e, item.before.clone().map(Value::Object), item.snapshot_name.clone())
                }
                None if result.canceled => (
                    "skipped".to_string(),
                    Some(json!({"code": "canceled", "message": "canceled in Lightroom before this photo"})),
                    None,
                    None,
                ),
                None => (
                    "failed".to_string(),
                    Some(json!({"code": "no_result", "message": "plugin returned no result for this photo"})),
                    None,
                    None,
                ),
            };
            let mut applied_settings = Value::Object(pl.settings.clone());
            if !pl.skipped.is_empty() {
                applied_settings["__skippedControls"] = json!(pl.skipped);
            }
            ctx.db.record_applied_edit(&NewAppliedEdit {
                apply_batch_id: batch.id.clone(),
                prediction_id: pl.prediction.id.clone(),
                asset_id: pl.prediction.asset_id.clone(),
                before_settings: before,
                applied_settings,
                lightroom_snapshot_name: snapshot,
                result: outcome.clone(),
                error,
            })?;
            if outcome == "applied" {
                if let Some(observed) = item.and_then(|i| i.read_back.clone()) {
                    let normalized = edit_dna::normalize(&observed);
                    ctx.db.insert_edit_snapshot(&NewEditSnapshot {
                        asset_id: pl.prediction.asset_id.clone(),
                        source: "prediction".into(),
                        process_version: normalized.lightroom.process_version.clone(),
                        normalized_settings: serde_json::to_value(&normalized).map_err(|e| JobError::Failed(e.to_string()))?,
                        raw_settings: Value::Object(observed),
                        unknown_settings: Value::Object(normalized.unknown.clone()),
                        mapping_version: normalized.mapping_version.clone(),
                        capability_schema_version: Some(conn.capabilities.schema_version.clone()),
                        provenance: json!({"predictionId": pl.prediction.id, "applyBatchId": batch.id, "catalogFingerprint": conn.catalog_fingerprint}),
                    })?;
                }
            }
            done += 1;
        }
        ctx.progress(done as i64, total as i64, "applying");
        if result.canceled {
            canceled = true;
            break;
        }
    }
    Ok((canceled, bridge_error))
}

// ---------------------------------------------------------------------------
// Restore (rollback of one apply batch)
// ---------------------------------------------------------------------------

/// Settings to write back for one applied edit: the before-values of exactly
/// the keys Mimic wrote. Keys without a recorded before-value cannot be
/// restored and are reported.
pub fn restore_settings(edit: &AppliedEdit) -> (Map<String, Value>, Vec<String>) {
    let mut out = Map::new();
    let mut missing = Vec::new();
    let Some(before) = edit.before_settings.as_ref().and_then(Value::as_object) else {
        return (out, edit.applied_settings.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default());
    };
    for key in edit.applied_settings.as_object().map(|m| m.keys()).into_iter().flatten() {
        if key.starts_with("__") {
            continue;
        }
        match before.get(key) {
            Some(v) if !v.is_null() => {
                out.insert(key.clone(), v.clone());
            }
            _ => missing.push(key.clone()),
        }
    }
    (out, missing)
}

/// (edit, photo id, settings to write, keys with no before-value)
type RestorePlan = (AppliedEdit, i64, Map<String, Value>, Vec<String>);

async fn restore_batch(ctx: &JobContext, bridge: &BridgeHandle) -> Result<Value, JobError> {
    let batch_id = payload_str(&ctx.job.payload, "applyBatchId")?;
    let batch: ApplyBatch = ctx
        .db
        .get_apply_batch(&batch_id)?
        .ok_or_else(|| JobError::Failed(format!("apply batch {batch_id} not found")))?;
    let conn = bridge.connection().ok_or(BridgeError::NotConnected)?;
    if let Some(fp) = &batch.lightroom_catalog_fingerprint {
        if fp != &conn.catalog_fingerprint {
            return Err(JobError::Failed(format!(
                "batch was applied to catalog {fp} but Lightroom has {} open",
                conn.catalog_fingerprint
            )));
        }
    }
    let edits: Vec<AppliedEdit> = ctx
        .db
        .applied_edits(&batch_id)?
        .into_iter()
        .filter(|e| matches!(e.result.as_str(), "applied" | "verify_failed") && e.restore_result.is_none())
        .collect();
    if edits.is_empty() {
        return Err(JobError::Failed("nothing left to restore in this batch".into()));
    }
    let mut assets = Vec::new();
    for e in &edits {
        if let Some(a) = ctx.db.get_asset(&e.asset_id)? {
            assets.push(a);
        }
    }
    ctx.progress(0, edits.len() as i64, "resolving photos");
    let photo_ids = resolve_photo_ids(bridge, &conn, &ctx.db, &assets).await?;
    let mut restored = 0usize;
    let mut failed = 0usize;
    let mut done = 0usize;
    let total = edits.len();
    let mut plan: Vec<RestorePlan> = Vec::new();
    for e in edits {
        let Some(photo_id) = photo_ids.get(&e.asset_id).copied() else {
            ctx.db.record_restore(
                &e.id,
                "skipped",
                Some(&json!({"code": "photo_not_in_catalog", "message": "photo is not in the open catalog"})),
            )?;
            failed += 1;
            done += 1;
            continue;
        };
        let (settings, missing) = restore_settings(&e);
        if settings.is_empty() {
            ctx.db.record_restore(&e.id, "skipped", Some(&json!({"code": "no_before_state", "message": "no before-values were recorded for this edit; use the Lightroom snapshot", "missingKeys": missing})))?;
            failed += 1;
            done += 1;
            continue;
        }
        plan.push((e, photo_id, settings, missing));
    }
    for chunk in plan.chunks(APPLY_BATCH_SIZE) {
        ctx.check_cancel()?;
        let items: Vec<Value> = chunk
            .iter()
            .map(|(e, photo_id, settings, _)| json!({"photoId": photo_id, "predictionId": format!("restore-{}", e.id), "settings": settings}))
            .collect();
        let res = bridge
            .send_command(
                CommandType::ApplySettingsAsPluginPreset,
                json!({"items": items, "createSnapshot": false, "readBack": true, "applyBatchId": batch_id, "restore": true}),
                APPLY_TIMEOUT,
            )
            .await?;
        let result: ApplyBatchResult = serde_json::from_value(res)
            .map_err(|e| JobError::Failed(format!("bad restore result from plugin: {e}")))?;
        for (e, _, settings, missing) in chunk {
            let item = result.items.iter().find(|i| i.prediction_id == format!("restore-{}", e.id));
            let (outcome, mut error) = match item {
                Some(item) => {
                    let (o, err) = verify_item(settings, item);
                    (if o == "applied" { "restored".to_string() } else { o }, err)
                }
                None => {
                    ("failed".to_string(), Some(json!({"code": "no_result", "message": "plugin returned no result"})))
                }
            };
            if !missing.is_empty() {
                let e = error.get_or_insert_with(|| json!({}));
                e["missingKeys"] = json!(missing);
            }
            ctx.db.record_restore(&e.id, &outcome, error.as_ref())?;
            if outcome == "restored" {
                restored += 1;
                ctx.db.set_prediction_status(&e.prediction_id, "pending")?;
            } else {
                failed += 1;
            }
            done += 1;
        }
        ctx.progress(done as i64, total as i64, "restoring");
        if result.canceled {
            return Err(JobError::Canceled);
        }
    }
    let remaining = ctx
        .db
        .applied_edits(&batch_id)?
        .iter()
        .filter(|e| {
            matches!(e.result.as_str(), "applied" | "verify_failed") && e.restore_result.as_deref() != Some("restored")
        })
        .count();
    ctx.db.set_batch_rollback_available(&batch_id, remaining > 0)?;
    let summary = json!({
        "applyBatchId": batch_id,
        "restored": restored,
        "failed": failed,
        "remaining": remaining,
    });
    let _ = ctx.db.log_event(&NewEvent::info("sessions", "restored", summary.clone()).entity("apply_batch", &batch_id));
    Ok(summary)
}

// ---------------------------------------------------------------------------
// Read models for the UI
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub session: Session,
    pub source: Option<SessionSource>,
    pub clusters: Vec<SceneCluster>,
    pub prediction_counts: BTreeMap<String, i64>,
    pub batches: Vec<ApplyBatch>,
    pub correction_syncs: Vec<crate::db::CorrectionSync>,
    pub photo_count: i64,
    pub photos_with_features: i64,
    pub grouped: bool,
}

pub fn session_detail(db: &Db, session_id: &str) -> Result<SessionDetail, DbError> {
    let session = db.get_session(session_id)?.ok_or_else(|| DbError::NotFound(session_id.to_string()))?;
    let members = db.session_assets(session_id)?;
    let photos_with_features = match &session.source_library_id {
        Some(lib) => members.len() as i64 - db.assets_missing_features(lib, FEATURE_VERSION)?.len() as i64,
        None => 0,
    };
    Ok(SessionDetail {
        source: session_source(db, session_id)?,
        clusters: db.scene_clusters(session_id)?,
        prediction_counts: db.count_session_predictions_by_status(session_id)?.into_iter().collect(),
        batches: db.session_apply_batches(session_id)?,
        correction_syncs: db.correction_syncs(session_id)?,
        photo_count: members.len() as i64,
        photos_with_features,
        grouped: members.iter().any(|m| m.cluster_id.is_some()),
        session,
    })
}

/// One row of the session grid / review queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPhoto {
    pub asset: Asset,
    pub sequence_index: i64,
    pub cluster_id: Option<String>,
    pub burst_id: Option<String>,
    pub preview_path: Option<String>,
    pub prediction: Option<Prediction>,
    pub last_apply: Option<AppliedEdit>,
}

pub fn session_photos(db: &Db, session_id: &str) -> Result<Vec<SessionPhoto>, DbError> {
    let members = db.session_assets(session_id)?;
    let preds: HashMap<String, Prediction> =
        db.session_predictions(session_id, false)?.into_iter().map(|p| (p.asset_id.clone(), p)).collect();
    let mut last_apply: HashMap<String, AppliedEdit> = HashMap::new();
    for b in db.session_apply_batches(session_id)? {
        for e in db.applied_edits(&b.id)? {
            last_apply.entry(e.prediction_id.clone()).or_insert(e);
        }
    }
    let mut out = Vec::with_capacity(members.len());
    for m in members {
        let Some(asset) = db.get_asset(&m.asset_id)? else { continue };
        let preview_path = db.visual_features(&asset.id, FEATURE_VERSION)?.and_then(|f| f.preview_path);
        let prediction = preds.get(&asset.id).cloned();
        let apply = prediction.as_ref().and_then(|p| last_apply.get(&p.id).cloned());
        out.push(SessionPhoto {
            asset,
            sequence_index: m.sequence_index,
            cluster_id: m.cluster_id,
            burst_id: m.burst_id,
            preview_path,
            prediction,
            last_apply: apply,
        });
    }
    Ok(out)
}

/// Review decision on a prediction. Only `reviewed` and `rejected` are user
/// settable; `applied`/`superseded` are owned by the jobs.
pub fn set_review_status(db: &Db, prediction_id: &str, status: &str) -> Result<Prediction, DbError> {
    if !matches!(status, "pending" | "reviewed" | "rejected") {
        return Err(DbError::Invalid(format!("review status must be pending, reviewed or rejected (got {status})")));
    }
    let p = db.get_prediction(prediction_id)?.ok_or_else(|| DbError::NotFound(prediction_id.to_string()))?;
    if matches!(p.status.as_str(), "applied" | "superseded") {
        return Err(DbError::Invalid(format!("prediction is {}; it cannot be re-reviewed", p.status)));
    }
    db.set_prediction_status(prediction_id, status)?;
    db.get_prediction(prediction_id)?.ok_or_else(|| DbError::NotFound(prediction_id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_settings_uses_only_written_keys_with_before_values() {
        let edit = AppliedEdit {
            id: "e".into(),
            apply_batch_id: "b".into(),
            prediction_id: "p".into(),
            asset_id: "a".into(),
            before_settings: Some(json!({"Exposure2012": 0.0, "Contrast2012": 5, "Vibrance": 0, "Temperature": null})),
            applied_settings: json!({"Exposure2012": 0.4, "Contrast2012": 12, "Temperature": 5200, "__skippedControls": []}),
            lightroom_snapshot_name: None,
            result: "applied".into(),
            error: None,
            applied_at: "t".into(),
            restored_at: None,
            restore_result: None,
            restore_error: None,
        };
        let (settings, missing) = restore_settings(&edit);
        assert_eq!(settings, json!({"Exposure2012": 0.0, "Contrast2012": 5}).as_object().unwrap().clone());
        assert_eq!(missing, vec!["Temperature".to_string()]);
        let no_before = AppliedEdit { before_settings: None, ..edit };
        let (settings, missing) = restore_settings(&no_before);
        assert!(settings.is_empty());
        assert_eq!(missing.len(), 4);
    }

    #[test]
    fn global_from_engine_maps_known_controls_only() {
        let g =
            json!({"tone": {"exposure": {"value": 0.6, "raw": 1.0}, "bogus": {"raw": 1}}, "nope": {"x": {"raw": 1}}});
        let m = global_from_engine(&g);
        assert_eq!(m.len(), 1);
        let cv = &m["tone"]["exposure"];
        assert_eq!(cv.raw, json!(1.0));
        assert_eq!(cv.source_key, "Exposure2012");
        assert!(!m["tone"].contains_key("bogus"));
    }

    #[test]
    fn preflight_refuses_without_lightroom_or_predictions() {
        let db = Db::open_in_memory().unwrap();
        let s = db.create_session("S", None, None, None).unwrap();
        let pf = apply_preflight(&db, &s.id, None, None).unwrap();
        assert!(!pf.ok);
        assert_eq!(pf.blockers.len(), 2, "{:?}", pf.blockers);
        assert!(pf.blockers[1].contains("not connected"));
        let missing = apply_preflight(&db, "nope", None, None).unwrap();
        assert_eq!(missing.blockers, vec!["session not found".to_string()]);
    }

    #[test]
    fn ui_fixtures_round_trip_through_the_read_models() {
        // The same files are asserted by packages/contracts (zod), so a rename
        // on either side fails a test instead of rendering undefined.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sessions");
        let photo_json: Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("session_photo.json")).unwrap()).unwrap();
        let photo: SessionPhoto = serde_json::from_value(photo_json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&photo).unwrap(), photo_json, "field names/shape drifted");
        assert_eq!(photo.last_apply.as_ref().unwrap().result, "verify_failed");
        let pf_json: Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("apply_preflight.refused.json")).unwrap()).unwrap();
        let pf: ApplyPreflight = serde_json::from_value(pf_json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&pf).unwrap(), pf_json);
        assert!(!pf.ok && pf.stale_count == 2);
    }

    #[test]
    fn review_status_transitions() {
        let db = Db::open_in_memory().unwrap();
        assert!(set_review_status(&db, "missing", "reviewed").is_err());
        assert!(matches!(set_review_status(&db, "x", "applied"), Err(DbError::Invalid(_))));
    }
}
