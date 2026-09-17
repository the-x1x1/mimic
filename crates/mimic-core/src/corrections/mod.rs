//! Continuous learning (spec §26–§28): after an apply, read each photo's
//! current develop settings back from Lightroom, keep the photographer's
//! final edit as a `correction`, and derive the No-Touch Rate from what was
//! left alone. Corrections become training pairs for the next version.
//!
//! Nothing here writes to Lightroom: `collect_correction_state` is a read.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::bridge::{BridgeError, BridgeHandle, CommandType};
use crate::db::{AppliedEdit, Db, DbError, NewEditSnapshot, NewEvent, NoTouchStats};
use crate::edit_dna::{self, ControlValue, MAPPING};
use crate::jobs::{JobContext, JobError, JobExecutor, JobFuture};
use crate::sessions::{resolve_photo_ids, APPLY_BATCH_SIZE};

pub const JOB_SYNC_CORRECTIONS: &str = "sync_corrections";
const READ_TIMEOUT: Duration = Duration::from_secs(600);

pub struct CorrectionsExecutor {
    pub bridge: BridgeHandle,
}

impl JobExecutor for CorrectionsExecutor {
    fn kinds(&self) -> &'static [&'static str] {
        &[JOB_SYNC_CORRECTIONS]
    }
    fn resumable(&self, _kind: &str) -> bool {
        true // read-only against Lightroom; idempotent per prediction
    }
    fn execute(&self, ctx: JobContext) -> JobFuture {
        let bridge = self.bridge.clone();
        Box::pin(async move { sync_corrections(&ctx, &bridge).await })
    }
}

pub fn executor(bridge: BridgeHandle) -> Arc<dyn JobExecutor> {
    Arc::new(CorrectionsExecutor { bridge })
}

/// Per-control difference between what Mimic applied and what the
/// photographer ended with, in normalized units (fraction of the control's
/// range) plus raw values for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlDelta {
    pub canonical: String,
    pub predicted_raw: Value,
    pub corrected_raw: Value,
    /// corrected − predicted, normalized; `None` for non-numeric controls.
    pub delta: Option<f64>,
}

/// Compare the applied flat settings with the current flat settings for the
/// keys Mimic wrote. Returns `None` when nothing changed beyond read-back
/// tolerance; otherwise the canonical deltas and the mean absolute normalized
/// delta over changed numeric controls.
pub fn diff_applied(applied: &Map<String, Value>, current: &Map<String, Value>) -> Option<(Vec<ControlDelta>, f64)> {
    let written: Map<String, Value> =
        applied.iter().filter(|(k, _)| !k.starts_with("__")).map(|(k, v)| (k.clone(), v.clone())).collect();
    let verify = edit_dna::verify_readback(&written, current);
    if verify.ok {
        return None;
    }
    let mut deltas = Vec::new();
    let mut sum = 0.0;
    let mut n = 0usize;
    for m in &verify.mismatches {
        let Some(control) = MAPPING.control_for_key(&m.key) else { continue };
        let delta = match (edit_dna::parse_number(&m.intended), edit_dna::parse_number(&m.observed)) {
            (Some(a), Some(b)) => match (control.normalize_value(a), control.normalize_value(b)) {
                (Some(na), Some(nb)) => Some(nb - na),
                _ => None,
            },
            _ => None,
        };
        if let Some(d) = delta {
            sum += d.abs();
            n += 1;
        }
        deltas.push(ControlDelta {
            canonical: control.canonical.clone(),
            predicted_raw: m.intended.clone(),
            corrected_raw: m.observed.clone(),
            delta,
        });
    }
    if deltas.is_empty() {
        return None;
    }
    let magnitude = if n > 0 { sum / n as f64 } else { 0.0 };
    Some((deltas, magnitude))
}

/// Latest verified, non-restored apply per prediction in a session.
fn checkable_edits(db: &Db, session_id: &str) -> Result<Vec<AppliedEdit>, DbError> {
    let mut latest: HashMap<String, AppliedEdit> = HashMap::new();
    for batch in db.session_apply_batches(session_id)? {
        for e in db.applied_edits(&batch.id)? {
            latest.entry(e.prediction_id.clone()).or_insert(e);
        }
    }
    Ok(latest.into_values().filter(|e| e.result == "applied" && e.restore_result.is_none()).collect())
}

async fn sync_corrections(ctx: &JobContext, bridge: &BridgeHandle) -> Result<Value, JobError> {
    let session_id = ctx
        .job
        .payload
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| JobError::Failed("job payload missing sessionId".into()))?;
    ctx.db.get_session(&session_id)?.ok_or_else(|| JobError::Failed("session not found".into()))?;
    let conn = bridge.connection().ok_or(BridgeError::NotConnected)?;
    let edits = checkable_edits(&ctx.db, &session_id)?;
    if edits.is_empty() {
        return Err(JobError::Failed(
            "nothing to sync: no verified applies in this session (or they were restored)".into(),
        ));
    }
    for batch in ctx.db.session_apply_batches(&session_id)? {
        if let Some(fp) = &batch.lightroom_catalog_fingerprint {
            if fp != &conn.catalog_fingerprint {
                return Err(JobError::Failed(format!(
                    "session was applied to catalog {fp} but Lightroom has {} open",
                    conn.catalog_fingerprint
                )));
            }
        }
    }
    let mut assets = Vec::new();
    for e in &edits {
        if let Some(a) = ctx.db.get_asset(&e.asset_id)? {
            assets.push(a);
        }
    }
    ctx.progress(0, edits.len() as i64, "resolving photos");
    let photo_ids = resolve_photo_ids(bridge, &conn, &ctx.db, &assets).await?;
    let mut by_photo: HashMap<i64, &AppliedEdit> = HashMap::new();
    let mut unresolved = 0i64;
    for e in &edits {
        match photo_ids.get(&e.asset_id) {
            Some(id) => {
                by_photo.insert(*id, e);
            }
            None => unresolved += 1,
        }
    }
    let ids: Vec<i64> = by_photo.keys().copied().collect();
    let total = ids.len();
    let mut checked = 0i64;
    let mut untouched = 0i64;
    let mut corrected = 0i64;
    let mut kept = 0i64;
    let mut top: BTreeMap<String, (f64, usize)> = BTreeMap::new();
    for chunk in ids.chunks(APPLY_BATCH_SIZE) {
        ctx.check_cancel()?;
        let res =
            bridge.send_command(CommandType::CollectCorrectionState, json!({"photoIds": chunk}), READ_TIMEOUT).await?;
        let items = res.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
        for item in items {
            let Some(photo_id) = item.get("photoId").and_then(Value::as_i64) else { continue };
            let Some(edit) = by_photo.get(&photo_id) else { continue };
            let Some(current) = item.get("settings").and_then(Value::as_object) else {
                unresolved += 1;
                continue;
            };
            checked += 1;
            let applied = edit.applied_settings.as_object().cloned().unwrap_or_default();
            let Some((deltas, magnitude)) = diff_applied(&applied, current) else {
                untouched += 1;
                continue;
            };
            corrected += 1;
            for d in &deltas {
                if let Some(v) = d.delta {
                    let e = top.entry(d.canonical.clone()).or_insert((0.0, 0));
                    e.0 += v.abs();
                    e.1 += 1;
                }
            }
            let prediction = ctx.db.get_prediction(&edit.prediction_id)?;
            let Some(prediction) = prediction else { continue };
            let normalized = edit_dna::normalize(current);
            let corrected_global: BTreeMap<String, BTreeMap<String, ControlValue>> = normalized.global.clone();
            let stored = ctx.db.upsert_correction(
                &edit.asset_id,
                &edit.prediction_id,
                &prediction.model_version_id,
                &json!({"global": prediction.predicted_settings.get("global").cloned().unwrap_or(Value::Null), "lightroom": applied}),
                &json!({"global": corrected_global, "lightroom": current}),
                &json!(deltas),
                magnitude,
            )?;
            if stored.is_none() {
                kept += 1;
                continue;
            }
            ctx.db.insert_edit_snapshot(&NewEditSnapshot {
                asset_id: edit.asset_id.clone(),
                source: "correction".into(),
                process_version: normalized.lightroom.process_version.clone(),
                normalized_settings: serde_json::to_value(&normalized).map_err(|e| JobError::Failed(e.to_string()))?,
                raw_settings: Value::Object(current.clone()),
                unknown_settings: Value::Object(normalized.unknown.clone()),
                mapping_version: normalized.mapping_version.clone(),
                capability_schema_version: Some(conn.capabilities.schema_version.clone()),
                provenance: json!({"predictionId": edit.prediction_id, "appliedEditId": edit.id, "catalogFingerprint": conn.catalog_fingerprint, "collectedAt": res.get("collectedAt")}),
            })?;
        }
        ctx.progress(checked, total as i64, "comparing with Lightroom");
    }
    let sync = ctx.db.record_correction_sync(
        &session_id,
        Some(&conn.catalog_fingerprint),
        (checked, untouched, corrected, unresolved),
    )?;
    let mut most: Vec<(String, f64, usize)> = top.into_iter().map(|(k, (s, n))| (k, s / n as f64, n)).collect();
    most.sort_by(|a, b| b.1.total_cmp(&a.1));
    let summary = json!({
        "sessionId": session_id,
        "syncId": sync.id,
        "checked": checked,
        "untouched": untouched,
        "corrected": corrected,
        "keptEarlierCorrections": kept,
        "unresolved": unresolved,
        "noTouchRate": if checked > 0 { Some(untouched as f64 / checked as f64) } else { None },
        "mostCorrected": most.iter().take(5).map(|(c, m, n)| json!({"canonical": c, "meanAbsDelta": m, "count": n})).collect::<Vec<_>>(),
    });
    let _ = ctx.db.log_event(&NewEvent::info("corrections", "synced", summary.clone()).entity("session", &session_id));
    Ok(summary)
}

// ---------------------------------------------------------------------------
// Style health
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlInsight {
    pub canonical: String,
    pub corrections: usize,
    pub mean_abs_delta: f64,
    /// Mean signed delta; a consistent sign means the model is biased.
    pub mean_delta: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleHealth {
    pub style_id: String,
    pub no_touch: Vec<NoTouchStats>,
    /// Latest active-version No-Touch Rate, if any photo was ever checked.
    pub active_no_touch_rate: Option<f64>,
    pub corrections_total: usize,
    pub corrections_pending_training: usize,
    pub most_corrected: Vec<ControlInsight>,
    pub insights: Vec<String>,
}

/// Deterministic, data-backed insights. Every sentence is computed from rows,
/// never templated from assumptions.
pub fn style_health(db: &Db, style_id: &str) -> Result<StyleHealth, DbError> {
    let no_touch = db.no_touch_stats(style_id)?;
    let active = db.active_model_version(style_id)?;
    let active_no_touch_rate =
        active.as_ref().and_then(|a| no_touch.iter().find(|n| n.model_version_id == a.id)).and_then(|n| n.rate);
    let rows = db.corrections_for_style(style_id, 5000)?;
    let pending = db.pending_correction_asset_ids(style_id)?.len();
    let mut acc: BTreeMap<String, (f64, f64, usize)> = BTreeMap::new();
    for r in &rows {
        for d in r.correction.delta.as_array().into_iter().flatten() {
            let (Some(c), Some(v)) =
                (d.get("canonical").and_then(Value::as_str), d.get("delta").and_then(Value::as_f64))
            else {
                continue;
            };
            let e = acc.entry(c.to_string()).or_insert((0.0, 0.0, 0));
            e.0 += v.abs();
            e.1 += v;
            e.2 += 1;
        }
    }
    let mut most: Vec<ControlInsight> = acc
        .into_iter()
        .map(|(c, (a, s, n))| ControlInsight {
            canonical: c,
            corrections: n,
            mean_abs_delta: a / n as f64,
            mean_delta: s / n as f64,
        })
        .collect();
    most.sort_by(|a, b| {
        (b.mean_abs_delta * b.corrections as f64).total_cmp(&(a.mean_abs_delta * a.corrections as f64))
    });
    most.truncate(8);

    let mut insights = Vec::new();
    match active_no_touch_rate {
        Some(r) => {
            let n = no_touch
                .iter()
                .find(|n| Some(&n.model_version_id) == active.as_ref().map(|a| &a.id))
                .map(|n| n.applied_checked)
                .unwrap_or(0);
            insights.push(format!("No-Touch Rate {:.0}% over {n} applied photo(s) checked after sync.", r * 100.0));
        }
        None => insights
            .push("No-Touch Rate is not measurable yet: apply a session, then sync corrections from Lightroom.".into()),
    }
    if pending > 0 {
        insights.push(format!(
            "{pending} correction(s) have not been used by any training run; Train New Version will include them."
        ));
    }
    for c in most.iter().take(3) {
        if c.corrections >= 3 && c.mean_delta.abs() > 0.6 * c.mean_abs_delta {
            insights.push(format!(
                "{} is consistently corrected {} (mean {:.1}% of range across {} photos); the model is biased on it.",
                c.canonical,
                if c.mean_delta > 0.0 { "upward" } else { "downward" },
                c.mean_delta.abs() * 100.0,
                c.corrections
            ));
        }
    }
    if no_touch.len() >= 2 {
        let measured: Vec<&NoTouchStats> = no_touch.iter().filter(|n| n.rate.is_some()).collect();
        if measured.len() >= 2 {
            let (first, last) = (measured[0], measured[measured.len() - 1]);
            let (a, b) = (first.rate.unwrap_or(0.0), last.rate.unwrap_or(0.0));
            insights.push(format!(
                "No-Touch Rate went from {:.0}% (v{}) to {:.0}% (v{}).",
                a * 100.0,
                first.semantic_version,
                b * 100.0,
                last.semantic_version
            ));
        }
    }
    Ok(StyleHealth {
        style_id: style_id.to_string(),
        no_touch,
        active_no_touch_rate,
        corrections_total: rows.len(),
        corrections_pending_training: pending,
        most_corrected: most,
        insights,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    #[test]
    fn diff_ignores_tolerance_and_reports_normalized_deltas() {
        let applied =
            map(&[("Exposure2012", json!(0.35)), ("Contrast2012", json!(12)), ("__skippedControls", json!([]))]);
        let same = map(&[("Exposure2012", json!(0.352)), ("Contrast2012", json!(12)), ("Vibrance", json!(99))]);
        assert!(diff_applied(&applied, &same).is_none(), "within tolerance and untouched keys are not corrections");
        let changed = map(&[("Exposure2012", json!(0.85)), ("Contrast2012", json!(12))]);
        let (deltas, magnitude) = diff_applied(&applied, &changed).unwrap();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].canonical, "tone.exposure");
        let d = deltas[0].delta.unwrap();
        assert!((d - 0.05).abs() < 1e-6, "0.5 EV of a 10 EV range = 0.05, got {d}");
        assert!((magnitude - 0.05).abs() < 1e-6);
        let missing = map(&[("Contrast2012", json!(12))]);
        let (deltas, _) = diff_applied(&applied, &missing).unwrap();
        assert_eq!(deltas[0].corrected_raw, Value::Null);
        assert!(deltas[0].delta.is_none());
    }

    #[test]
    fn ui_fixtures_round_trip() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sessions");
        let health_json: Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("style_health.json")).unwrap()).unwrap();
        let health: StyleHealth = serde_json::from_value(health_json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&health).unwrap(), health_json);
        let row_json: Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("correction_row.json")).unwrap()).unwrap();
        let row: crate::db::CorrectionRow = serde_json::from_value(row_json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&row).unwrap(), row_json);
        let deltas: Vec<ControlDelta> = serde_json::from_value(row.correction.delta.clone()).unwrap();
        assert_eq!(deltas[0].canonical, "tone.exposure");
    }

    #[test]
    fn health_on_an_untrained_style_is_honest() {
        let db = Db::open_in_memory().unwrap();
        let style = db.create_style_profile("S", None).unwrap();
        let h = style_health(&db, &style.id).unwrap();
        assert!(h.no_touch.is_empty() && h.active_no_touch_rate.is_none());
        assert_eq!(h.corrections_total, 0);
        assert!(h.insights[0].contains("not measurable"));
    }
}
