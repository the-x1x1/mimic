//! Analysis runs: what was computed, over how much, and when.
//!
//! These rows are what lets the Voice screen say "last analyzed 3 days ago
//! over 4,182 of your messages" instead of showing a number with no
//! provenance.

use rusqlite::{params, OptionalExtension, Row};
use serde_json::Value;

use super::models::{json_col, json_obj};
use super::{AnalysisRun, Db, DbError, DbResult};
use crate::ids::{new_id, now_rfc3339};

const COLS: &str = "id, kind, analysis_version, scope_json, started_at, completed_at, status, messages_considered, profiles_written, error_json";

fn map(r: &Row<'_>) -> rusqlite::Result<AnalysisRun> {
    Ok(AnalysisRun {
        id: r.get(0)?,
        kind: r.get(1)?,
        analysis_version: r.get(2)?,
        scope: json_obj(r.get(3)?),
        started_at: r.get(4)?,
        completed_at: r.get(5)?,
        status: r.get(6)?,
        messages_considered: r.get(7)?,
        profiles_written: r.get(8)?,
        error: json_col(r.get(9)?),
    })
}

impl Db {
    pub fn start_analysis_run(&self, kind: &str, analysis_version: &str, scope: &Value) -> DbResult<String> {
        let id = new_id();
        self.conn().execute(
            "INSERT INTO analysis_runs(id, kind, analysis_version, scope_json, started_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, 'running')",
            params![id, kind, analysis_version, scope.to_string(), now_rfc3339()],
        )?;
        Ok(id)
    }

    pub fn finish_analysis_run(
        &self,
        id: &str,
        status: &str,
        messages_considered: i64,
        profiles_written: i64,
        error: Option<&Value>,
    ) -> DbResult<()> {
        if !["completed", "failed", "canceled"].contains(&status) {
            return Err(DbError::Invalid(format!("unknown analysis status {status:?}")));
        }
        self.conn().execute(
            "UPDATE analysis_runs SET status = ?1, completed_at = ?2, messages_considered = ?3,
                                      profiles_written = ?4, error_json = ?5 WHERE id = ?6",
            params![status, now_rfc3339(), messages_considered, profiles_written, error.map(|e| e.to_string()), id],
        )?;
        Ok(())
    }

    /// The most recent completed run of a kind.
    pub fn last_analysis_run(&self, kind: &str) -> DbResult<Option<AnalysisRun>> {
        Ok(self
            .conn()
            .query_row(
                &format!(
                    "SELECT {COLS} FROM analysis_runs WHERE kind = ?1 AND status = 'completed'
                     ORDER BY completed_at DESC LIMIT 1"
                ),
                [kind],
                map,
            )
            .optional()?)
    }

    pub fn recent_analysis_runs(&self, limit: usize) -> DbResult<Vec<AnalysisRun>> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare(&format!("SELECT {COLS} FROM analysis_runs ORDER BY started_at DESC LIMIT {limit}"))?;
        let rows = stmt.query_map([], map)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_run_is_only_the_latest_once_it_has_finished() {
        let db = Db::open_in_memory().unwrap();
        let id = db.start_analysis_run("voice", "voice_v1", &json!({"scope": "all"})).unwrap();
        assert!(db.last_analysis_run("voice").unwrap().is_none(), "a running analysis is not a result");
        db.finish_analysis_run(&id, "completed", 1200, 7, None).unwrap();
        let run = db.last_analysis_run("voice").unwrap().unwrap();
        assert_eq!((run.messages_considered, run.profiles_written), (1200, 7));
        assert!(run.completed_at.is_some());
        assert_eq!(run.scope["scope"], "all");
    }

    #[test]
    fn a_failed_run_keeps_its_reason_and_does_not_become_the_latest() {
        let db = Db::open_in_memory().unwrap();
        let ok = db.start_analysis_run("voice", "voice_v1", &json!({})).unwrap();
        db.finish_analysis_run(&ok, "completed", 10, 1, None).unwrap();
        let bad = db.start_analysis_run("voice", "voice_v1", &json!({})).unwrap();
        db.finish_analysis_run(&bad, "failed", 0, 0, Some(&json!({"message": "disk full"}))).unwrap();
        assert_eq!(db.last_analysis_run("voice").unwrap().unwrap().id, ok);
        assert!(db.finish_analysis_run(&bad, "exploded", 0, 0, None).is_err());
        let all = db.recent_analysis_runs(10).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].error.as_ref().unwrap()["message"], "disk full");
    }
}
