//! Style Brain training job (spec §11, §13, §14.5).
//!
//! The engine trains; this module owns the lifecycle: an immutable
//! `model_versions` row is created in `training` state, finalized exactly once
//! with metrics + artifact manifest, and activated only if there is no active
//! version or the new holdout error is not worse than the active one's.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::db::{Db, NewEvent, NewModelVersion};
use crate::engine::{EngineClient, EngineError};
use crate::ingest::MIN_PAIRS_TO_TRAIN;
use crate::jobs::{forward_engine_progress, JobContext, JobError, JobExecutor, JobFuture};

pub const JOB_TRAIN_STYLE: &str = "train_style";

pub struct TrainingExecutor {
    pub engine: EngineClient,
}

impl JobExecutor for TrainingExecutor {
    fn kinds(&self) -> &'static [&'static str] {
        &[JOB_TRAIN_STYLE]
    }
    fn resumable(&self, _kind: &str) -> bool {
        false
    }
    fn execute(&self, ctx: JobContext) -> JobFuture {
        let engine = self.engine.clone();
        Box::pin(async move { train_style(&ctx, &engine).await })
    }
}

pub fn executor(engine: EngineClient) -> Arc<dyn JobExecutor> {
    Arc::new(TrainingExecutor { engine })
}

/// Holdout (or validation) hybrid nMAE from a metrics document, if present.
pub fn primary_error(metrics: &Value) -> Option<f64> {
    for set in ["holdout", "validation"] {
        let n = metrics.get(set).and_then(|s| s.get("n")).and_then(Value::as_i64).unwrap_or(0);
        if n > 0 {
            if let Some(v) = metrics[set]["hybrid"]["overall"]["nMae"].as_f64() {
                return Some(v);
            }
        }
    }
    None
}

async fn train_style(ctx: &JobContext, engine: &EngineClient) -> Result<Value, JobError> {
    let style_id = ctx
        .job
        .payload
        .get("styleId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| JobError::Failed("job payload missing styleId".into()))?;
    let mut config = ctx.job.payload.get("config").cloned().unwrap_or_else(|| json!({}));
    let style = ctx.db.get_style_profile(&style_id)?.ok_or_else(|| JobError::Failed("style not found".into()))?;
    // Corrections the photographer made after earlier applies join the
    // training set as pairs (their latest snapshot is `source = correction`).
    let include_corrections = config.get("includeCorrections").and_then(Value::as_bool).unwrap_or(true);
    let pending_corrections =
        if include_corrections { ctx.db.pending_correction_asset_ids(&style_id)? } else { Vec::new() };
    if !pending_corrections.is_empty() {
        config["correctionAssetIds"] = json!(pending_corrections.iter().map(|(_, a)| a.clone()).collect::<Vec<_>>());
    }
    if style.library_ids.is_empty() {
        return Err(JobError::Failed("this Style has no training data attached".into()));
    }
    let mut pairs = 0i64;
    for lib in &style.library_ids {
        pairs += ctx.db.count_assets_with_edits(lib)?;
    }
    if pairs < MIN_PAIRS_TO_TRAIN {
        return Err(JobError::Failed(format!(
            "{pairs} edited examples; at least {MIN_PAIRS_TO_TRAIN} are needed to train"
        )));
    }
    let semver = ctx.db.next_model_semver(&style_id)?;
    let mv = ctx.db.create_model_version(&NewModelVersion {
        style_profile_id: style_id.clone(),
        semantic_version: semver.clone(),
        model_type: "hybrid_knn_residual".into(),
        feature_schema_version: crate::ingest::FEATURE_VERSION.into(),
        edit_schema_version: crate::edit_dna::SCHEMA_VERSION.to_string(),
        training_set_id: None,
        training_config: config.clone(),
        metrics: json!({}),
        artifact_manifest: json!({}),
        status: "training".into(),
    })?;
    ctx.db.set_style_status(&style_id, "training")?;
    ctx.progress(0, 0, "starting training");
    let _forwarder = forward_engine_progress(engine, ctx);
    let params = json!({
        "styleId": style_id,
        "modelVersionId": mv.id,
        "libraryIds": style.library_ids,
        "config": config,
        "jobId": ctx.job.id,
    });
    let result = engine.call_with_timeout("training.train", params, Duration::from_secs(6 * 3600)).await;
    let result = match result {
        Ok(r) => r,
        Err(e) => {
            let code = match &e {
                EngineError::Remote { code, .. } => code.clone(),
                _ => "engine".into(),
            };
            let details = match &e {
                EngineError::Remote { details, .. } => details.clone().unwrap_or(Value::Null),
                _ => Value::Null,
            };
            let _ = ctx.db.finalize_model_version(
                &mv.id,
                "failed",
                &json!({"error": {"code": code, "message": e.to_string(), "details": details}}),
                &json!({}),
            );
            let _ = ctx.db.set_style_status(
                &style_id,
                if ctx.db.active_model_version(&style_id)?.is_some() { "ready" } else { "empty" },
            );
            return Err(JobError::Engine(e));
        }
    };

    let metrics = result.get("metrics").cloned().unwrap_or(json!({}));
    let training_config = result.get("trainingConfig").cloned().unwrap_or(json!({}));
    let manifest = json!({
        "artifactDir": result.get("artifactDir"),
        "artifacts": result.get("artifacts"),
        "beatsBaselines": result.get("beatsBaselines"),
        "split": result.get("split"),
    });
    let counts = result.get("counts").cloned().unwrap_or(json!({}));
    let used_corrections: Vec<String> = result["trainingSet"]["correctionAssetIds"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let ts = ctx.db.create_training_set(
        &style_id,
        &json!({"libraryIds": style.library_ids, "correctionAssetIds": used_corrections}),
        (
            result["trainingSet"]["assetCount"].as_i64().unwrap_or(0),
            result["trainingSet"]["validPairCount"].as_i64().unwrap_or(0),
            counts["train"].as_i64().unwrap_or(0),
            counts["validation"].as_i64().unwrap_or(0),
            counts["holdout"].as_i64().unwrap_or(0),
        ),
        result["split"]["strategy"].as_str().unwrap_or("unknown"),
        result["trainingSet"]["fingerprint"].as_str(),
    )?;
    ctx.db.set_model_version_training_set(&mv.id, &ts.id)?;
    // Store the training config the engine actually used (seed, versions, fingerprint).
    ctx.db.set_model_version_training_config(
        &mv.id,
        &training_config,
        result["modelType"].as_str().unwrap_or("hybrid_knn_residual"),
    )?;
    for a in result.get("artifacts").and_then(Value::as_array).cloned().unwrap_or_default() {
        ctx.db.add_model_artifact(
            &mv.id,
            a["kind"].as_str().unwrap_or("artifact"),
            a["path"].as_str().unwrap_or(""),
            a["sha256"].as_str().unwrap_or(""),
            a["sizeBytes"].as_i64().unwrap_or(0),
            a["format"].as_str().unwrap_or("bin"),
        )?;
    }
    let finalized = ctx.db.finalize_model_version(&mv.id, "ready", &metrics, &manifest)?;
    let correction_ids: Vec<String> = pending_corrections
        .iter()
        .filter(|(_, asset)| used_corrections.contains(asset))
        .map(|(id, _)| id.clone())
        .collect();
    let corrections_included = ctx.db.mark_corrections_included(&correction_ids, &semver)?;

    // Activation policy (§14.5).
    let new_err = primary_error(&metrics);
    let active = ctx.db.active_model_version(&style_id)?;
    let (activated, reason) = match (&active, new_err) {
        (None, _) => (true, "first trained version".to_string()),
        (Some(a), Some(n)) => match primary_error(&a.metrics) {
            Some(cur) if n <= cur => (true, format!("holdout nMAE {n:.4} ≤ active v{} ({cur:.4})", a.semantic_version)),
            Some(cur) => (
                false,
                format!(
                    "holdout nMAE {n:.4} is worse than active v{} ({cur:.4}); activate manually if you prefer it",
                    a.semantic_version
                ),
            ),
            None => (true, "active version has no comparable metrics".to_string()),
        },
        (Some(_), None) => (false, "new version has no evaluable holdout; kept inactive".to_string()),
    };
    if activated {
        ctx.db.activate_model_version(&mv.id)?;
    } else {
        ctx.db.set_style_status(&style_id, "ready")?;
    }
    let summary = json!({
        "modelVersionId": finalized.id,
        "semanticVersion": semver,
        "activated": activated,
        "activationReason": reason,
        "counts": counts,
        "holdoutNmae": new_err,
        "correctionsIncluded": corrections_included,
        "beatsBaselines": result.get("beatsBaselines"),
        "warnings": training_config.get("warnings"),
    });
    let _ = ctx
        .db
        .log_event(&NewEvent::info("training", "completed", summary.clone()).entity("model_version", &finalized.id));
    Ok(summary)
}

/// Activate a ready version by hand (rollback / override of the policy).
pub fn activate(db: &Db, model_version_id: &str) -> Result<crate::db::ModelVersion, crate::db::DbError> {
    let mv = db.activate_model_version(model_version_id)?;
    let _ = db.log_event(
        &NewEvent::info("training", "activated", json!({"semanticVersion": mv.semantic_version}))
            .entity("model_version", model_version_id),
    );
    Ok(mv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_error_prefers_holdout() {
        let m = json!({"holdout": {"n": 10, "hybrid": {"overall": {"nMae": 0.05}}}, "validation": {"n": 5, "hybrid": {"overall": {"nMae": 0.09}}}});
        assert_eq!(primary_error(&m), Some(0.05));
        let v = json!({"holdout": {"n": 0}, "validation": {"n": 5, "hybrid": {"overall": {"nMae": 0.09}}}});
        assert_eq!(primary_error(&v), Some(0.09));
        assert_eq!(primary_error(&json!({})), None);
    }
}
