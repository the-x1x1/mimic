//! Historical ingest: folder+sidecar scanning and Lightroom-connected capture,
//! joined into `assets` / `sidecars` / `edit_snapshots` / `visual_features`,
//! plus the data-quality report (spec §9).
//!
//! Source media is only ever read. Sidecars are only ever read.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::bridge::{BridgeHandle, CommandType};
use crate::db::{Db, NewAsset, NewEditSnapshot, NewEvent, NewSidecar, VisualFeatures};
use crate::edit_dna;
use crate::engine::EngineClient;
use crate::jobs::{JobContext, JobError, JobExecutor, JobFuture};

pub const FEATURE_VERSION: &str = "features_v1";
const ANALYZE_BATCH: usize = 32;
const LIGHTROOM_BATCH: usize = 25;

pub const JOB_SCAN_LIBRARY: &str = "scan_library";
pub const JOB_INGEST_LIGHTROOM: &str = "ingest_lightroom";
pub const JOB_ANALYZE_LIBRARY: &str = "analyze_library";

/// One scanned file as the engine reports it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedAsset {
    pub source_path: String,
    pub file_name: String,
    pub extension: String,
    #[serde(default)]
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    #[serde(default)]
    pub modified_time: Option<String>,
    pub fast_hash: String,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default)]
    pub sidecars: Vec<ScannedSidecar>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedSidecar {
    #[serde(rename = "type")]
    pub kind: String,
    pub path: String,
    #[serde(default)]
    pub modified_time: Option<String>,
    #[serde(default)]
    pub hash: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    #[serde(default)]
    pub assets: Vec<ScannedAsset>,
    #[serde(default)]
    pub unsupported: Vec<Value>,
    #[serde(default)]
    pub duplicate_basenames: Vec<Value>,
    #[serde(default)]
    pub orphan_sidecars: Vec<String>,
    #[serde(default)]
    pub stats: Value,
}

pub struct IngestExecutor {
    pub engine: EngineClient,
    pub bridge: BridgeHandle,
}

impl JobExecutor for IngestExecutor {
    fn kinds(&self) -> &'static [&'static str] {
        &[JOB_SCAN_LIBRARY, JOB_INGEST_LIGHTROOM, JOB_ANALYZE_LIBRARY]
    }

    fn resumable(&self, kind: &str) -> bool {
        // All three are idempotent (upserts keyed on identity), so they may be
        // re-queued after an interrupted run.
        matches!(kind, JOB_SCAN_LIBRARY | JOB_INGEST_LIGHTROOM | JOB_ANALYZE_LIBRARY)
    }

    fn execute(&self, ctx: JobContext) -> JobFuture {
        let engine = self.engine.clone();
        let bridge = self.bridge.clone();
        Box::pin(async move {
            match ctx.job.kind.as_str() {
                JOB_SCAN_LIBRARY => scan_library(&ctx, &engine).await,
                JOB_INGEST_LIGHTROOM => ingest_lightroom(&ctx, &engine, &bridge).await,
                JOB_ANALYZE_LIBRARY => {
                    let library_id = payload_str(&ctx.job.payload, "libraryId")?;
                    let n = analyze_missing_features(&ctx, &engine, &library_id, 0).await?;
                    Ok(json!({"analyzed": n}))
                }
                other => Err(JobError::Failed(format!("unknown job kind {other}"))),
            }
        })
    }
}

fn payload_str(payload: &Value, key: &str) -> Result<String, JobError> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| JobError::Failed(format!("job payload missing {key}")))
}

fn opt_str(m: &Map<String, Value>, k: &str) -> Option<String> {
    m.get(k).and_then(|v| match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}
fn opt_f64(m: &Map<String, Value>, k: &str) -> Option<f64> {
    m.get(k).and_then(edit_dna::parse_number)
}
fn opt_i64(m: &Map<String, Value>, k: &str) -> Option<i64> {
    m.get(k).and_then(edit_dna::parse_number).map(|f| f.round() as i64)
}

#[allow(clippy::too_many_arguments)]
fn asset_from_metadata(
    library_id: &str,
    source_path: &str,
    file_name: &str,
    extension: &str,
    size: i64,
    modified: Option<String>,
    fast_hash: &str,
    m: &Map<String, Value>,
) -> NewAsset {
    NewAsset {
        library_id: Some(library_id.to_string()),
        source_path: source_path.to_string(),
        file_name: file_name.to_string(),
        extension: extension.to_string(),
        mime_type: opt_str(m, "mimeType"),
        size_bytes: size,
        modified_time: modified,
        fast_hash: fast_hash.to_string(),
        camera_make: opt_str(m, "cameraMake"),
        camera_model: opt_str(m, "cameraModel"),
        lens: opt_str(m, "lens"),
        focal_length: opt_f64(m, "focalLength"),
        iso: opt_i64(m, "iso"),
        aperture: opt_f64(m, "aperture"),
        shutter_speed: opt_f64(m, "shutterSpeed"),
        captured_at: opt_str(m, "capturedAt"),
        width: opt_i64(m, "width"),
        height: opt_i64(m, "height"),
        orientation: opt_i64(m, "orientation"),
        lightroom_local_id: opt_i64(m, "lightroomLocalId"),
    }
}

/// Folder + sidecar ingest.
async fn scan_library(ctx: &JobContext, engine: &EngineClient) -> Result<Value, JobError> {
    let library_id = payload_str(&ctx.job.payload, "libraryId")?;
    let library =
        ctx.db.get_library(&library_id)?.ok_or_else(|| JobError::Failed(format!("library {library_id} not found")))?;
    let root = library.root_path.clone().ok_or_else(|| JobError::Failed("library has no root path".into()))?;
    ctx.db.set_library_status(&library_id, "scanning", false)?;
    ctx.progress(0, 0, "scanning");

    let scan: ScanResult = serde_json::from_value(
        engine
            .call_with_timeout(
                "scan.folder",
                json!({"roots": [root], "jobId": ctx.job.id, "includeMetadata": true}),
                Duration::from_secs(3600),
            )
            .await?,
    )
    .map_err(|e| JobError::Failed(format!("bad scan result: {e}")))?;

    let total = scan.assets.len() as i64;
    let mut inserted = 0usize;
    let mut updated = 0usize;
    let mut parsed_xmp = 0usize;
    let mut failed_xmp = 0usize;
    let mut acr_count = 0usize;
    let mut snapshots_reused = 0usize;

    for (i, a) in scan.assets.iter().enumerate() {
        ctx.check_cancel()?;
        let new_asset = asset_from_metadata(
            &library_id,
            &a.source_path,
            &a.file_name,
            &a.extension,
            a.size_bytes,
            a.modified_time.clone(),
            &a.fast_hash,
            &a.metadata,
        );
        let (asset, outcome) = ctx.db.upsert_asset(&new_asset)?;
        match outcome {
            crate::db::UpsertOutcome::Inserted => inserted += 1,
            crate::db::UpsertOutcome::Updated => updated += 1,
        }
        let existing_sidecars = ctx.db.sidecars_for_asset(&asset.id)?;
        for sc in &a.sidecars {
            if sc.kind == "acr" {
                acr_count += 1;
                ctx.db.upsert_sidecar(&NewSidecar {
                    asset_id: asset.id.clone(),
                    kind: "acr".into(),
                    path: sc.path.clone(),
                    modified_time: sc.modified_time.clone(),
                    hash: sc.hash.clone(),
                    parse_status: "opaque".into(),
                    parser_version: None,
                    raw_metadata: None,
                    warnings: Some(json!(["ACR sidecar detected; heavy edits are not readable offline"])),
                })?;
                continue;
            }
            // Skip re-parsing an unchanged XMP that already produced a snapshot.
            let unchanged = existing_sidecars
                .iter()
                .any(|e| e.path == sc.path && e.parse_status == "parsed" && e.hash.is_some() && e.hash == sc.hash);
            if unchanged && ctx.db.latest_observed_snapshot(&asset.id)?.is_some() {
                snapshots_reused += 1;
                continue;
            }
            match engine.call("xmp.parse", json!({"path": sc.path})).await {
                Ok(parsed) => {
                    let raw = parsed.get("rawSettings").and_then(Value::as_object).cloned().unwrap_or_default();
                    let warnings = parsed.get("warnings").cloned().unwrap_or(json!([]));
                    let parser_version =
                        parsed.get("parserVersion").and_then(Value::as_str).unwrap_or("unknown").to_string();
                    let source_hash = parsed
                        .get("sourceHash")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| sc.hash.clone());
                    ctx.db.upsert_sidecar(&NewSidecar {
                        asset_id: asset.id.clone(),
                        kind: "xmp".into(),
                        path: sc.path.clone(),
                        modified_time: sc.modified_time.clone(),
                        hash: sc.hash.clone(),
                        parse_status: "parsed".into(),
                        parser_version: Some(parser_version.clone()),
                        raw_metadata: parsed.get("metadata").cloned(),
                        warnings: Some(warnings.clone()),
                    })?;
                    if raw.is_empty() {
                        continue;
                    }
                    let normalized = edit_dna::normalize(&raw);
                    ctx.db.insert_edit_snapshot(&NewEditSnapshot {
                        asset_id: asset.id.clone(),
                        source: "xmp".into(),
                        process_version: normalized.lightroom.process_version.clone(),
                        normalized_settings: serde_json::to_value(&normalized).map_err(|e| JobError::Failed(e.to_string()))?,
                        raw_settings: Value::Object(raw),
                        unknown_settings: Value::Object(normalized.unknown.clone()),
                        mapping_version: normalized.mapping_version.clone(),
                        capability_schema_version: None,
                        provenance: json!({"parserVersion": parser_version, "sidecarPath": sc.path, "sourceHash": source_hash, "warnings": warnings}),
                    })?;
                    parsed_xmp += 1;
                }
                Err(e) => {
                    failed_xmp += 1;
                    ctx.db.upsert_sidecar(&NewSidecar {
                        asset_id: asset.id.clone(),
                        kind: "xmp".into(),
                        path: sc.path.clone(),
                        modified_time: sc.modified_time.clone(),
                        hash: sc.hash.clone(),
                        parse_status: "failed".into(),
                        parser_version: None,
                        raw_metadata: None,
                        warnings: Some(json!([e.to_string()])),
                    })?;
                }
            }
        }
        if i % 10 == 0 || i as i64 + 1 == total {
            ctx.progress(i as i64 + 1, total, "ingesting");
        }
    }

    let analyzed = analyze_missing_features(ctx, engine, &library_id, total).await?;
    ctx.db.set_library_status(&library_id, "scanned", true)?;
    let summary = json!({
        "assetsFound": total,
        "inserted": inserted,
        "updated": updated,
        "xmpParsed": parsed_xmp,
        "xmpFailed": failed_xmp,
        "xmpUnchanged": snapshots_reused,
        "acrSidecars": acr_count,
        "unsupported": scan.unsupported.len(),
        "duplicateBasenames": scan.duplicate_basenames.len(),
        "orphanSidecars": scan.orphan_sidecars.len(),
        "analyzed": analyzed,
        "scanStats": scan.stats,
    });
    let _ = ctx.db.log_event(&NewEvent::info("jobs", "scan_completed", summary.clone()).entity("library", &library_id));
    Ok(summary)
}

/// Compute visual features for assets that lack them. Returns count analyzed.
async fn analyze_missing_features(
    ctx: &JobContext,
    engine: &EngineClient,
    library_id: &str,
    offset_total: i64,
) -> Result<usize, JobError> {
    let missing = ctx.db.assets_missing_features(library_id, FEATURE_VERSION)?;
    let total = missing.len() as i64;
    let mut done = 0usize;
    for chunk in missing.chunks(ANALYZE_BATCH) {
        ctx.check_cancel()?;
        let items: Vec<Value> = chunk.iter().map(|(id, path)| json!({"assetId": id, "path": path})).collect();
        let res = engine
            .call_with_timeout(
                "image.analyze_batch",
                json!({"items": items, "featureVersion": FEATURE_VERSION, "jobId": ctx.job.id}),
                Duration::from_secs(1800),
            )
            .await?;
        let results = res.get("results").and_then(Value::as_array).cloned().unwrap_or_default();
        for r in results {
            let Some(asset_id) = r.get("assetId").and_then(Value::as_str) else { continue };
            if let Some(err) = r.get("error") {
                let _ = ctx
                    .db
                    .log_event(&NewEvent::warn("engine", "analyze_failed", err.clone()).entity("asset", asset_id));
                continue;
            }
            let f = VisualFeatures {
                asset_id: asset_id.to_string(),
                feature_version: r.get("featureVersion").and_then(Value::as_str).unwrap_or(FEATURE_VERSION).to_string(),
                histogram: r.get("histogram").cloned().unwrap_or(json!({})),
                luminance: r.get("luminance").cloned().unwrap_or(json!({})),
                color: r.get("color").cloned().unwrap_or(json!({})),
                sharpness: r.get("sharpness").and_then(Value::as_f64),
                noise_estimate: r.get("noiseEstimate").and_then(Value::as_f64),
                clipping: r.get("clipping").cloned().unwrap_or(json!({})),
                scene_labels: r.get("sceneLabels").cloned().unwrap_or(json!({})),
                embedding_artifact_id: r.get("embeddingArtifactId").and_then(Value::as_str).map(str::to_string),
                preview_path: r.get("previewPath").and_then(Value::as_str).map(str::to_string),
                computed_at: crate::ids::now_rfc3339(),
            };
            ctx.db.upsert_visual_features(&f)?;
            done += 1;
        }
        ctx.progress(done as i64, total.max(offset_total.min(total)), "analyzing");
    }
    Ok(done)
}

/// Lightroom-connected ingest through the bridge.
async fn ingest_lightroom(ctx: &JobContext, engine: &EngineClient, bridge: &BridgeHandle) -> Result<Value, JobError> {
    let library_id = payload_str(&ctx.job.payload, "libraryId")?;
    let scope = ctx.job.payload.get("scope").and_then(Value::as_str).unwrap_or("selection").to_string();
    let conn = bridge.connection().ok_or(crate::bridge::BridgeError::NotConnected)?;
    let library = ctx.db.get_library(&library_id)?.ok_or_else(|| JobError::Failed("library not found".into()))?;
    if let Some(fp) = &library.lightroom_catalog_fingerprint {
        if fp != &conn.catalog_fingerprint {
            return Err(JobError::Failed(format!(
                "library was created from catalog {fp} but Lightroom has {} open",
                conn.catalog_fingerprint
            )));
        }
    }
    ctx.db.set_library_status(&library_id, "scanning", false)?;
    ctx.progress(0, 0, "listing photos");

    let listing = bridge
        .send_command(
            CommandType::GetSelectedPhotos,
            json!({"scope": scope, "maxPhotos": 50_000}),
            Duration::from_secs(600),
        )
        .await?;
    let photos = listing.get("photos").and_then(Value::as_array).cloned().unwrap_or_default();
    let total = photos.len() as i64;
    let capability_version = conn.capabilities.schema_version.clone();
    let mut captured = 0usize;
    let mut missing_files = 0usize;
    let mut no_edits = 0usize;

    for (batch_index, chunk) in photos.chunks(LIGHTROOM_BATCH).enumerate() {
        ctx.check_cancel()?;
        let ids: Vec<i64> = chunk.iter().filter_map(|p| p.get("photoId").and_then(Value::as_i64)).collect();
        let settings_res = bridge
            .send_command(CommandType::GetDevelopSettings, json!({"photoIds": ids}), Duration::from_secs(120))
            .await?;
        let settings_items = settings_res.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
        for p in chunk {
            let photo_id = p.get("photoId").and_then(Value::as_i64).unwrap_or_default();
            let path = p.get("path").and_then(Value::as_str).unwrap_or("").to_string();
            let file_exists = !path.is_empty() && std::path::Path::new(&path).is_file();
            if !file_exists {
                missing_files += 1;
            }
            let mut meta = p.get("metadata").and_then(Value::as_object).cloned().unwrap_or_default();
            meta.insert("lightroomLocalId".into(), json!(photo_id));
            // Enrich EXIF from the file when it is reachable.
            if file_exists {
                if let Ok(m) = engine.call("image.metadata", json!({"path": path})).await {
                    if let Some(obj) = m.as_object() {
                        for (k, v) in obj {
                            meta.entry(k.clone()).or_insert(v.clone());
                        }
                    }
                }
            }
            let file_name = std::path::Path::new(&path)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| format!("lr-{photo_id}"));
            let extension =
                std::path::Path::new(&path).extension().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            let size = std::fs::metadata(&path).map(|m| m.len() as i64).unwrap_or(0);
            let fast_hash = format!("lr:{}:{photo_id}", conn.catalog_fingerprint);
            let new_asset =
                asset_from_metadata(&library_id, &path, &file_name, &extension, size, None, &fast_hash, &meta);
            let (asset, _) = ctx.db.upsert_asset(&new_asset)?;

            let item = settings_items.iter().find(|s| s.get("photoId").and_then(Value::as_i64) == Some(photo_id));
            let raw = item.and_then(|s| s.get("settings")).and_then(Value::as_object).cloned();
            match raw {
                Some(raw) if !raw.is_empty() => {
                    let normalized = edit_dna::normalize(&raw);
                    ctx.db.insert_edit_snapshot(&NewEditSnapshot {
                        asset_id: asset.id.clone(),
                        source: "lightroom_sdk".into(),
                        process_version: normalized.lightroom.process_version.clone(),
                        normalized_settings: serde_json::to_value(&normalized).map_err(|e| JobError::Failed(e.to_string()))?,
                        raw_settings: Value::Object(raw),
                        unknown_settings: Value::Object(normalized.unknown.clone()),
                        mapping_version: normalized.mapping_version.clone(),
                        capability_schema_version: Some(capability_version.clone()),
                        provenance: json!({"catalogFingerprint": conn.catalog_fingerprint, "lightroomVersion": conn.lightroom_version, "photoId": photo_id, "pluginVersion": conn.plugin_version}),
                    })?;
                    captured += 1;
                }
                _ => no_edits += 1,
            }
        }
        ctx.progress(
            ((batch_index + 1) * LIGHTROOM_BATCH).min(total as usize) as i64,
            total,
            "capturing develop settings",
        );
    }

    let analyzed = analyze_missing_features(ctx, engine, &library_id, total).await?;
    ctx.db.set_library_status(&library_id, "scanned", true)?;
    if library.lightroom_catalog_fingerprint.is_none() {
        // First successful ingest pins the library to this catalog.
        let _ = ctx.db.conn().execute(
            "UPDATE libraries SET lightroom_catalog_fingerprint = ?2 WHERE id = ?1",
            rusqlite::params![library_id, conn.catalog_fingerprint],
        );
    }
    let summary = json!({
        "photosListed": total,
        "settingsCaptured": captured,
        "photosWithoutEdits": no_edits,
        "missingFiles": missing_files,
        "analyzed": analyzed,
        "catalogFingerprint": conn.catalog_fingerprint,
        "lightroomVersion": conn.lightroom_version,
    });
    let _ = ctx
        .db
        .log_event(&NewEvent::info("lightroom", "ingest_completed", summary.clone()).entity("library", &library_id));
    Ok(summary)
}

// ---------------------------------------------------------------------------
// Data quality report
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataQualityReport {
    pub library_id: String,
    pub assets_found: i64,
    pub valid_pairs: i64,
    pub missing_edits: i64,
    pub features_computed: i64,
    pub lightroom_connected_pairs: i64,
    pub sidecar_only_pairs: i64,
    pub acr_heavy_edit_count: i64,
    pub local_edit_count: i64,
    pub failed_sidecars: i64,
    pub cameras: Vec<CountRow>,
    pub capture_days: Vec<CountRow>,
    pub recommendation: Recommendation,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountRow {
    pub label: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    /// `insufficient` | `minimal` | `good` | `strong`
    pub level: String,
    pub headline: String,
    pub detail: String,
}

pub const MIN_PAIRS_TO_TRAIN: i64 = 30;

pub fn data_quality_report(db: &Db, library_id: &str) -> Result<DataQualityReport, crate::db::DbError> {
    let assets_found = db.count_assets(Some(library_id))?;
    let valid_pairs = db.count_assets_with_edits(library_id)?;
    let breakdown = db.edit_source_breakdown(library_id)?;
    let lightroom_connected_pairs = breakdown.iter().find(|(s, _)| s == "lightroom_sdk").map(|(_, n)| *n).unwrap_or(0);
    let sidecar_only_pairs = breakdown.iter().find(|(s, _)| s == "xmp").map(|(_, n)| *n).unwrap_or(0);
    let (features_computed, acr_count, local_edit_count, failed_sidecars): (i64, i64, i64, i64) = {
        let conn = db.conn();
        let features: i64 = conn.query_row(
            "SELECT COUNT(*) FROM visual_features f JOIN assets a ON a.id = f.asset_id WHERE a.library_id = ?1 AND f.feature_version = ?2",
            rusqlite::params![library_id, FEATURE_VERSION],
            |r| r.get(0),
        )?;
        let acr: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sidecars s JOIN assets a ON a.id = s.asset_id WHERE a.library_id = ?1 AND s.type = 'acr'",
            [library_id],
            |r| r.get(0),
        )?;
        let local: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT s.asset_id) FROM edit_snapshots s JOIN assets a ON a.id = s.asset_id
             WHERE a.library_id = ?1 AND json_extract(s.normalized_settings_json, '$.local.status') = 'observed'",
            [library_id],
            |r| r.get(0),
        )?;
        let failed: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sidecars s JOIN assets a ON a.id = s.asset_id WHERE a.library_id = ?1 AND s.parse_status = 'failed'",
            [library_id],
            |r| r.get(0),
        )?;
        (features, acr, local, failed)
    };
    let cameras: Vec<CountRow> =
        db.camera_distribution(library_id)?.into_iter().map(|(label, count)| CountRow { label, count }).collect();
    let capture_days: Vec<CountRow> =
        db.capture_date_distribution(library_id)?.into_iter().map(|(label, count)| CountRow { label, count }).collect();

    let mut warnings = Vec::new();
    if acr_count > 0 {
        warnings.push("Some Lightroom edits are stored in an ACR sidecar and cannot be fully read offline. Connect Lightroom Classic for the most complete training data.".into());
    }
    if assets_found > 0 && valid_pairs == 0 {
        warnings.push("We found RAW files, but not enough Lightroom edit metadata to learn from. Enable “Automatically write changes into XMP” in Lightroom, or connect Lightroom Classic.".into());
    }
    if let Some(top) = cameras.first() {
        if valid_pairs >= MIN_PAIRS_TO_TRAIN && cameras.len() > 1 && top.count * 10 >= assets_found * 9 {
            warnings.push(
                "Most examples are from one camera. Mimic can train, but confidence will be lower for other cameras."
                    .into(),
            );
        }
    }
    if capture_days.len() == 1 && valid_pairs >= MIN_PAIRS_TO_TRAIN {
        warnings.push("All examples come from a single day. A session-grouped holdout split is not possible until more shoots are added; validation will be optimistic.".into());
    }
    if local_edit_count > 0 {
        warnings.push(format!("{local_edit_count} photos contain masks or local adjustments. Mimic 0.x learns global edits only; local work is preserved but not predicted."));
    }
    if failed_sidecars > 0 {
        warnings.push(format!("{failed_sidecars} sidecar files could not be parsed and were skipped."));
    }

    let missing_edits = (assets_found - valid_pairs).max(0);
    let recommendation = if valid_pairs < MIN_PAIRS_TO_TRAIN {
        Recommendation {
            level: "insufficient".into(),
            headline: format!("Not enough edited examples yet ({valid_pairs} of {MIN_PAIRS_TO_TRAIN} minimum)"),
            detail: "Add more edited photos or connect Lightroom to capture develop settings directly.".into(),
        }
    } else if valid_pairs < 200 {
        Recommendation {
            level: "minimal".into(),
            headline: format!("{valid_pairs} edited examples — enough to train a first version"),
            detail: "Expect wider confidence bands; more sessions across lighting conditions will help most.".into(),
        }
    } else if valid_pairs < 2000 {
        Recommendation {
            level: "good".into(),
            headline: format!("{valid_pairs} edited examples — good coverage"),
            detail: "Train a version and review holdout metrics before applying to a new session.".into(),
        }
    } else {
        Recommendation {
            level: "strong".into(),
            headline: format!("{valid_pairs} edited examples — strong dataset"),
            detail: "Session-grouped validation should be representative.".into(),
        }
    };

    Ok(DataQualityReport {
        library_id: library_id.to_string(),
        assets_found,
        valid_pairs,
        missing_edits,
        features_computed,
        lightroom_connected_pairs,
        sidecar_only_pairs,
        acr_heavy_edit_count: acr_count,
        local_edit_count,
        failed_sidecars,
        cameras,
        capture_days,
        recommendation,
        warnings,
    })
}

/// Build an `IngestExecutor` behind the trait object the runner wants.
pub fn executor(engine: EngineClient, bridge: BridgeHandle) -> Arc<dyn JobExecutor> {
    Arc::new(IngestExecutor { engine, bridge })
}

/// Set of extensions Mimic treats as source photographs.
pub fn supported_extensions() -> HashSet<&'static str> {
    [
        "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2", "raf", "orf", "rw2", "pef", "dng", "3fr", "fff", "iiq", "erf",
        "mrw", "x3f", "srw", "kdc", "dcr", "mef", "mos", "rwl", "jpg", "jpeg", "tif", "tiff", "heic", "heif", "psd",
    ]
    .into_iter()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_on_empty_library_is_insufficient() {
        let db = Db::open_in_memory().unwrap();
        let lib = db.create_library("L", "folder_sidecars", Some("/p"), None).unwrap();
        let r = data_quality_report(&db, &lib.id).unwrap();
        assert_eq!(r.assets_found, 0);
        assert_eq!(r.recommendation.level, "insufficient");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn report_flags_raw_without_edits_and_acr() {
        let db = Db::open_in_memory().unwrap();
        let lib = db.create_library("L", "folder_sidecars", Some("/p"), None).unwrap();
        for i in 0..3 {
            let (asset, _) = db
                .upsert_asset(&NewAsset {
                    library_id: Some(lib.id.clone()),
                    source_path: format!("/p/{i}.cr3"),
                    file_name: format!("{i}.cr3"),
                    extension: "cr3".into(),
                    fast_hash: format!("h{i}"),
                    camera_make: Some("Canon".into()),
                    camera_model: Some("R5".into()),
                    captured_at: Some("2025-04-12T10:00:00Z".into()),
                    ..Default::default()
                })
                .unwrap();
            if i == 0 {
                db.upsert_sidecar(&NewSidecar {
                    asset_id: asset.id.clone(),
                    kind: "acr".into(),
                    path: "/p/0.acr".into(),
                    modified_time: None,
                    hash: None,
                    parse_status: "opaque".into(),
                    parser_version: None,
                    raw_metadata: None,
                    warnings: None,
                })
                .unwrap();
            }
        }
        let r = data_quality_report(&db, &lib.id).unwrap();
        assert_eq!(r.assets_found, 3);
        assert_eq!(r.valid_pairs, 0);
        assert_eq!(r.missing_edits, 3);
        assert_eq!(r.acr_heavy_edit_count, 1);
        assert!(r.warnings.iter().any(|w| w.contains("ACR")));
        assert!(r.warnings.iter().any(|w| w.contains("not enough Lightroom edit metadata")));
        assert_eq!(r.cameras[0].label, "Canon R5");
        assert_eq!(r.capture_days[0].label, "2025-04-12");
    }
}
