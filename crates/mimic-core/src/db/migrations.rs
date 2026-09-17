//! Forward-only, transactional SQL migrations embedded in the binary.
//!
//! Rules (CLAUDE.md): every schema change is a new numbered file; existing
//! files are never edited after release; the app backs up the database before
//! applying a migration that changes an existing install.

use rusqlite::{params, Connection};

pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[
    Migration { version: 1, name: "init", sql: include_str!("migrations/0001_init.sql") },
    Migration { version: 2, name: "sessions", sql: include_str!("migrations/0002_sessions.sql") },
    Migration { version: 3, name: "corrections", sql: include_str!("migrations/0003_corrections.sql") },
];

/// Highest schema version this build knows about.
pub fn latest_version() -> i64 {
    MIGRATIONS.iter().map(|m| m.version).max().unwrap_or(0)
}

pub fn ensure_version_table(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version    INTEGER PRIMARY KEY,
            name       TEXT NOT NULL,
            applied_at TEXT NOT NULL
        );",
    )
}

pub fn current_version(conn: &Connection) -> rusqlite::Result<i64> {
    ensure_version_table(conn)?;
    conn.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |r| r.get(0))
}

/// Apply every migration above the current version, each in its own
/// transaction. Returns the list of versions applied.
pub fn migrate_to(conn: &mut Connection, target: i64) -> rusqlite::Result<Vec<i64>> {
    ensure_version_table(conn)?;
    let current = current_version(conn)?;
    let mut applied = Vec::new();
    for m in MIGRATIONS.iter().filter(|m| m.version > current && m.version <= target) {
        let tx = conn.transaction()?;
        tx.execute_batch(m.sql)?;
        tx.execute(
            "INSERT INTO schema_migrations(version, name, applied_at) VALUES (?1, ?2, ?3)",
            params![m.version, m.name, crate::ids::now_rfc3339()],
        )?;
        tx.commit()?;
        applied.push(m.version);
    }
    Ok(applied)
}

pub fn migrate(conn: &mut Connection) -> rusqlite::Result<Vec<i64>> {
    migrate_to(conn, latest_version())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_strictly_increasing_from_one() {
        for (i, m) in MIGRATIONS.iter().enumerate() {
            assert_eq!(m.version, i as i64 + 1, "migration {} out of order", m.name);
        }
        assert_eq!(latest_version(), 3);
    }

    fn column_names(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})")).unwrap();
        stmt.query_map([], |r| r.get::<_, String>(1)).unwrap().map(Result::unwrap).collect()
    }

    #[test]
    fn v1_database_with_data_upgrades_to_latest_keeping_rows() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migrate_to(&mut conn, 1).unwrap();
        assert_eq!(current_version(&conn), Ok(1));
        assert!(!column_names(&conn, "libraries").contains(&"purpose".to_string()));
        conn.execute(
            "INSERT INTO libraries(id, name, source_type, created_at) VALUES ('l1', 'Old', 'folder_sidecars', 't')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets(id, library_id, source_path, normalized_path, file_name, extension, fast_hash, created_at, updated_at)
             VALUES ('a1', 'l1', '/p/a.cr3', '/p/a.cr3', 'a.cr3', 'cr3', 'h', 't', 't')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO sessions(id, name, created_at) VALUES ('s1', 'S', 't')", []).unwrap();
        conn.execute(
            "INSERT INTO style_profiles(id, name, created_at, updated_at) VALUES ('st', 'Style', 't', 't')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO model_versions(id, style_profile_id, semantic_version, model_type, feature_schema_version, edit_schema_version, created_at, status)
             VALUES ('mv', 'st', '1.0.0', 'hybrid', 'f1', '1.0', 't', 'ready')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO predictions(id, session_id, asset_id, model_version_id, predicted_settings_json, confidence, created_at, status)
             VALUES ('p1', 's1', 'a1', 'mv', '{}', 0.5, 't', 'pending')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO apply_batches(id, session_id, started_at, status) VALUES ('b1', 's1', 't', 'completed')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO applied_edits(id, apply_batch_id, prediction_id, asset_id, applied_settings_json, result, applied_at)
             VALUES ('e1', 'b1', 'p1', 'a1', '{}', 'applied', 't')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO corrections(id, asset_id, prediction_id, model_version_id, predicted_settings_json, corrected_settings_json, delta_json, correction_magnitude, observed_at)
             VALUES ('c1', 'a1', 'p1', 'mv', '{}', '{}', '{}', 0.1, 't')",
            [],
        )
        .unwrap();
        let applied = migrate(&mut conn).unwrap();
        assert_eq!(applied, vec![2, 3]);
        assert_eq!(current_version(&conn), Ok(latest_version()));
        let purpose: String =
            conn.query_row("SELECT purpose FROM libraries WHERE id = 'l1'", [], |r| r.get(0)).unwrap();
        assert_eq!(purpose, "training", "existing libraries default to training");
        let cols = column_names(&conn, "applied_edits");
        for c in ["restored_at", "restore_result", "restore_error_json"] {
            assert!(cols.contains(&c.to_string()), "missing {c}");
        }
        let cols = column_names(&conn, "predictions");
        assert!(cols.contains(&"capability_schema_version".to_string()));
        assert!(cols.contains(&"cluster_id".to_string()));
        let (result, restored): (String, Option<String>) = conn
            .query_row("SELECT result, restore_result FROM applied_edits WHERE id = 'e1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((result.as_str(), restored), ("applied", None));
        assert!(
            conn.execute("UPDATE libraries SET purpose = 'bogus' WHERE id = 'l1'", []).is_err(),
            "purpose is constrained"
        );
        assert!(column_names(&conn, "correction_syncs").contains(&"untouched_count".to_string()));
        assert!(
            conn.execute(
                "INSERT INTO corrections(id, asset_id, prediction_id, model_version_id, predicted_settings_json, corrected_settings_json, delta_json, correction_magnitude, observed_at)
                 VALUES ('c2', 'a1', 'p1', 'mv', '{}', '{}', '{}', 0.1, 't')",
                [],
            )
            .is_err(),
            "one correction per prediction"
        );
        assert!(migrate(&mut conn).unwrap().is_empty(), "idempotent");
    }
}
