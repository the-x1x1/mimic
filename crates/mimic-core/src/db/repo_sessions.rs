use rusqlite::{params, OptionalExtension, Row};
use serde_json::Value;

use super::models::{json_col, json_col_or_default};
use super::{
    AppliedEdit, ApplyBatch, Correction, CorrectionRow, CorrectionSync, Db, DbError, DbResult, NewPrediction,
    NoTouchStats, Prediction, SceneCluster, Session, SessionAsset,
};
use crate::ids::{new_id, now_rfc3339};

const SESSION_COLS: &str = "s.id, s.name, s.source_path, s.source_library_id, s.captured_start, s.captured_end, s.status, s.active_style_profile_id, s.created_at, (SELECT COUNT(*) FROM session_assets sa WHERE sa.session_id = s.id)";

fn map_session(r: &Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        name: r.get(1)?,
        source_path: r.get(2)?,
        source_library_id: r.get(3)?,
        captured_start: r.get(4)?,
        captured_end: r.get(5)?,
        status: r.get(6)?,
        active_style_profile_id: r.get(7)?,
        created_at: r.get(8)?,
        asset_count: r.get(9)?,
    })
}

const PRED_COLS: &str = "id, session_id, asset_id, model_version_id, predicted_settings_json, raw_model_output_json, confidence, confidence_components_json, nearest_examples_json, created_at, status, capability_schema_version, cluster_id";

fn map_pred(r: &Row<'_>) -> rusqlite::Result<Prediction> {
    Ok(Prediction {
        id: r.get(0)?,
        session_id: r.get(1)?,
        asset_id: r.get(2)?,
        model_version_id: r.get(3)?,
        predicted_settings: json_col_or_default(r.get(4)?),
        raw_model_output: json_col_or_default(r.get(5)?),
        confidence: r.get(6)?,
        confidence_components: json_col_or_default(r.get(7)?),
        nearest_examples: json_col_or_default(r.get(8)?),
        created_at: r.get(9)?,
        status: r.get(10)?,
        capability_schema_version: r.get(11)?,
        cluster_id: r.get(12)?,
    })
}

const BATCH_COLS: &str = "id, session_id, lightroom_catalog_fingerprint, started_at, completed_at, status, applied_count, failed_count, rollback_available, error_json";

fn map_batch(r: &Row<'_>) -> rusqlite::Result<ApplyBatch> {
    Ok(ApplyBatch {
        id: r.get(0)?,
        session_id: r.get(1)?,
        lightroom_catalog_fingerprint: r.get(2)?,
        started_at: r.get(3)?,
        completed_at: r.get(4)?,
        status: r.get(5)?,
        applied_count: r.get(6)?,
        failed_count: r.get(7)?,
        rollback_available: r.get::<_, i64>(8)? != 0,
        error: json_col(r.get(9)?),
    })
}

const AE_COLS: &str = "id, apply_batch_id, prediction_id, asset_id, before_settings_json, applied_settings_json, lightroom_snapshot_name, result, error_json, applied_at";
const AE_ALL_COLS: &str = "id, apply_batch_id, prediction_id, asset_id, before_settings_json, applied_settings_json, lightroom_snapshot_name, result, error_json, applied_at, restored_at, restore_result, restore_error_json";

fn map_ae(r: &Row<'_>) -> rusqlite::Result<AppliedEdit> {
    Ok(AppliedEdit {
        id: r.get(0)?,
        apply_batch_id: r.get(1)?,
        prediction_id: r.get(2)?,
        asset_id: r.get(3)?,
        before_settings: json_col(r.get(4)?),
        applied_settings: json_col_or_default(r.get(5)?),
        lightroom_snapshot_name: r.get(6)?,
        result: r.get(7)?,
        error: json_col(r.get(8)?),
        applied_at: r.get(9)?,
        restored_at: r.get(10)?,
        restore_result: r.get(11)?,
        restore_error: json_col(r.get(12)?),
    })
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAppliedEdit {
    pub apply_batch_id: String,
    pub prediction_id: String,
    pub asset_id: String,
    pub before_settings: Option<Value>,
    pub applied_settings: Value,
    pub lightroom_snapshot_name: Option<String>,
    pub result: String,
    pub error: Option<Value>,
}

impl Db {
    pub fn create_session(
        &self,
        name: &str,
        source_path: Option<&str>,
        source_library_id: Option<&str>,
        style_id: Option<&str>,
    ) -> DbResult<Session> {
        let id = new_id();
        self.conn().execute(
            "INSERT INTO sessions(id, name, source_path, source_library_id, status, active_style_profile_id, created_at)
             VALUES (?1,?2,?3,?4,'new',?5,?6)",
            params![id, name, source_path, source_library_id, style_id, now_rfc3339()],
        )?;
        self.get_session(&id)?.ok_or(DbError::NotFound(id))
    }

    pub fn get_session(&self, id: &str) -> DbResult<Option<Session>> {
        Ok(self
            .conn()
            .query_row(&format!("SELECT {SESSION_COLS} FROM sessions s WHERE s.id = ?1"), [id], map_session)
            .optional()?)
    }

    pub fn list_sessions(&self, limit: usize) -> DbResult<Vec<Session>> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare(&format!("SELECT {SESSION_COLS} FROM sessions s ORDER BY s.created_at DESC LIMIT {limit}"))?;
        let rows = stmt.query_map([], map_session)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// Delete a session, its memberships, clusters, predictions and apply
    /// records, plus the hidden library that backed it. Assets are kept
    /// (library_id → NULL) so applied-edit history is never silently lost.
    pub fn delete_session(&self, id: &str) -> DbResult<()> {
        let session = self.get_session(id)?.ok_or_else(|| DbError::NotFound(id.to_string()))?;
        self.transaction(|tx| {
            tx.execute("DELETE FROM sessions WHERE id = ?1", [id])?;
            tx.execute("DELETE FROM app_settings WHERE key = ?1", [format!("session.{id}.source")])?;
            if let Some(lib) = &session.source_library_id {
                tx.execute("DELETE FROM libraries WHERE id = ?1 AND purpose = 'session'", [lib])?;
            }
            Ok(())
        })
    }

    pub fn set_session_status(&self, id: &str, status: &str) -> DbResult<()> {
        self.conn().execute("UPDATE sessions SET status = ?2 WHERE id = ?1", params![id, status])?;
        Ok(())
    }

    pub fn set_session_style(&self, id: &str, style_id: Option<&str>) -> DbResult<()> {
        self.conn().execute("UPDATE sessions SET active_style_profile_id = ?2 WHERE id = ?1", params![id, style_id])?;
        Ok(())
    }

    pub fn set_session_capture_range(&self, id: &str, start: Option<&str>, end: Option<&str>) -> DbResult<()> {
        self.conn().execute(
            "UPDATE sessions SET captured_start = ?2, captured_end = ?3 WHERE id = ?1",
            params![id, start, end],
        )?;
        Ok(())
    }

    pub fn add_session_asset(&self, session_id: &str, asset_id: &str, sequence_index: i64) -> DbResult<()> {
        self.conn().execute(
            "INSERT INTO session_assets(session_id, asset_id, sequence_index) VALUES (?1, ?2, ?3)
             ON CONFLICT(session_id, asset_id) DO UPDATE SET sequence_index = excluded.sequence_index",
            params![session_id, asset_id, sequence_index],
        )?;
        Ok(())
    }

    pub fn session_assets(&self, session_id: &str) -> DbResult<Vec<SessionAsset>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT session_id, asset_id, sequence_index, cluster_id, burst_id FROM session_assets WHERE session_id = ?1 ORDER BY sequence_index",
        )?;
        let rows = stmt.query_map([session_id], |r| {
            Ok(SessionAsset {
                session_id: r.get(0)?,
                asset_id: r.get(1)?,
                sequence_index: r.get(2)?,
                cluster_id: r.get(3)?,
                burst_id: r.get(4)?,
            })
        })?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    pub fn assign_cluster(
        &self,
        session_id: &str,
        asset_id: &str,
        cluster_id: Option<&str>,
        burst_id: Option<&str>,
    ) -> DbResult<()> {
        self.conn().execute(
            "UPDATE session_assets SET cluster_id = ?3, burst_id = ?4 WHERE session_id = ?1 AND asset_id = ?2",
            params![session_id, asset_id, cluster_id, burst_id],
        )?;
        Ok(())
    }

    pub fn replace_scene_clusters(
        &self,
        session_id: &str,
        clusters: &[(String, String, Value)],
    ) -> DbResult<Vec<SceneCluster>> {
        self.transaction(|tx| {
            tx.execute("DELETE FROM scene_clusters WHERE session_id = ?1", [session_id])?;
            for (id, label, summary) in clusters {
                tx.execute(
                    "INSERT INTO scene_clusters(id, session_id, label, feature_summary_json, created_at) VALUES (?1,?2,?3,?4,?5)",
                    params![id, session_id, label, summary.to_string(), now_rfc3339()],
                )?;
            }
            Ok(())
        })?;
        self.scene_clusters(session_id)
    }

    pub fn scene_clusters(&self, session_id: &str) -> DbResult<Vec<SceneCluster>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT c.id, c.session_id, c.label, c.centroid_artifact_id, c.feature_summary_json, c.created_at,
                    (SELECT COUNT(*) FROM session_assets sa WHERE sa.cluster_id = c.id)
             FROM scene_clusters c WHERE c.session_id = ?1 ORDER BY c.label",
        )?;
        let rows = stmt.query_map([session_id], |r| {
            Ok(SceneCluster {
                id: r.get(0)?,
                session_id: r.get(1)?,
                label: r.get(2)?,
                centroid_artifact_id: r.get(3)?,
                feature_summary: json_col_or_default(r.get(4)?),
                created_at: r.get(5)?,
                asset_count: r.get(6)?,
            })
        })?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    pub fn rename_scene_cluster(&self, cluster_id: &str, label: &str) -> DbResult<()> {
        self.conn().execute("UPDATE scene_clusters SET label = ?2 WHERE id = ?1", params![cluster_id, label])?;
        Ok(())
    }

    // ----- predictions --------------------------------------------------

    /// Insert a prediction and mark any earlier pending prediction for the same
    /// (session, asset) as superseded.
    pub fn insert_prediction(&self, p: &NewPrediction) -> DbResult<Prediction> {
        let id = new_id();
        self.transaction(|tx| {
            tx.execute(
                "UPDATE predictions SET status = 'superseded' WHERE session_id = ?1 AND asset_id = ?2 AND status IN ('pending','reviewed')",
                params![p.session_id, p.asset_id],
            )?;
            tx.execute(
                &format!("INSERT INTO predictions({PRED_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'pending',?11,?12)"),
                params![
                    id, p.session_id, p.asset_id, p.model_version_id, p.predicted_settings.to_string(), p.raw_model_output.to_string(),
                    p.confidence, p.confidence_components.to_string(), p.nearest_examples.to_string(), now_rfc3339(),
                    p.capability_schema_version, p.cluster_id
                ],
            )?;
            Ok(())
        })?;
        self.get_prediction(&id)?.ok_or(DbError::NotFound(id))
    }

    pub fn get_prediction(&self, id: &str) -> DbResult<Option<Prediction>> {
        Ok(self
            .conn()
            .query_row(&format!("SELECT {PRED_COLS} FROM predictions WHERE id = ?1"), [id], map_pred)
            .optional()?)
    }

    pub fn session_predictions(&self, session_id: &str, include_superseded: bool) -> DbResult<Vec<Prediction>> {
        let conn = self.conn();
        let filter = if include_superseded { "" } else { "AND status != 'superseded'" };
        let mut stmt = conn.prepare(&format!(
            "SELECT {PRED_COLS} FROM predictions WHERE session_id = ?1 {filter} ORDER BY created_at"
        ))?;
        let rows = stmt.query_map([session_id], map_pred)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    pub fn set_prediction_status(&self, id: &str, status: &str) -> DbResult<()> {
        if !matches!(status, "pending" | "reviewed" | "applied" | "rejected" | "superseded") {
            return Err(DbError::Invalid(format!("bad prediction status {status}")));
        }
        let n = self.conn().execute("UPDATE predictions SET status = ?2 WHERE id = ?1", params![id, status])?;
        if n == 0 {
            return Err(DbError::NotFound(id.to_string()));
        }
        Ok(())
    }

    // ----- apply batches -----------------------------------------------

    pub fn create_apply_batch(&self, session_id: &str, catalog_fingerprint: Option<&str>) -> DbResult<ApplyBatch> {
        let id = new_id();
        self.conn().execute(
            "INSERT INTO apply_batches(id, session_id, lightroom_catalog_fingerprint, started_at, status) VALUES (?1,?2,?3,?4,'running')",
            params![id, session_id, catalog_fingerprint, now_rfc3339()],
        )?;
        self.get_apply_batch(&id)?.ok_or(DbError::NotFound(id))
    }

    pub fn get_apply_batch(&self, id: &str) -> DbResult<Option<ApplyBatch>> {
        Ok(self
            .conn()
            .query_row(&format!("SELECT {BATCH_COLS} FROM apply_batches WHERE id = ?1"), [id], map_batch)
            .optional()?)
    }

    pub fn session_apply_batches(&self, session_id: &str) -> DbResult<Vec<ApplyBatch>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {BATCH_COLS} FROM apply_batches WHERE session_id = ?1 ORDER BY started_at DESC"
        ))?;
        let rows = stmt.query_map([session_id], map_batch)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// Record one item result. Idempotent per (batch, prediction): a retry never
    /// double-counts. Counters are recomputed from rows.
    pub fn record_applied_edit(&self, e: &NewAppliedEdit) -> DbResult<AppliedEdit> {
        let id = new_id();
        self.transaction(|tx| {
            tx.execute(
                &format!(
                    "INSERT INTO applied_edits({AE_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
                     ON CONFLICT(apply_batch_id, prediction_id) DO UPDATE SET before_settings_json = excluded.before_settings_json,
                       applied_settings_json = excluded.applied_settings_json, lightroom_snapshot_name = excluded.lightroom_snapshot_name,
                       result = excluded.result, error_json = excluded.error_json, applied_at = excluded.applied_at"
                ),
                params![
                    id, e.apply_batch_id, e.prediction_id, e.asset_id, e.before_settings.as_ref().map(|v| v.to_string()),
                    e.applied_settings.to_string(), e.lightroom_snapshot_name, e.result, e.error.as_ref().map(|v| v.to_string()), now_rfc3339()
                ],
            )?;
            tx.execute(
                "UPDATE apply_batches SET
                   applied_count = (SELECT COUNT(*) FROM applied_edits WHERE apply_batch_id = ?1 AND result = 'applied'),
                   failed_count = (SELECT COUNT(*) FROM applied_edits WHERE apply_batch_id = ?1 AND result IN ('failed','verify_failed')),
                   rollback_available = (SELECT COUNT(*) > 0 FROM applied_edits WHERE apply_batch_id = ?1 AND before_settings_json IS NOT NULL)
                 WHERE id = ?1",
                [&e.apply_batch_id],
            )?;
            if e.result == "applied" {
                tx.execute("UPDATE predictions SET status = 'applied' WHERE id = ?1", [&e.prediction_id])?;
            }
            Ok(())
        })?;
        let conn = self.conn();
        Ok(conn.query_row(
            &format!("SELECT {AE_ALL_COLS} FROM applied_edits WHERE apply_batch_id = ?1 AND prediction_id = ?2"),
            params![e.apply_batch_id, e.prediction_id],
            map_ae,
        )?)
    }

    pub fn applied_edits(&self, batch_id: &str) -> DbResult<Vec<AppliedEdit>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {AE_ALL_COLS} FROM applied_edits WHERE apply_batch_id = ?1 ORDER BY applied_at"
        ))?;
        let rows = stmt.query_map([batch_id], map_ae)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// Record the outcome of restoring one applied edit (batch rollback).
    /// Idempotent per edit; the apply row itself is never rewritten.
    pub fn record_restore(&self, applied_edit_id: &str, result: &str, error: Option<&Value>) -> DbResult<AppliedEdit> {
        if !matches!(result, "restored" | "verify_failed" | "failed" | "skipped") {
            return Err(DbError::Invalid(format!("bad restore result {result}")));
        }
        let n = self.conn().execute(
            "UPDATE applied_edits SET restored_at = ?2, restore_result = ?3, restore_error_json = ?4 WHERE id = ?1",
            params![applied_edit_id, now_rfc3339(), result, error.map(|v| v.to_string())],
        )?;
        if n == 0 {
            return Err(DbError::NotFound(applied_edit_id.to_string()));
        }
        let conn = self.conn();
        Ok(conn.query_row(
            &format!("SELECT {AE_ALL_COLS} FROM applied_edits WHERE id = ?1"),
            [applied_edit_id],
            map_ae,
        )?)
    }

    /// Mark a batch's rollback availability after a restore pass. A batch can be
    /// restored once; items that failed to restore keep their apply row intact.
    pub fn set_batch_rollback_available(&self, batch_id: &str, available: bool) -> DbResult<()> {
        self.conn().execute(
            "UPDATE apply_batches SET rollback_available = ?2 WHERE id = ?1",
            params![batch_id, available as i64],
        )?;
        Ok(())
    }

    /// Predictions for a session joined with the asset summary the UI needs.
    pub fn session_predictions_with_assets(&self, session_id: &str) -> DbResult<Vec<(Prediction, super::Asset)>> {
        let preds = self.session_predictions(session_id, false)?;
        let mut out = Vec::with_capacity(preds.len());
        for p in preds {
            if let Some(a) = self.get_asset(&p.asset_id)? {
                out.push((p, a));
            }
        }
        Ok(out)
    }

    pub fn count_session_predictions_by_status(&self, session_id: &str) -> DbResult<Vec<(String, i64)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT status, COUNT(*) FROM predictions WHERE session_id = ?1 AND status != 'superseded' GROUP BY status",
        )?;
        let rows = stmt.query_map([session_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// Close a batch. Status is derived from rows: `completed` only when nothing failed.
    pub fn complete_apply_batch(&self, id: &str, canceled: bool, error: Option<&Value>) -> DbResult<ApplyBatch> {
        let batch = self.get_apply_batch(id)?.ok_or_else(|| DbError::NotFound(id.to_string()))?;
        let status = if canceled {
            "canceled"
        } else if error.is_some() {
            "failed"
        } else if batch.failed_count > 0 {
            "completed_with_failures"
        } else {
            "completed"
        };
        self.conn().execute(
            "UPDATE apply_batches SET status = ?2, completed_at = ?3, error_json = ?4 WHERE id = ?1",
            params![id, status, now_rfc3339(), error.map(|v| v.to_string())],
        )?;
        self.get_apply_batch(id)?.ok_or_else(|| DbError::NotFound(id.to_string()))
    }

    // ----- corrections --------------------------------------------------

    /// Replace the correction for a prediction unless an earlier one was
    /// already consumed by a training run (that one is immutable history).
    /// Returns `None` when the existing correction was kept.
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_correction(
        &self,
        asset_id: &str,
        prediction_id: &str,
        model_version_id: &str,
        predicted: &Value,
        corrected: &Value,
        delta: &Value,
        magnitude: f64,
    ) -> DbResult<Option<Correction>> {
        let existing: Option<(String, Option<String>)> = self
            .conn()
            .query_row(
                "SELECT id, included_in_training_version FROM corrections WHERE prediction_id = ?1",
                [prediction_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match existing {
            Some((_, Some(_))) => return Ok(None),
            Some((id, None)) => {
                self.conn().execute("DELETE FROM corrections WHERE id = ?1", [id])?;
            }
            None => {}
        }
        self.insert_correction(asset_id, prediction_id, model_version_id, predicted, corrected, delta, magnitude)
            .map(Some)
    }

    pub fn record_correction_sync(
        &self,
        session_id: &str,
        catalog_fingerprint: Option<&str>,
        counts: (i64, i64, i64, i64),
    ) -> DbResult<CorrectionSync> {
        let id = new_id();
        self.conn().execute(
            "INSERT INTO correction_syncs(id, session_id, lightroom_catalog_fingerprint, synced_at, checked_count, untouched_count, corrected_count, unresolved_count)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![id, session_id, catalog_fingerprint, now_rfc3339(), counts.0, counts.1, counts.2, counts.3],
        )?;
        let syncs = self.correction_syncs(session_id)?;
        syncs.into_iter().find(|s| s.id == id).ok_or(DbError::NotFound(id))
    }

    pub fn correction_syncs(&self, session_id: &str) -> DbResult<Vec<CorrectionSync>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, lightroom_catalog_fingerprint, synced_at, checked_count, untouched_count, corrected_count, unresolved_count
             FROM correction_syncs WHERE session_id = ?1 ORDER BY synced_at DESC",
        )?;
        let rows = stmt.query_map([session_id], |r| {
            Ok(CorrectionSync {
                id: r.get(0)?,
                session_id: r.get(1)?,
                lightroom_catalog_fingerprint: r.get(2)?,
                synced_at: r.get(3)?,
                checked_count: r.get(4)?,
                untouched_count: r.get(5)?,
                corrected_count: r.get(6)?,
                unresolved_count: r.get(7)?,
            })
        })?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// Corrections for every version of a Style, newest first, with the asset
    /// file name and version label the UI shows.
    pub fn corrections_for_style(&self, style_id: &str, limit: usize) -> DbResult<Vec<CorrectionRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT c.id, c.asset_id, c.prediction_id, c.model_version_id, c.predicted_settings_json, c.corrected_settings_json,
                    c.delta_json, c.correction_magnitude, c.observed_at, c.included_in_training_version,
                    p.session_id, a.file_name, mv.semantic_version
             FROM corrections c
             JOIN model_versions mv ON mv.id = c.model_version_id
             JOIN predictions p ON p.id = c.prediction_id
             JOIN assets a ON a.id = c.asset_id
             WHERE mv.style_profile_id = ?1 ORDER BY c.observed_at DESC LIMIT {limit}"
        ))?;
        let rows = stmt.query_map([style_id], |r| {
            Ok(CorrectionRow {
                correction: Correction {
                    id: r.get(0)?,
                    asset_id: r.get(1)?,
                    prediction_id: r.get(2)?,
                    model_version_id: r.get(3)?,
                    predicted_settings: json_col_or_default(r.get(4)?),
                    corrected_settings: json_col_or_default(r.get(5)?),
                    delta: json_col_or_default(r.get(6)?),
                    correction_magnitude: r.get(7)?,
                    observed_at: r.get(8)?,
                    included_in_training_version: r.get(9)?,
                },
                session_id: r.get(10)?,
                file_name: r.get(11)?,
                semantic_version: r.get(12)?,
            })
        })?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// Asset ids of corrections for a Style that no training run has used yet.
    pub fn pending_correction_asset_ids(&self, style_id: &str) -> DbResult<Vec<(String, String)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT c.id, c.asset_id FROM corrections c JOIN model_versions mv ON mv.id = c.model_version_id
             WHERE mv.style_profile_id = ?1 AND c.included_in_training_version IS NULL ORDER BY c.observed_at",
        )?;
        let rows = stmt.query_map([style_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    pub fn mark_corrections_included(&self, correction_ids: &[String], semantic_version: &str) -> DbResult<usize> {
        let mut n = 0;
        self.transaction(|tx| {
            for id in correction_ids {
                n += tx.execute(
                    "UPDATE corrections SET included_in_training_version = ?2 WHERE id = ?1 AND included_in_training_version IS NULL",
                    params![id, semantic_version],
                )?;
            }
            Ok(())
        })?;
        Ok(n)
    }

    /// No-Touch statistics per model version of a Style. Only applied, verified,
    /// non-restored photos in sessions that have been synced count.
    pub fn no_touch_stats(&self, style_id: &str) -> DbResult<Vec<NoTouchStats>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT mv.id, mv.semantic_version,
                    (SELECT COUNT(*) FROM applied_edits ae JOIN predictions p ON p.id = ae.prediction_id
                      WHERE p.model_version_id = mv.id AND ae.result = 'applied' AND ae.restore_result IS NULL
                        AND p.session_id IN (SELECT session_id FROM correction_syncs)),
                    (SELECT COUNT(*) FROM corrections c JOIN applied_edits ae ON ae.prediction_id = c.prediction_id
                      JOIN predictions p ON p.id = c.prediction_id
                      WHERE c.model_version_id = mv.id AND ae.result = 'applied' AND ae.restore_result IS NULL
                        AND p.session_id IN (SELECT session_id FROM correction_syncs))
             FROM model_versions mv WHERE mv.style_profile_id = ?1 ORDER BY mv.created_at",
        )?;
        let rows = stmt.query_map([style_id], |r| {
            let checked: i64 = r.get(2)?;
            let corrected: i64 = r.get(3)?;
            let corrected = corrected.min(checked);
            Ok(NoTouchStats {
                model_version_id: r.get(0)?,
                semantic_version: r.get(1)?,
                applied_checked: checked,
                corrected,
                untouched: checked - corrected,
                rate: if checked > 0 { Some((checked - corrected) as f64 / checked as f64) } else { None },
            })
        })?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_correction(
        &self,
        asset_id: &str,
        prediction_id: &str,
        model_version_id: &str,
        predicted: &Value,
        corrected: &Value,
        delta: &Value,
        magnitude: f64,
    ) -> DbResult<Correction> {
        let id = new_id();
        self.conn().execute(
            "INSERT INTO corrections(id, asset_id, prediction_id, model_version_id, predicted_settings_json, corrected_settings_json,
               delta_json, correction_magnitude, observed_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![id, asset_id, prediction_id, model_version_id, predicted.to_string(), corrected.to_string(), delta.to_string(), magnitude, now_rfc3339()],
        )?;
        let conn = self.conn();
        Ok(conn.query_row(
            "SELECT id, asset_id, prediction_id, model_version_id, predicted_settings_json, corrected_settings_json, delta_json,
                    correction_magnitude, observed_at, included_in_training_version FROM corrections WHERE id = ?1",
            [&id],
            |r| {
                Ok(Correction {
                    id: r.get(0)?,
                    asset_id: r.get(1)?,
                    prediction_id: r.get(2)?,
                    model_version_id: r.get(3)?,
                    predicted_settings: json_col_or_default(r.get(4)?),
                    corrected_settings: json_col_or_default(r.get(5)?),
                    delta: json_col_or_default(r.get(6)?),
                    correction_magnitude: r.get(7)?,
                    observed_at: r.get(8)?,
                    included_in_training_version: r.get(9)?,
                })
            },
        )?)
    }

    pub fn corrections_for_model(&self, model_version_id: &str) -> DbResult<Vec<Correction>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, asset_id, prediction_id, model_version_id, predicted_settings_json, corrected_settings_json, delta_json,
                    correction_magnitude, observed_at, included_in_training_version FROM corrections WHERE model_version_id = ?1 ORDER BY observed_at",
        )?;
        let rows = stmt.query_map([model_version_id], |r| {
            Ok(Correction {
                id: r.get(0)?,
                asset_id: r.get(1)?,
                prediction_id: r.get(2)?,
                model_version_id: r.get(3)?,
                predicted_settings: json_col_or_default(r.get(4)?),
                corrected_settings: json_col_or_default(r.get(5)?),
                delta: json_col_or_default(r.get(6)?),
                correction_magnitude: r.get(7)?,
                observed_at: r.get(8)?,
                included_in_training_version: r.get(9)?,
            })
        })?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::repo_styles::NewModelVersion;
    use crate::db::NewAsset;
    use serde_json::json;

    fn seed(db: &Db) -> (Session, String, String) {
        let style = db.create_style_profile("S", None).unwrap();
        let mv = db
            .create_model_version(&NewModelVersion {
                style_profile_id: style.id.clone(),
                semantic_version: "1.0.0".into(),
                model_type: "hybrid".into(),
                feature_schema_version: "features_v1".into(),
                edit_schema_version: "1.0".into(),
                training_set_id: None,
                training_config: json!({}),
                metrics: json!({}),
                artifact_manifest: json!({}),
                status: "training".into(),
            })
            .unwrap();
        let (asset, _) = db
            .upsert_asset(&NewAsset {
                source_path: "/p/a.cr3".into(),
                file_name: "a.cr3".into(),
                extension: "cr3".into(),
                fast_hash: "h".into(),
                ..Default::default()
            })
            .unwrap();
        let session = db.create_session("Wedding", Some("/p"), None, Some(&style.id)).unwrap();
        db.add_session_asset(&session.id, &asset.id, 0).unwrap();
        (session, asset.id, mv.id)
    }

    #[test]
    fn prediction_supersede_and_apply_batch_accounting() {
        let db = Db::open_in_memory().unwrap();
        let (session, asset_id, mv_id) = seed(&db);
        let p1 = db
            .insert_prediction(&NewPrediction {
                session_id: session.id.clone(),
                asset_id: asset_id.clone(),
                model_version_id: mv_id.clone(),
                predicted_settings: json!({"tone": {"exposure": {"raw": 0.3}}}),
                raw_model_output: json!({}),
                confidence: 0.9,
                confidence_components: json!({"knnDistance": 0.1}),
                nearest_examples: json!([]),
                capability_schema_version: None,
                cluster_id: None,
            })
            .unwrap();
        let p2 = db
            .insert_prediction(&NewPrediction {
                session_id: session.id.clone(),
                asset_id: asset_id.clone(),
                model_version_id: mv_id.clone(),
                predicted_settings: json!({}),
                raw_model_output: json!({}),
                confidence: 0.5,
                confidence_components: json!({}),
                nearest_examples: json!([]),
                capability_schema_version: None,
                cluster_id: None,
            })
            .unwrap();
        assert_eq!(db.get_prediction(&p1.id).unwrap().unwrap().status, "superseded");
        assert_eq!(db.session_predictions(&session.id, false).unwrap().len(), 1);
        assert_eq!(db.session_predictions(&session.id, true).unwrap().len(), 2);

        let batch = db.create_apply_batch(&session.id, Some("cat")).unwrap();
        db.record_applied_edit(&NewAppliedEdit {
            apply_batch_id: batch.id.clone(),
            prediction_id: p2.id.clone(),
            asset_id: asset_id.clone(),
            before_settings: Some(json!({"Exposure2012": 0})),
            applied_settings: json!({"Exposure2012": 0.3}),
            lightroom_snapshot_name: Some("Mimic Before — t".into()),
            result: "verify_failed".into(),
            error: Some(json!({"code": "readback_mismatch"})),
        })
        .unwrap();
        // Retry same item: must not double count.
        db.record_applied_edit(&NewAppliedEdit {
            apply_batch_id: batch.id.clone(),
            prediction_id: p2.id.clone(),
            asset_id: asset_id.clone(),
            before_settings: Some(json!({"Exposure2012": 0})),
            applied_settings: json!({"Exposure2012": 0.3}),
            lightroom_snapshot_name: Some("Mimic Before — t".into()),
            result: "applied".into(),
            error: None,
        })
        .unwrap();
        let b = db.get_apply_batch(&batch.id).unwrap().unwrap();
        assert_eq!((b.applied_count, b.failed_count), (1, 0));
        assert!(b.rollback_available);
        assert_eq!(db.get_prediction(&p2.id).unwrap().unwrap().status, "applied");
        let done = db.complete_apply_batch(&batch.id, false, None).unwrap();
        assert_eq!(done.status, "completed");
        let edits = db.applied_edits(&batch.id).unwrap();
        assert_eq!(edits.len(), 1);
        assert!(edits[0].restore_result.is_none());
        let restored = db.record_restore(&edits[0].id, "restored", None).unwrap();
        assert_eq!(restored.restore_result.as_deref(), Some("restored"));
        assert!(restored.restored_at.is_some() && restored.result == "applied", "apply row untouched");
        assert!(db.record_restore(&edits[0].id, "bogus", None).is_err());
        db.set_batch_rollback_available(&batch.id, false).unwrap();
        assert!(!db.get_apply_batch(&batch.id).unwrap().unwrap().rollback_available);
        assert_eq!(db.count_session_predictions_by_status(&session.id).unwrap(), vec![("applied".to_string(), 1)]);

        let c = db
            .insert_correction(
                &asset_id,
                &p2.id,
                &mv_id,
                &json!({"e": 0.3}),
                &json!({"e": 0.5}),
                &json!({"e": 0.2}),
                0.2,
            )
            .unwrap();
        assert_eq!(c.correction_magnitude, 0.2);
        assert_eq!(db.corrections_for_model(&mv_id).unwrap().len(), 1);
        // Re-sync replaces an unused correction; a used one is kept.
        let again = db
            .upsert_correction(&asset_id, &p2.id, &mv_id, &json!({}), &json!({}), &json!({"e": 0.1}), 0.1)
            .unwrap()
            .unwrap();
        assert_ne!(again.id, c.id);
        assert_eq!(db.corrections_for_model(&mv_id).unwrap().len(), 1);
        assert_eq!(db.mark_corrections_included(std::slice::from_ref(&again.id), "1.1.0").unwrap(), 1);
        assert!(db
            .upsert_correction(&asset_id, &p2.id, &mv_id, &json!({}), &json!({}), &json!({}), 0.5)
            .unwrap()
            .is_none());
        let style_id = db.get_model_version(&mv_id).unwrap().unwrap().style_profile_id;
        let rows = db.corrections_for_style(&style_id, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].file_name, "a.cr3");
        assert_eq!(rows[0].correction.included_in_training_version.as_deref(), Some("1.1.0"));
        assert!(db.pending_correction_asset_ids(&style_id).unwrap().is_empty());
        // No-Touch: nothing counts until a sync is recorded, and restored edits never count.
        assert_eq!(db.no_touch_stats(&style_id).unwrap()[0].rate, None);
        db.record_correction_sync(&session.id, Some("cat"), (0, 0, 0, 0)).unwrap();
        assert_eq!(db.no_touch_stats(&style_id).unwrap()[0].applied_checked, 0, "p2's edit was restored");
        let p3 = db
            .insert_prediction(&NewPrediction {
                session_id: session.id.clone(),
                asset_id: asset_id.clone(),
                model_version_id: mv_id.clone(),
                predicted_settings: json!({}),
                raw_model_output: json!({}),
                confidence: 0.7,
                confidence_components: json!({}),
                nearest_examples: json!([]),
                capability_schema_version: None,
                cluster_id: None,
            })
            .unwrap();
        let b2 = db.create_apply_batch(&session.id, Some("cat")).unwrap();
        db.record_applied_edit(&NewAppliedEdit {
            apply_batch_id: b2.id.clone(),
            prediction_id: p3.id.clone(),
            asset_id: asset_id.clone(),
            before_settings: Some(json!({"Exposure2012": 0})),
            applied_settings: json!({"Exposure2012": 0.2}),
            lightroom_snapshot_name: None,
            result: "applied".into(),
            error: None,
        })
        .unwrap();
        let nt = &db.no_touch_stats(&style_id).unwrap()[0];
        assert_eq!((nt.applied_checked, nt.corrected, nt.untouched, nt.rate), (1, 0, 1, Some(1.0)));
        db.upsert_correction(&asset_id, &p3.id, &mv_id, &json!({}), &json!({}), &json!({}), 0.3).unwrap();
        let nt = &db.no_touch_stats(&style_id).unwrap()[0];
        assert_eq!((nt.corrected, nt.untouched, nt.rate), (1, 0, Some(0.0)));
        assert_eq!(db.correction_syncs(&session.id).unwrap().len(), 1);
    }

    #[test]
    fn batch_with_failure_is_never_completed_clean() {
        let db = Db::open_in_memory().unwrap();
        let (session, asset_id, mv_id) = seed(&db);
        let asset_id_copy = asset_id.clone();
        let p = db
            .insert_prediction(&NewPrediction {
                session_id: session.id.clone(),
                asset_id: asset_id.clone(),
                model_version_id: mv_id.clone(),
                predicted_settings: json!({}),
                raw_model_output: json!({}),
                confidence: 0.9,
                confidence_components: json!({}),
                nearest_examples: json!([]),
                capability_schema_version: None,
                cluster_id: None,
            })
            .unwrap();
        let batch = db.create_apply_batch(&session.id, None).unwrap();
        db.record_applied_edit(&NewAppliedEdit {
            apply_batch_id: batch.id.clone(),
            prediction_id: p.id.clone(),
            asset_id,
            before_settings: None,
            applied_settings: json!({}),
            lightroom_snapshot_name: None,
            result: "failed".into(),
            error: Some(json!({"code": "sdk_error"})),
        })
        .unwrap();
        let done = db.complete_apply_batch(&batch.id, false, None).unwrap();
        assert_eq!(done.status, "completed_with_failures");
        assert!(!done.rollback_available);
        let clusters = db.replace_scene_clusters(&session.id, &[("c1".into(), "Group 1".into(), json!({}))]).unwrap();
        assert_eq!(clusters.len(), 1);
        assert_eq!(db.list_sessions(10).unwrap()[0].asset_count, 1);
        let style_id = db.get_model_version(&mv_id).unwrap().unwrap().style_profile_id;
        assert!(
            matches!(db.delete_style_profile(&style_id), Err(DbError::Invalid(_))),
            "a Style with predictions cannot be deleted"
        );
        db.delete_session(&session.id).unwrap();
        db.delete_style_profile(&style_id).unwrap();
        assert!(db.get_session(&session.id).unwrap().is_none());
        assert!(db.get_prediction(&p.id).unwrap().is_none(), "predictions cascade");
        assert!(db.get_asset(&asset_id_copy).unwrap().is_some(), "assets survive");
        assert!(matches!(db.delete_session(&session.id), Err(DbError::NotFound(_))));
    }
}
