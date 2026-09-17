use rusqlite::{params, OptionalExtension, Row};

use super::models::json_col_or_default;
use super::{Db, DbError, DbResult, EditSnapshot, NewEditSnapshot};
use crate::ids::{new_id, now_rfc3339};

const COLS: &str = "id, asset_id, source, process_version, normalized_settings_json, raw_settings_json, unknown_settings_json, mapping_version, capability_schema_version, observed_at, provenance_json";

fn map(r: &Row<'_>) -> rusqlite::Result<EditSnapshot> {
    Ok(EditSnapshot {
        id: r.get(0)?,
        asset_id: r.get(1)?,
        source: r.get(2)?,
        process_version: r.get(3)?,
        normalized_settings: json_col_or_default(r.get(4)?),
        raw_settings: json_col_or_default(r.get(5)?),
        unknown_settings: json_col_or_default(r.get(6)?),
        mapping_version: r.get(7)?,
        capability_schema_version: r.get(8)?,
        observed_at: r.get(9)?,
        provenance: json_col_or_default(r.get(10)?),
    })
}

impl Db {
    pub fn insert_edit_snapshot(&self, s: &NewEditSnapshot) -> DbResult<EditSnapshot> {
        if !matches!(s.source.as_str(), "xmp" | "lightroom_sdk" | "prediction" | "correction") {
            return Err(DbError::Invalid(format!("unknown snapshot source {}", s.source)));
        }
        let id = new_id();
        self.conn().execute(
            &format!("INSERT INTO edit_snapshots({COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)"),
            params![
                id,
                s.asset_id,
                s.source,
                s.process_version,
                s.normalized_settings.to_string(),
                s.raw_settings.to_string(),
                s.unknown_settings.to_string(),
                s.mapping_version,
                s.capability_schema_version,
                now_rfc3339(),
                s.provenance.to_string()
            ],
        )?;
        self.get_edit_snapshot(&id)?.ok_or(DbError::NotFound(id))
    }

    pub fn get_edit_snapshot(&self, id: &str) -> DbResult<Option<EditSnapshot>> {
        Ok(self.conn().query_row(&format!("SELECT {COLS} FROM edit_snapshots WHERE id = ?1"), [id], map).optional()?)
    }

    /// Most recent observed (non-prediction) snapshot for an asset.
    pub fn latest_observed_snapshot(&self, asset_id: &str) -> DbResult<Option<EditSnapshot>> {
        Ok(self
            .conn()
            .query_row(
                &format!(
                    "SELECT {COLS} FROM edit_snapshots WHERE asset_id = ?1 AND source IN ('xmp','lightroom_sdk','correction')
                     ORDER BY observed_at DESC LIMIT 1"
                ),
                [asset_id],
                map,
            )
            .optional()?)
    }

    pub fn snapshots_for_asset(&self, asset_id: &str) -> DbResult<Vec<EditSnapshot>> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare(&format!("SELECT {COLS} FROM edit_snapshots WHERE asset_id = ?1 ORDER BY observed_at"))?;
        let rows = stmt.query_map([asset_id], map)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// Number of assets in a library with at least one observed edit snapshot.
    pub fn count_assets_with_edits(&self, library_id: &str) -> DbResult<i64> {
        Ok(self.conn().query_row(
            "SELECT COUNT(DISTINCT a.id) FROM assets a JOIN edit_snapshots s ON s.asset_id = a.id
             WHERE a.library_id = ?1 AND s.source IN ('xmp','lightroom_sdk')",
            [library_id],
            |r| r.get(0),
        )?)
    }

    /// Source breakdown for a library: (source, distinct asset count).
    pub fn edit_source_breakdown(&self, library_id: &str) -> DbResult<Vec<(String, i64)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT s.source, COUNT(DISTINCT s.asset_id) FROM edit_snapshots s JOIN assets a ON a.id = s.asset_id
             WHERE a.library_id = ?1 GROUP BY s.source",
        )?;
        let rows = stmt.query_map([library_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::NewAsset;
    use serde_json::json;

    #[test]
    fn snapshot_roundtrip_preserves_json_structure() {
        let db = Db::open_in_memory().unwrap();
        let (asset, _) = db
            .upsert_asset(&NewAsset {
                source_path: "/p/a.cr3".into(),
                file_name: "a.cr3".into(),
                extension: "cr3".into(),
                fast_hash: "h".into(),
                ..Default::default()
            })
            .unwrap();
        let snap = db
            .insert_edit_snapshot(&NewEditSnapshot {
                asset_id: asset.id.clone(),
                source: "xmp".into(),
                process_version: Some("15.4".into()),
                normalized_settings: json!({"tone": {"exposure": {"raw": 0.5, "value": 0.55}}}),
                raw_settings: json!({"Exposure2012": "+0.50"}),
                unknown_settings: json!({"FutureKey": "x"}),
                mapping_version: "edit_mapping_v1".into(),
                capability_schema_version: None,
                provenance: json!({"parserVersion": "xmp_parser_v1"}),
            })
            .unwrap();
        assert_eq!(snap.normalized_settings["tone"]["exposure"]["raw"], 0.5);
        assert_eq!(snap.unknown_settings["FutureKey"], "x");
        assert!(db
            .insert_edit_snapshot(&NewEditSnapshot { source: "bogus".into(), ..snapshot_like(&asset.id) })
            .is_err());
        assert_eq!(db.latest_observed_snapshot(&asset.id).unwrap().unwrap().id, snap.id);
        assert_eq!(db.snapshots_for_asset(&asset.id).unwrap().len(), 1);
    }

    fn snapshot_like(asset_id: &str) -> NewEditSnapshot {
        NewEditSnapshot {
            asset_id: asset_id.to_string(),
            source: "xmp".into(),
            process_version: None,
            normalized_settings: json!({}),
            raw_settings: json!({}),
            unknown_settings: json!({}),
            mapping_version: "edit_mapping_v1".into(),
            capability_schema_version: None,
            provenance: json!({}),
        }
    }
}
