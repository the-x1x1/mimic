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
    Migration {
        version: 4,
        name: "session_intelligence",
        sql: include_str!("migrations/0004_session_intelligence.sql"),
    },
    Migration { version: 5, name: "communication", sql: include_str!("migrations/0005_communication.sql") },
    Migration { version: 6, name: "themes", sql: include_str!("migrations/0006_themes.sql") },
    Migration { version: 7, name: "situations", sql: include_str!("migrations/0007_situations.sql") },
    Migration { version: 8, name: "message_ids", sql: include_str!("migrations/0008_message_ids.sql") },
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
        assert_eq!(latest_version(), 8);
    }

    fn table_names(conn: &Connection) -> Vec<String> {
        let mut stmt =
            conn.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'").unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0)).unwrap().map(Result::unwrap).collect()
    }

    fn column_names(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})")).unwrap();
        stmt.query_map([], |r| r.get::<_, String>(1)).unwrap().map(Result::unwrap).collect()
    }

    /// The situation vocabulary arrives with the migration, matches the six
    /// the classifier knows, and survives being applied to a database that
    /// already has them.
    #[test]
    fn the_situation_vocabulary_is_seeded_and_matches_the_classifier() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        let mut stmt = conn.prepare("SELECT id, label, is_builtin FROM situations ORDER BY rowid").unwrap();
        let rows: Vec<(String, String, i64)> =
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect();
        let expected: Vec<(String, String, i64)> =
            crate::situations::BUILTINS.iter().map(|b| (b.id.to_string(), b.label.to_string(), 1)).collect();
        assert_eq!(rows, expected);
        // Re-running the seed is harmless.
        conn.execute_batch(MIGRATIONS[6].sql).unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM situations", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 6);
    }

    /// A theme name that this build cannot render is carried over rather than
    /// left in place to fail validation the first time settings are parsed.
    #[test]
    fn an_old_theme_name_is_carried_over_instead_of_breaking_the_first_render() {
        for (stored, expected) in [("\"dark\"", "\"night\""), ("\"light\"", "\"plain\""), ("\"graphite\"", "\"plain\"")]
        {
            let mut conn = Connection::open_in_memory().unwrap();
            migrate_to(&mut conn, 5).unwrap();
            conn.execute(
                "INSERT INTO app_settings(key, value_json, updated_at) VALUES ('general.theme', ?1, 't')",
                params![stored],
            )
            .unwrap();
            migrate(&mut conn).unwrap();
            let got: String = conn
                .query_row("SELECT value_json FROM app_settings WHERE key='general.theme'", [], |r| r.get(0))
                .unwrap();
            assert_eq!(got, expected, "{stored} should become {expected}");
        }
    }

    /// A photography install upgrades cleanly: the generic tables keep their
    /// rows, every photography table is gone, and the communication schema is
    /// in place with its constraints.
    #[test]
    fn photography_database_upgrades_to_the_communication_schema() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migrate_to(&mut conn, 4).unwrap();
        assert_eq!(current_version(&conn), Ok(4));

        // Seed a realistic v4 install: photography rows plus the generic rows
        // that must survive.
        conn.execute(
            "INSERT INTO libraries(id, name, source_type, created_at) VALUES ('l1','Old','folder_sidecars','t')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets(id, library_id, source_path, normalized_path, file_name, extension, fast_hash, created_at, updated_at)
             VALUES ('a1','l1','/p/a.cr3','/p/a.cr3','a.cr3','cr3','h','t','t')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO sessions(id, name, created_at) VALUES ('s1','S','t')", []).unwrap();
        conn.execute("INSERT INTO style_profiles(id, name, created_at, updated_at) VALUES ('st','Style','t','t')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO app_settings(key, value_json, updated_at) VALUES ('general.theme','\"dark\"','t')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO jobs(id, type, status, created_at) VALUES ('j1','import','completed','t')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO events(level, category, event_type, created_at) VALUES ('info','app','started','t')",
            [],
        )
        .unwrap();

        let applied = migrate(&mut conn).unwrap();
        assert_eq!(applied, vec![5, 6, 7, 8]);
        assert_eq!(current_version(&conn), Ok(latest_version()));

        let tables = table_names(&conn);
        for gone in [
            "libraries",
            "assets",
            "sidecars",
            "edit_snapshots",
            "visual_features",
            "style_profiles",
            "style_profile_libraries",
            "sessions",
            "session_assets",
            "scene_clusters",
            "training_sets",
            "model_versions",
            "model_artifacts",
            "predictions",
            "apply_batches",
            "applied_edits",
            "corrections",
            "correction_syncs",
            "lightroom_connections",
        ] {
            assert!(!tables.contains(&gone.to_string()), "photography table {gone} survived");
        }
        for kept in ["app_settings", "jobs", "events", "update_state"] {
            assert!(tables.contains(&kept.to_string()), "generic table {kept} was dropped");
        }
        for created in [
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
        ] {
            assert!(tables.contains(&created.to_string()), "missing table {created}");
        }

        // Generic rows survived the drop, and the one whose vocabulary changed
        // was carried over rather than left to fail validation on first render.
        let theme: String =
            conn.query_row("SELECT value_json FROM app_settings WHERE key='general.theme'", [], |r| r.get(0)).unwrap();
        assert_eq!(theme, "\"night\"", "an install that stored 'dark' should land on the dark theme, not lose it");
        let jobs: i64 = conn.query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get(0)).unwrap();
        let events: i64 = conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0)).unwrap();
        assert_eq!((jobs, events), (1, 1));

        assert!(column_names(&conn, "messages").contains(&"direction".to_string()));
        let situations: i64 = conn.query_row("SELECT COUNT(*) FROM situations", [], |r| r.get(0)).unwrap();
        assert_eq!(situations, 6, "an upgraded install gets the vocabulary a fresh one does");
        assert!(migrate(&mut conn).unwrap().is_empty(), "idempotent");
    }

    /// The constraints that keep the communication model honest.
    #[test]
    fn communication_schema_enforces_its_invariants() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migrate(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO sources(id, connector, name, channel, created_at) VALUES ('s','mbox','M','email','t')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO participants(id, display_name, created_at, updated_at) VALUES ('p','Ada','t','t')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO conversations(id, source_id, external_id, channel, created_at) VALUES ('c','s','x','email','t')",
            [],
        )
        .unwrap();

        // direction is a closed vocabulary
        assert!(conn
            .execute(
                "INSERT INTO messages(id, conversation_id, source_id, external_id, direction, channel, body, body_hash, imported_at)
                 VALUES ('m0','c','s','m0','sideways','email','hi','h','t')",
                [],
            )
            .is_err());

        conn.execute(
            "INSERT INTO messages(id, conversation_id, source_id, external_id, participant_id, direction, channel, body, body_hash, imported_at)
             VALUES ('m1','c','s','ext-1','p','other','email','hi','h','t')",
            [],
        )
        .unwrap();
        // (source_id, external_id) is the import identity key
        assert!(conn
            .execute(
                "INSERT INTO messages(id, conversation_id, source_id, external_id, direction, channel, body, body_hash, imported_at)
                 VALUES ('m2','c','s','ext-1','self','email','hi','h','t')",
                [],
            )
            .is_err());

        // Deleting a participant takes their messages with them.
        conn.execute("DELETE FROM participants WHERE id='p'", []).unwrap();
        let left: i64 = conn.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0, "messages must cascade from their participant");

        // Deleting a source takes its conversations with them.
        conn.execute("DELETE FROM sources WHERE id='s'", []).unwrap();
        let convos: i64 = conn.query_row("SELECT COUNT(*) FROM conversations", [], |r| r.get(0)).unwrap();
        assert_eq!(convos, 0);
    }
}
