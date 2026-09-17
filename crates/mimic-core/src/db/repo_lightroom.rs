use rusqlite::{params, OptionalExtension, Row};
use serde_json::Value;

use super::models::json_col_or_default;
use super::{Db, DbError, DbResult, LightroomConnection};
use crate::ids::{new_id, now_rfc3339};

const COLS: &str = "id, catalog_fingerprint, lightroom_version, sdk_version, plugin_version, capabilities_json, first_seen_at, last_seen_at, status";

fn map(r: &Row<'_>) -> rusqlite::Result<LightroomConnection> {
    Ok(LightroomConnection {
        id: r.get(0)?,
        catalog_fingerprint: r.get(1)?,
        lightroom_version: r.get(2)?,
        sdk_version: r.get(3)?,
        plugin_version: r.get(4)?,
        capabilities: json_col_or_default(r.get(5)?),
        first_seen_at: r.get(6)?,
        last_seen_at: r.get(7)?,
        status: r.get(8)?,
    })
}

impl Db {
    pub fn record_lightroom_connection(
        &self,
        catalog_fingerprint: &str,
        lightroom_version: Option<&str>,
        sdk_version: Option<&str>,
        plugin_version: Option<&str>,
        capabilities: &Value,
        status: &str,
    ) -> DbResult<LightroomConnection> {
        let now = now_rfc3339();
        self.conn().execute(
            "INSERT INTO lightroom_connections(id, catalog_fingerprint, lightroom_version, sdk_version, plugin_version,
               capabilities_json, first_seen_at, last_seen_at, status)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?7,?8)
             ON CONFLICT(catalog_fingerprint) DO UPDATE SET lightroom_version = excluded.lightroom_version,
               sdk_version = excluded.sdk_version, plugin_version = excluded.plugin_version,
               capabilities_json = excluded.capabilities_json, last_seen_at = excluded.last_seen_at, status = excluded.status",
            params![new_id(), catalog_fingerprint, lightroom_version, sdk_version, plugin_version, capabilities.to_string(), now, status],
        )?;
        let conn = self.conn();
        Ok(conn.query_row(
            &format!("SELECT {COLS} FROM lightroom_connections WHERE catalog_fingerprint = ?1"),
            [catalog_fingerprint],
            map,
        )?)
    }

    pub fn set_lightroom_connection_status(&self, catalog_fingerprint: &str, status: &str) -> DbResult<()> {
        self.conn().execute(
            "UPDATE lightroom_connections SET status = ?2, last_seen_at = ?3 WHERE catalog_fingerprint = ?1",
            params![catalog_fingerprint, status, now_rfc3339()],
        )?;
        Ok(())
    }

    pub fn latest_lightroom_connection(&self) -> DbResult<Option<LightroomConnection>> {
        Ok(self
            .conn()
            .query_row(&format!("SELECT {COLS} FROM lightroom_connections ORDER BY last_seen_at DESC LIMIT 1"), [], map)
            .optional()?)
    }

    pub fn list_lightroom_connections(&self) -> DbResult<Vec<LightroomConnection>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM lightroom_connections ORDER BY last_seen_at DESC"))?;
        let rows = stmt.query_map([], map)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn connection_upsert_by_catalog() {
        let db = Db::open_in_memory().unwrap();
        let c1 = db
            .record_lightroom_connection(
                "cat-1",
                Some("14.3"),
                Some("14.0"),
                Some("0.1.0"),
                &json!({"snapshots": true}),
                "connected",
            )
            .unwrap();
        let c2 = db
            .record_lightroom_connection(
                "cat-1",
                Some("14.4"),
                Some("14.0"),
                Some("0.1.0"),
                &json!({"snapshots": true}),
                "connected",
            )
            .unwrap();
        assert_eq!(c1.id, c2.id);
        assert_eq!(c2.lightroom_version.as_deref(), Some("14.4"));
        db.set_lightroom_connection_status("cat-1", "disconnected").unwrap();
        assert_eq!(db.latest_lightroom_connection().unwrap().unwrap().status, "disconnected");
        assert_eq!(db.list_lightroom_connections().unwrap().len(), 1);
    }
}
