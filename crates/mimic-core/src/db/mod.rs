//! SQLite access. One connection behind a mutex; every call is short.
//!
//! WAL mode + foreign keys are enabled on open. Migrations run on open; when
//! an existing database will be migrated, a timestamped backup is written to
//! `data/backups/` first (spec §24.4).

pub mod migrations;
pub mod models;
mod repo_analysis;
mod repo_drafts;
mod repo_identity;
mod repo_jobs;
mod repo_messages;
pub mod repo_people;
mod repo_situations;
mod repo_sources;
mod repo_voice;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};

use crate::ids::now_rfc3339;

pub use models::*;
pub use repo_drafts::{DraftOutcomes, NewDraft};
pub use repo_messages::{word_count, AwaitingReply, ImportCounts, NewMessage, SelfScope};
pub use repo_people::IdentifierInput;
pub use repo_sources::channel_is_known;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
}

pub type DbResult<T> = Result<T, DbError>;

#[derive(Clone)]
pub struct Db {
    inner: Arc<Mutex<Connection>>,
    path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenReport {
    pub schema_version_before: i64,
    pub schema_version_after: i64,
    pub applied: Vec<i64>,
    pub backup_path: Option<PathBuf>,
}

impl Db {
    /// Open (creating if needed) and migrate the database at `path`.
    pub fn open(path: &Path, backups_dir: &Path) -> DbResult<(Db, OpenReport)> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let existed = path.exists();
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let mut conn = Connection::open_with_flags(path, flags)?;
        configure(&conn)?;
        let before = migrations::current_version(&conn)?;
        let needs_migration = before < migrations::latest_version();
        let backup_path = if existed && needs_migration {
            Some(backup_before_migration(&conn, path, backups_dir, before)?)
        } else {
            None
        };
        let applied = migrations::migrate(&mut conn)?;
        let after = migrations::current_version(&conn)?;
        let db = Db { inner: Arc::new(Mutex::new(conn)), path: Some(path.to_path_buf()) };
        db.ensure_update_state_row()?;
        Ok((db, OpenReport { schema_version_before: before, schema_version_after: after, applied, backup_path }))
    }

    /// In-memory database, fully migrated. Used by tests and demo mode.
    pub fn open_in_memory() -> DbResult<Db> {
        let mut conn = Connection::open_in_memory()?;
        configure(&conn)?;
        migrations::migrate(&mut conn)?;
        let db = Db { inner: Arc::new(Mutex::new(conn)), path: None };
        db.ensure_update_state_row()?;
        Ok(db)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub(crate) fn conn(&self) -> MutexGuard<'_, Connection> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn schema_version(&self) -> DbResult<i64> {
        Ok(migrations::current_version(&self.conn())?)
    }

    /// Run a closure inside a transaction.
    pub fn transaction<T>(&self, f: impl FnOnce(&rusqlite::Transaction<'_>) -> DbResult<T>) -> DbResult<T> {
        let mut guard = self.conn();
        let tx = guard.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    // ----- app_settings -------------------------------------------------

    pub fn get_setting<T: DeserializeOwned>(&self, key: &str) -> DbResult<Option<T>> {
        let conn = self.conn();
        let raw: Option<String> =
            conn.query_row("SELECT value_json FROM app_settings WHERE key = ?1", [key], |r| r.get(0)).optional()?;
        match raw {
            Some(s) => Ok(Some(serde_json::from_str(&s)?)),
            None => Ok(None),
        }
    }

    pub fn set_setting<T: Serialize>(&self, key: &str, value: &T) -> DbResult<()> {
        let json = serde_json::to_string(value)?;
        self.conn().execute(
            "INSERT INTO app_settings(key, value_json, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at",
            params![key, json, now_rfc3339()],
        )?;
        Ok(())
    }

    pub fn all_settings(&self) -> DbResult<serde_json::Map<String, serde_json::Value>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT key, value_json FROM app_settings ORDER BY key")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = serde_json::Map::new();
        for row in rows {
            let (k, v) = row?;
            out.insert(k, serde_json::from_str(&v)?);
        }
        Ok(out)
    }

    // ----- events -------------------------------------------------------

    pub fn log_event(&self, ev: &NewEvent<'_>) -> DbResult<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO events(level, category, event_type, entity_type, entity_id, payload_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                ev.level,
                ev.category,
                ev.event_type,
                ev.entity_type,
                ev.entity_id,
                serde_json::to_string(&ev.payload)?,
                now_rfc3339()
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn recent_events(&self, limit: usize, min_level: Option<&str>) -> DbResult<Vec<EventRow>> {
        let conn = self.conn();
        let levels: Vec<&str> = match min_level {
            Some("error") => vec!["error"],
            Some("warn") => vec!["warn", "error"],
            _ => vec!["debug", "info", "warn", "error"],
        };
        let placeholders = levels.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, level, category, event_type, entity_type, entity_id, payload_json, created_at
             FROM events WHERE level IN ({placeholders}) ORDER BY id DESC LIMIT {limit}"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(levels.iter()), |r| {
            Ok(EventRow {
                id: r.get(0)?,
                level: r.get(1)?,
                category: r.get(2)?,
                event_type: r.get(3)?,
                entity_type: r.get(4)?,
                entity_id: r.get(5)?,
                payload_json: r.get(6)?,
                created_at: r.get(7)?,
            })
        })?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    // ----- update_state -------------------------------------------------

    fn ensure_update_state_row(&self) -> DbResult<()> {
        self.conn().execute(
            "INSERT OR IGNORE INTO update_state(id, current_version, channel) VALUES (1, ?1, 'stable')",
            [crate::APP_VERSION],
        )?;
        // Keep current_version honest after an upgrade.
        self.conn().execute("UPDATE update_state SET current_version = ?1 WHERE id = 1", [crate::APP_VERSION])?;
        Ok(())
    }

    pub fn update_state(&self) -> DbResult<UpdateState> {
        Ok(self.conn().query_row(
            "SELECT current_version, latest_seen_version, staged_version, channel, last_checked_at, last_update_result, update_error
             FROM update_state WHERE id = 1",
            [],
            |r| {
                Ok(UpdateState {
                    current_version: r.get(0)?,
                    latest_seen_version: r.get(1)?,
                    staged_version: r.get(2)?,
                    channel: r.get(3)?,
                    last_checked_at: r.get(4)?,
                    last_update_result: r.get(5)?,
                    update_error: r.get(6)?,
                })
            },
        )?)
    }

    pub fn save_update_state(&self, s: &UpdateState) -> DbResult<()> {
        self.conn().execute(
            "UPDATE update_state SET latest_seen_version = ?1, staged_version = ?2, channel = ?3,
             last_checked_at = ?4, last_update_result = ?5, update_error = ?6 WHERE id = 1",
            params![
                s.latest_seen_version,
                s.staged_version,
                s.channel,
                s.last_checked_at,
                s.last_update_result,
                s.update_error
            ],
        )?;
        Ok(())
    }

    // ----- maintenance --------------------------------------------------

    /// Table row counts for diagnostics.
    pub fn table_counts(&self) -> DbResult<Vec<(String, i64)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?;
        let names: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        let mut out = Vec::with_capacity(names.len());
        for name in names {
            let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM \"{name}\""), [], |r| r.get(0))?;
            out.push((name, n));
        }
        Ok(out)
    }

    /// Online backup to `dest` using SQLite's backup API.
    pub fn backup_to(&self, dest: &Path) -> DbResult<()> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = self.conn();
        let mut dst = Connection::open(dest)?;
        let backup = rusqlite::backup::Backup::new(&conn, &mut dst)?;
        backup.run_to_completion(256, std::time::Duration::from_millis(5), None)?;
        Ok(())
    }
}

fn configure(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;
         PRAGMA temp_store = MEMORY;",
    )
}

fn backup_before_migration(conn: &Connection, path: &Path, backups_dir: &Path, from_version: i64) -> DbResult<PathBuf> {
    std::fs::create_dir_all(backups_dir)?;
    let stamp = now_rfc3339().replace([':', '.'], "-");
    let file_name =
        format!("{}.v{from_version}.{stamp}.bak", path.file_name().and_then(|n| n.to_str()).unwrap_or("mimic.db"));
    let dest = backups_dir.join(file_name);
    let mut dst = Connection::open(&dest)?;
    let backup = rusqlite::backup::Backup::new(conn, &mut dst)?;
    backup.run_to_completion(256, std::time::Duration::from_millis(5), None)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_install_migrates_to_latest() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, report) = Db::open(&tmp.path().join("data").join("mimic.db"), &tmp.path().join("backups")).unwrap();
        assert_eq!(report.schema_version_before, 0);
        assert_eq!(report.schema_version_after, migrations::latest_version());
        assert!(report.backup_path.is_none(), "fresh install must not create a backup");
        assert_eq!(db.schema_version().unwrap(), migrations::latest_version());
        let counts = db.table_counts().unwrap();
        let names: Vec<&str> = counts.iter().map(|(n, _)| n.as_str()).collect();
        for t in [
            "app_settings",
            "user_identity",
            "user_identifiers",
            "sources",
            "participants",
            "participant_identifiers",
            "conversations",
            "conversation_participants",
            "messages",
            "message_embeddings",
            "situations",
            "message_situations",
            "voice_profiles",
            "voice_preferences",
            "representative_examples",
            "drafts",
            "draft_feedback",
            "analysis_runs",
            "evaluations",
            "evaluation_cases",
            "jobs",
            "events",
            "update_state",
        ] {
            assert!(names.contains(&t), "missing table {t}");
        }
    }

    #[test]
    fn reopen_is_idempotent_and_keeps_data() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("mimic.db");
        {
            let (db, _) = Db::open(&file, &tmp.path().join("backups")).unwrap();
            db.set_setting("theme", &"dark").unwrap();
        }
        let (db, report) = Db::open(&file, &tmp.path().join("backups")).unwrap();
        assert!(report.applied.is_empty());
        assert!(report.backup_path.is_none());
        assert_eq!(db.get_setting::<String>("theme").unwrap().as_deref(), Some("dark"));
    }

    #[test]
    fn upgrade_from_older_schema_creates_backup() {
        // Simulate an install at schema version 0 (only the migrations table).
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("mimic.db");
        {
            let conn = Connection::open(&file).unwrap();
            migrations::ensure_version_table(&conn).unwrap();
        }
        let backups = tmp.path().join("backups");
        let (db, report) = Db::open(&file, &backups).unwrap();
        assert_eq!(report.schema_version_before, 0);
        assert_eq!(report.schema_version_after, migrations::latest_version());
        let backup = report.backup_path.expect("backup expected before migrating an existing db");
        assert!(backup.exists());
        assert!(backup.starts_with(&backups));
        assert_eq!(db.schema_version().unwrap(), migrations::latest_version());
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let db = Db::open_in_memory().unwrap();
        let err = db
            .conn()
            .execute(
                "INSERT INTO conversations(id, source_id, external_id, channel, created_at)
                 VALUES ('c','missing','x','email','t')",
                [],
            )
            .unwrap_err();
        assert!(err.to_string().contains("FOREIGN KEY"), "{err}");
    }

    #[test]
    fn settings_roundtrip_and_events() {
        let db = Db::open_in_memory().unwrap();
        db.set_setting("concurrency", &4u32).unwrap();
        db.set_setting("concurrency", &6u32).unwrap();
        assert_eq!(db.get_setting::<u32>("concurrency").unwrap(), Some(6));
        assert_eq!(db.get_setting::<u32>("missing").unwrap(), None);
        db.log_event(&NewEvent::info("app", "started", serde_json::json!({"v": 1}))).unwrap();
        db.log_event(&NewEvent::error("engine", "crashed", serde_json::json!({}))).unwrap();
        assert_eq!(db.recent_events(10, Some("error")).unwrap().len(), 1);
        assert_eq!(db.recent_events(10, None).unwrap().len(), 2);
        let st = db.update_state().unwrap();
        assert_eq!(st.current_version, crate::APP_VERSION);
        assert_eq!(st.channel, "stable");
    }
}
