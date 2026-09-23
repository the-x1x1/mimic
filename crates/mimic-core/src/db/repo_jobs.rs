use rusqlite::{params, OptionalExtension, Row};
use serde_json::Value;

use super::models::{json_col, json_col_or_default};
use super::{Db, DbError, DbResult, Job};
use crate::ids::{new_id, now_rfc3339};

const COLS: &str = "id, type, status, payload_json, progress_current, progress_total, phase, resumable, created_at, started_at, heartbeat_at, completed_at, result_json, error_json";

fn map(r: &Row<'_>) -> rusqlite::Result<Job> {
    Ok(Job {
        id: r.get(0)?,
        kind: r.get(1)?,
        status: r.get(2)?,
        payload: json_col_or_default(r.get(3)?),
        progress_current: r.get(4)?,
        progress_total: r.get(5)?,
        phase: r.get(6)?,
        resumable: r.get::<_, i64>(7)? != 0,
        created_at: r.get(8)?,
        started_at: r.get(9)?,
        heartbeat_at: r.get(10)?,
        completed_at: r.get(11)?,
        result: json_col(r.get(12)?),
        error: json_col(r.get(13)?),
    })
}

impl Db {
    pub fn create_job(&self, kind: &str, payload: &Value, resumable: bool) -> DbResult<Job> {
        let id = new_id();
        self.conn().execute(
            "INSERT INTO jobs(id, type, status, payload_json, resumable, created_at) VALUES (?1, ?2, 'queued', ?3, ?4, ?5)",
            params![id, kind, payload.to_string(), resumable as i64, now_rfc3339()],
        )?;
        self.get_job(&id)?.ok_or(DbError::NotFound(id))
    }

    pub fn get_job(&self, id: &str) -> DbResult<Option<Job>> {
        Ok(self.conn().query_row(&format!("SELECT {COLS} FROM jobs WHERE id = ?1"), [id], map).optional()?)
    }

    pub fn list_jobs(&self, limit: usize, active_only: bool) -> DbResult<Vec<Job>> {
        let conn = self.conn();
        let filter = if active_only { "WHERE status IN ('queued','running','paused')" } else { "" };
        let mut stmt =
            conn.prepare(&format!("SELECT {COLS} FROM jobs {filter} ORDER BY created_at DESC LIMIT {limit}"))?;
        let rows = stmt.query_map([], map)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    pub fn next_queued_job(&self) -> DbResult<Option<Job>> {
        Ok(self
            .conn()
            .query_row(&format!("SELECT {COLS} FROM jobs WHERE status = 'queued' ORDER BY created_at LIMIT 1"), [], map)
            .optional()?)
    }

    /// Start a queued job. False when it is not queued any more — canceled
    /// between the runner picking it and starting it — in which case it is
    /// left as it is and not run: the cancel is not undone.
    pub fn mark_job_running(&self, id: &str) -> DbResult<bool> {
        let now = now_rfc3339();
        let started = self.conn().execute(
            "UPDATE jobs SET status = 'running', started_at = COALESCE(started_at, ?2), heartbeat_at = ?2
             WHERE id = ?1 AND status = 'queued'",
            params![id, now],
        )?;
        if started == 0 && self.get_job(id)?.is_none() {
            return Err(DbError::NotFound(id.to_string()));
        }
        Ok(started == 1)
    }

    pub fn update_job_progress(&self, id: &str, current: i64, total: i64, phase: Option<&str>) -> DbResult<()> {
        self.expect_row(self.conn().execute(
            "UPDATE jobs SET progress_current = ?2, progress_total = ?3, phase = COALESCE(?4, phase), heartbeat_at = ?5 WHERE id = ?1",
            params![id, current, total, phase, now_rfc3339()],
        )?, id)
    }

    pub fn heartbeat_job(&self, id: &str) -> DbResult<()> {
        self.expect_row(
            self.conn().execute("UPDATE jobs SET heartbeat_at = ?2 WHERE id = ?1", params![id, now_rfc3339()])?,
            id,
        )
    }

    pub fn complete_job(&self, id: &str, result: &Value) -> DbResult<()> {
        self.expect_row(
            self.conn().execute(
                "UPDATE jobs SET status = 'completed', completed_at = ?2, result_json = ?3 WHERE id = ?1",
                params![id, now_rfc3339(), result.to_string()],
            )?,
            id,
        )
    }

    pub fn fail_job(&self, id: &str, error: &Value) -> DbResult<()> {
        self.expect_row(
            self.conn().execute(
                "UPDATE jobs SET status = 'failed', completed_at = ?2, error_json = ?3 WHERE id = ?1",
                params![id, now_rfc3339(), error.to_string()],
            )?,
            id,
        )
    }

    /// Request cancellation. Queued jobs are canceled immediately; running jobs
    /// are flagged and the worker observes `job_cancel_requested`. A job that
    /// starts between the two is caught as running, not relabelled canceled
    /// while it still runs.
    pub fn cancel_job(&self, id: &str) -> DbResult<Job> {
        let job = self.get_job(id)?.ok_or_else(|| DbError::NotFound(id.to_string()))?;
        if matches!(job.status.as_str(), "queued" | "paused") {
            let canceled = self.conn().execute(
                "UPDATE jobs SET status = 'canceled', completed_at = ?2
                 WHERE id = ?1 AND status IN ('queued', 'paused')",
                params![id, now_rfc3339()],
            )?;
            if canceled == 1 {
                return self.get_job(id)?.ok_or_else(|| DbError::NotFound(id.to_string()));
            }
        }
        let job = self.get_job(id)?.ok_or_else(|| DbError::NotFound(id.to_string()))?;
        if job.status == "running" {
            let mut payload = job.payload.clone();
            if let Value::Object(map) = &mut payload {
                map.insert("cancelRequested".into(), Value::Bool(true));
            }
            self.conn().execute("UPDATE jobs SET payload_json = ?2 WHERE id = ?1", params![id, payload.to_string()])?;
        }
        self.get_job(id)?.ok_or_else(|| DbError::NotFound(id.to_string()))
    }

    pub fn finish_canceled_job(&self, id: &str) -> DbResult<()> {
        self.expect_row(
            self.conn().execute(
                "UPDATE jobs SET status = 'canceled', completed_at = ?2 WHERE id = ?1",
                params![id, now_rfc3339()],
            )?,
            id,
        )
    }

    pub fn job_cancel_requested(&self, id: &str) -> DbResult<bool> {
        let job = self.get_job(id)?.ok_or_else(|| DbError::NotFound(id.to_string()))?;
        Ok(job.payload.get("cancelRequested").and_then(Value::as_bool).unwrap_or(false) || job.status == "canceled")
    }

    /// Startup recovery: anything left `running` by a previous process becomes
    /// `interrupted`. Resumable interrupted jobs are re-queued; others stay
    /// interrupted with an explanatory error (spec §15 — never blindly repeat).
    pub fn recover_interrupted_jobs(&self) -> DbResult<(usize, usize)> {
        let now = now_rfc3339();
        let interrupted = self.conn().execute(
            "UPDATE jobs SET status = 'interrupted', error_json = ?1 WHERE status = 'running'",
            [serde_json::json!({"code": "interrupted", "message": "Mimic was closed while this job was running."})
                .to_string()],
        )?;
        let requeued = self.conn().execute(
            "UPDATE jobs SET status = 'queued', error_json = NULL, heartbeat_at = NULL WHERE status = 'interrupted' AND resumable = 1 AND completed_at IS NULL AND created_at >= ?1",
            [chrono::Utc::now().checked_sub_signed(chrono::Duration::days(7)).map(crate::ids::fmt_rfc3339).unwrap_or(now)],
        )?;
        Ok((interrupted, requeued))
    }

    fn expect_row(&self, n: usize, id: &str) -> DbResult<()> {
        if n == 0 {
            Err(DbError::NotFound(id.to_string()))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn job_lifecycle() {
        let db = Db::open_in_memory().unwrap();
        let job = db.create_job("scan_library", &json!({"libraryId": "x"}), true).unwrap();
        assert_eq!(job.status, "queued");
        assert_eq!(db.next_queued_job().unwrap().unwrap().id, job.id);
        db.mark_job_running(&job.id).unwrap();
        db.update_job_progress(&job.id, 5, 10, Some("scanning")).unwrap();
        let j = db.get_job(&job.id).unwrap().unwrap();
        assert_eq!((j.progress_current, j.progress_total, j.phase.as_deref()), (5, 10, Some("scanning")));
        assert!(!db.job_cancel_requested(&job.id).unwrap());
        db.cancel_job(&job.id).unwrap();
        assert!(db.job_cancel_requested(&job.id).unwrap());
        db.finish_canceled_job(&job.id).unwrap();
        assert_eq!(db.get_job(&job.id).unwrap().unwrap().status, "canceled");
        assert!(db.list_jobs(10, true).unwrap().is_empty());
        assert_eq!(db.list_jobs(10, false).unwrap().len(), 1);
    }

    #[test]
    fn a_job_canceled_after_it_was_picked_is_not_started() {
        let db = Db::open_in_memory().unwrap();
        let job = db.create_job("evaluate_drafts", &json!({}), false).unwrap();
        let picked = db.next_queued_job().unwrap().unwrap();
        // Someone deletes a person, which stops any measurement, just as the
        // runner is about to start this one.
        db.cancel_job(&job.id).unwrap();
        assert!(!db.mark_job_running(&picked.id).unwrap(), "the cancel is not undone");
        assert_eq!(db.get_job(&job.id).unwrap().unwrap().status, "canceled");
        assert!(matches!(db.mark_job_running("no-such-job"), Err(DbError::NotFound(_))));
    }

    #[test]
    fn interrupted_recovery_requeues_only_resumable() {
        let db = Db::open_in_memory().unwrap();
        let scan = db.create_job("scan_library", &json!({}), true).unwrap();
        let apply = db.create_job("apply_batch", &json!({}), false).unwrap();
        db.mark_job_running(&scan.id).unwrap();
        db.mark_job_running(&apply.id).unwrap();
        let (interrupted, requeued) = db.recover_interrupted_jobs().unwrap();
        assert_eq!((interrupted, requeued), (2, 1));
        assert_eq!(db.get_job(&scan.id).unwrap().unwrap().status, "queued");
        let a = db.get_job(&apply.id).unwrap().unwrap();
        assert_eq!(a.status, "interrupted");
        assert_eq!(a.error.unwrap()["code"], "interrupted");
    }
}
