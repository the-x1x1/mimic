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

pub const MIGRATIONS: &[Migration] =
    &[Migration { version: 1, name: "init", sql: include_str!("migrations/0001_init.sql") }];

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
    }
}
