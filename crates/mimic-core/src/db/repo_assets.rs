use rusqlite::{params, OptionalExtension, Row};
use serde_json::Value;

use super::models::{json_col, json_col_or_default, Asset};
use super::{Db, DbError, DbResult, NewAsset, NewSidecar, Sidecar, VisualFeatures};
use crate::ids::{new_id, now_rfc3339};

/// Case-folded, forward-slash path used for identity on case-insensitive filesystems.
pub fn normalize_path(p: &str) -> String {
    let mut s = p.replace('\\', "/");
    while s.ends_with('/') && s.len() > 1 {
        s.pop();
    }
    if cfg!(windows) || s.len() > 1 && s.as_bytes()[1] == b':' {
        s = s.to_lowercase();
    }
    s
}

const ASSET_COLS: &str = "id, library_id, source_path, normalized_path, file_name, extension, mime_type, size_bytes, modified_time, fast_hash, full_hash, camera_make, camera_model, lens, focal_length, iso, aperture, shutter_speed, captured_at, width, height, orientation, lightroom_local_id, created_at, updated_at";

fn map_asset(r: &Row<'_>) -> rusqlite::Result<Asset> {
    Ok(Asset {
        id: r.get(0)?,
        library_id: r.get(1)?,
        source_path: r.get(2)?,
        normalized_path: r.get(3)?,
        file_name: r.get(4)?,
        extension: r.get(5)?,
        mime_type: r.get(6)?,
        size_bytes: r.get(7)?,
        modified_time: r.get(8)?,
        fast_hash: r.get(9)?,
        full_hash: r.get(10)?,
        camera_make: r.get(11)?,
        camera_model: r.get(12)?,
        lens: r.get(13)?,
        focal_length: r.get(14)?,
        iso: r.get(15)?,
        aperture: r.get(16)?,
        shutter_speed: r.get(17)?,
        captured_at: r.get(18)?,
        width: r.get(19)?,
        height: r.get(20)?,
        orientation: r.get(21)?,
        lightroom_local_id: r.get(22)?,
        created_at: r.get(23)?,
        updated_at: r.get(24)?,
    })
}

const SIDECAR_COLS: &str =
    "id, asset_id, type, path, modified_time, hash, parse_status, parser_version, raw_metadata_json, warnings_json, detected_at";

fn map_sidecar(r: &Row<'_>) -> rusqlite::Result<Sidecar> {
    Ok(Sidecar {
        id: r.get(0)?,
        asset_id: r.get(1)?,
        kind: r.get(2)?,
        path: r.get(3)?,
        modified_time: r.get(4)?,
        hash: r.get(5)?,
        parse_status: r.get(6)?,
        parser_version: r.get(7)?,
        raw_metadata: json_col(r.get(8)?),
        warnings: json_col(r.get(9)?),
        detected_at: r.get(10)?,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpsertOutcome {
    Inserted,
    Updated,
}

impl Db {
    /// Insert or update an asset by identity (`normalized_path` + `fast_hash`).
    /// Metadata columns are only overwritten when the new value is `Some`.
    pub fn upsert_asset(&self, a: &NewAsset) -> DbResult<(Asset, UpsertOutcome)> {
        let normalized = normalize_path(&a.source_path);
        let now = now_rfc3339();
        let existing: Option<String> = self
            .conn()
            .query_row(
                "SELECT id FROM assets WHERE normalized_path = ?1 AND fast_hash = ?2",
                params![normalized, a.fast_hash],
                |r| r.get(0),
            )
            .optional()?;
        let (id, outcome) = match existing {
            Some(id) => {
                self.conn().execute(
                    "UPDATE assets SET library_id = COALESCE(?2, library_id), mime_type = COALESCE(?3, mime_type),
                       size_bytes = ?4, modified_time = COALESCE(?5, modified_time),
                       camera_make = COALESCE(?6, camera_make), camera_model = COALESCE(?7, camera_model),
                       lens = COALESCE(?8, lens), focal_length = COALESCE(?9, focal_length), iso = COALESCE(?10, iso),
                       aperture = COALESCE(?11, aperture), shutter_speed = COALESCE(?12, shutter_speed),
                       captured_at = COALESCE(?13, captured_at), width = COALESCE(?14, width), height = COALESCE(?15, height),
                       orientation = COALESCE(?16, orientation), lightroom_local_id = COALESCE(?17, lightroom_local_id),
                       updated_at = ?18
                     WHERE id = ?1",
                    params![
                        id, a.library_id, a.mime_type, a.size_bytes, a.modified_time, a.camera_make, a.camera_model,
                        a.lens, a.focal_length, a.iso, a.aperture, a.shutter_speed, a.captured_at, a.width, a.height,
                        a.orientation, a.lightroom_local_id, now
                    ],
                )?;
                (id, UpsertOutcome::Updated)
            }
            None => {
                let id = new_id();
                self.conn().execute(
                    "INSERT INTO assets(id, library_id, source_path, normalized_path, file_name, extension, mime_type,
                       size_bytes, modified_time, fast_hash, camera_make, camera_model, lens, focal_length, iso, aperture,
                       shutter_speed, captured_at, width, height, orientation, lightroom_local_id, created_at, updated_at)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?23)",
                    params![
                        id, a.library_id, a.source_path, normalized, a.file_name, a.extension.to_lowercase(), a.mime_type,
                        a.size_bytes, a.modified_time, a.fast_hash, a.camera_make, a.camera_model, a.lens, a.focal_length,
                        a.iso, a.aperture, a.shutter_speed, a.captured_at, a.width, a.height, a.orientation,
                        a.lightroom_local_id, now
                    ],
                )?;
                (id, UpsertOutcome::Inserted)
            }
        };
        let asset = self.get_asset(&id)?.ok_or_else(|| DbError::NotFound(id))?;
        Ok((asset, outcome))
    }

    pub fn get_asset(&self, id: &str) -> DbResult<Option<Asset>> {
        Ok(self
            .conn()
            .query_row(&format!("SELECT {ASSET_COLS} FROM assets WHERE id = ?1"), [id], map_asset)
            .optional()?)
    }

    pub fn find_asset_by_path(&self, source_path: &str) -> DbResult<Option<Asset>> {
        let normalized = normalize_path(source_path);
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {ASSET_COLS} FROM assets WHERE normalized_path = ?1 ORDER BY updated_at DESC LIMIT 1"),
                [normalized],
                map_asset,
            )
            .optional()?)
    }

    pub fn list_assets(&self, library_id: Option<&str>, limit: usize, offset: usize) -> DbResult<Vec<Asset>> {
        let conn = self.conn();
        let (sql, args): (String, Vec<String>) = match library_id {
            Some(l) => (
                format!("SELECT {ASSET_COLS} FROM assets WHERE library_id = ?1 ORDER BY captured_at, file_name LIMIT {limit} OFFSET {offset}"),
                vec![l.to_string()],
            ),
            None => (
                format!("SELECT {ASSET_COLS} FROM assets ORDER BY captured_at, file_name LIMIT {limit} OFFSET {offset}"),
                vec![],
            ),
        };
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), map_asset)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    pub fn count_assets(&self, library_id: Option<&str>) -> DbResult<i64> {
        let conn = self.conn();
        Ok(match library_id {
            Some(l) => conn.query_row("SELECT COUNT(*) FROM assets WHERE library_id = ?1", [l], |r| r.get(0))?,
            None => conn.query_row("SELECT COUNT(*) FROM assets", [], |r| r.get(0))?,
        })
    }

    // ----- sidecars -----------------------------------------------------

    pub fn upsert_sidecar(&self, s: &NewSidecar) -> DbResult<Sidecar> {
        let id = new_id();
        self.conn().execute(
            "INSERT INTO sidecars(id, asset_id, type, path, modified_time, hash, parse_status, parser_version,
               raw_metadata_json, warnings_json, detected_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(path) DO UPDATE SET asset_id = excluded.asset_id, modified_time = excluded.modified_time,
               hash = excluded.hash, parse_status = excluded.parse_status, parser_version = excluded.parser_version,
               raw_metadata_json = excluded.raw_metadata_json, warnings_json = excluded.warnings_json,
               detected_at = excluded.detected_at",
            params![
                id,
                s.asset_id,
                s.kind,
                s.path,
                s.modified_time,
                s.hash,
                s.parse_status,
                s.parser_version,
                s.raw_metadata.as_ref().map(|v| v.to_string()),
                s.warnings.as_ref().map(|v| v.to_string()),
                now_rfc3339()
            ],
        )?;
        let conn = self.conn();
        Ok(conn.query_row(&format!("SELECT {SIDECAR_COLS} FROM sidecars WHERE path = ?1"), [&s.path], map_sidecar)?)
    }

    pub fn sidecars_for_asset(&self, asset_id: &str) -> DbResult<Vec<Sidecar>> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare(&format!("SELECT {SIDECAR_COLS} FROM sidecars WHERE asset_id = ?1 ORDER BY type"))?;
        let rows = stmt.query_map([asset_id], map_sidecar)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    // ----- visual features ---------------------------------------------

    pub fn upsert_visual_features(&self, f: &VisualFeatures) -> DbResult<()> {
        self.conn().execute(
            "INSERT INTO visual_features(asset_id, feature_version, histogram_json, luminance_json, color_json, sharpness,
               noise_estimate, clipping_json, scene_labels_json, embedding_artifact_id, preview_path, computed_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(asset_id, feature_version) DO UPDATE SET histogram_json = excluded.histogram_json,
               luminance_json = excluded.luminance_json, color_json = excluded.color_json, sharpness = excluded.sharpness,
               noise_estimate = excluded.noise_estimate, clipping_json = excluded.clipping_json,
               scene_labels_json = excluded.scene_labels_json, embedding_artifact_id = excluded.embedding_artifact_id,
               preview_path = excluded.preview_path, computed_at = excluded.computed_at",
            params![
                f.asset_id,
                f.feature_version,
                f.histogram.to_string(),
                f.luminance.to_string(),
                f.color.to_string(),
                f.sharpness,
                f.noise_estimate,
                f.clipping.to_string(),
                f.scene_labels.to_string(),
                f.embedding_artifact_id,
                f.preview_path,
                f.computed_at
            ],
        )?;
        Ok(())
    }

    pub fn visual_features(&self, asset_id: &str, feature_version: &str) -> DbResult<Option<VisualFeatures>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT asset_id, feature_version, histogram_json, luminance_json, color_json, sharpness, noise_estimate,
                        clipping_json, scene_labels_json, embedding_artifact_id, preview_path, computed_at
                 FROM visual_features WHERE asset_id = ?1 AND feature_version = ?2",
                params![asset_id, feature_version],
                |r| {
                    Ok(VisualFeatures {
                        asset_id: r.get(0)?,
                        feature_version: r.get(1)?,
                        histogram: json_col_or_default(r.get(2)?),
                        luminance: json_col_or_default(r.get(3)?),
                        color: json_col_or_default(r.get(4)?),
                        sharpness: r.get(5)?,
                        noise_estimate: r.get(6)?,
                        clipping: json_col_or_default(r.get(7)?),
                        scene_labels: json_col_or_default(r.get(8)?),
                        embedding_artifact_id: r.get(9)?,
                        preview_path: r.get(10)?,
                        computed_at: r.get(11)?,
                    })
                },
            )
            .optional()?)
    }

    /// Asset ids in a library lacking features for the given version.
    pub fn assets_missing_features(&self, library_id: &str, feature_version: &str) -> DbResult<Vec<(String, String)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT a.id, a.source_path FROM assets a
             LEFT JOIN visual_features f ON f.asset_id = a.id AND f.feature_version = ?2
             WHERE a.library_id = ?1 AND f.asset_id IS NULL ORDER BY a.captured_at, a.file_name",
        )?;
        let rows = stmt.query_map(params![library_id, feature_version], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// Camera/lens distribution for data quality reporting.
    pub fn camera_distribution(&self, library_id: &str) -> DbResult<Vec<(String, i64)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT COALESCE(NULLIF(TRIM(COALESCE(camera_make,'') || ' ' || COALESCE(camera_model,'')), ''), 'Unknown camera') AS cam, COUNT(*)
             FROM assets WHERE library_id = ?1 GROUP BY cam ORDER BY COUNT(*) DESC",
        )?;
        let rows = stmt.query_map([library_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    pub fn capture_date_distribution(&self, library_id: &str) -> DbResult<Vec<(String, i64)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT COALESCE(substr(captured_at, 1, 10), 'unknown') AS day, COUNT(*) FROM assets
             WHERE library_id = ?1 GROUP BY day ORDER BY day",
        )?;
        let rows = stmt.query_map([library_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// Arbitrary JSON scalar per asset used by diagnostics (kept generic on purpose).
    pub fn asset_json_summary(&self, id: &str) -> DbResult<Value> {
        let a = self.get_asset(id)?.ok_or_else(|| DbError::NotFound(id.to_string()))?;
        Ok(serde_json::to_value(a)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_asset(path: &str, hash: &str) -> NewAsset {
        NewAsset {
            source_path: path.to_string(),
            file_name: path.rsplit('/').next().unwrap().to_string(),
            extension: "CR3".to_string(),
            size_bytes: 10,
            fast_hash: hash.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn normalize_path_folds_case_on_windows_style_paths() {
        assert_eq!(normalize_path("D:\\Photos\\A.CR3"), "d:/photos/a.cr3");
        assert_eq!(
            normalize_path("/mnt/photos/A.CR3"),
            if cfg!(windows) { "/mnt/photos/a.cr3" } else { "/mnt/photos/A.CR3" }
        );
    }

    #[test]
    fn upsert_asset_is_identity_stable() {
        let db = Db::open_in_memory().unwrap();
        let lib = db.create_library("L", "folder_sidecars", None, None).unwrap();
        let mut a = new_asset("D:/Photos/IMG_0001.CR3", "h1");
        a.library_id = Some(lib.id.clone());
        let (first, o1) = db.upsert_asset(&a).unwrap();
        assert_eq!(o1, UpsertOutcome::Inserted);
        assert_eq!(first.extension, "cr3");
        a.camera_make = Some("Canon".into());
        let (second, o2) = db.upsert_asset(&a).unwrap();
        assert_eq!(o2, UpsertOutcome::Updated);
        assert_eq!(first.id, second.id);
        assert_eq!(second.camera_make.as_deref(), Some("Canon"));
        // Different hash at same path = different identity (file replaced).
        let (third, _) = db.upsert_asset(&new_asset("D:/Photos/IMG_0001.CR3", "h2")).unwrap();
        assert_ne!(third.id, first.id);
        assert_eq!(db.count_assets(Some(&lib.id)).unwrap(), 1);
        assert_eq!(db.count_assets(None).unwrap(), 2);
        assert!(db.find_asset_by_path("d:\\photos\\img_0001.cr3").unwrap().is_some());
    }

    #[test]
    fn sidecars_and_features() {
        let db = Db::open_in_memory().unwrap();
        let lib = db.create_library("L", "folder_sidecars", None, None).unwrap();
        let mut a = new_asset("/p/a.cr3", "h");
        a.library_id = Some(lib.id.clone());
        let (asset, _) = db.upsert_asset(&a).unwrap();
        let sc = db
            .upsert_sidecar(&NewSidecar {
                asset_id: asset.id.clone(),
                kind: "xmp".into(),
                path: "/p/a.xmp".into(),
                modified_time: None,
                hash: Some("abc".into()),
                parse_status: "parsed".into(),
                parser_version: Some("xmp_parser_v1".into()),
                raw_metadata: None,
                warnings: Some(serde_json::json!([])),
            })
            .unwrap();
        assert_eq!(sc.kind, "xmp");
        assert_eq!(db.sidecars_for_asset(&asset.id).unwrap().len(), 1);
        assert_eq!(db.assets_missing_features(&lib.id, "features_v1").unwrap().len(), 1);
        db.upsert_visual_features(&VisualFeatures {
            asset_id: asset.id.clone(),
            feature_version: "features_v1".into(),
            histogram: serde_json::json!({"bins": [1, 2]}),
            luminance: serde_json::json!({"mean": 0.4}),
            color: serde_json::json!({}),
            sharpness: Some(0.2),
            noise_estimate: Some(0.01),
            clipping: serde_json::json!({}),
            scene_labels: serde_json::json!({"lowLight": false}),
            embedding_artifact_id: None,
            preview_path: None,
            computed_at: now_rfc3339(),
        })
        .unwrap();
        assert!(db.assets_missing_features(&lib.id, "features_v1").unwrap().is_empty());
        let f = db.visual_features(&asset.id, "features_v1").unwrap().unwrap();
        assert_eq!(f.luminance["mean"], 0.4);
        assert_eq!(db.camera_distribution(&lib.id).unwrap()[0].0, "Unknown camera");
    }
}
