use rusqlite::{params, OptionalExtension, Row};
use serde_json::Value;

use super::models::json_col_or_default;
use super::{Db, DbError, DbResult, ModelVersion, StyleProfile, TrainingSet};
use crate::ids::{new_id, now_rfc3339};

fn map_style(r: &Row<'_>) -> rusqlite::Result<StyleProfile> {
    Ok(StyleProfile {
        id: r.get(0)?,
        name: r.get(1)?,
        description: r.get(2)?,
        created_at: r.get(3)?,
        updated_at: r.get(4)?,
        active_model_version_id: r.get(5)?,
        status: r.get(6)?,
        library_ids: Vec::new(),
    })
}

const STYLE_COLS: &str = "id, name, description, created_at, updated_at, active_model_version_id, status";
const MV_COLS: &str = "id, style_profile_id, semantic_version, model_type, feature_schema_version, edit_schema_version, training_set_id, training_config_json, metrics_json, artifact_manifest_json, created_at, status, is_active";

fn map_mv(r: &Row<'_>) -> rusqlite::Result<ModelVersion> {
    Ok(ModelVersion {
        id: r.get(0)?,
        style_profile_id: r.get(1)?,
        semantic_version: r.get(2)?,
        model_type: r.get(3)?,
        feature_schema_version: r.get(4)?,
        edit_schema_version: r.get(5)?,
        training_set_id: r.get(6)?,
        training_config: json_col_or_default(r.get(7)?),
        metrics: json_col_or_default(r.get(8)?),
        artifact_manifest: json_col_or_default(r.get(9)?),
        created_at: r.get(10)?,
        status: r.get(11)?,
        is_active: r.get::<_, i64>(12)? != 0,
    })
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewModelVersion {
    pub style_profile_id: String,
    pub semantic_version: String,
    pub model_type: String,
    pub feature_schema_version: String,
    pub edit_schema_version: String,
    pub training_set_id: Option<String>,
    pub training_config: Value,
    pub metrics: Value,
    pub artifact_manifest: Value,
    pub status: String,
}

impl Db {
    pub fn create_style_profile(&self, name: &str, description: Option<&str>) -> DbResult<StyleProfile> {
        let id = new_id();
        let now = now_rfc3339();
        self.conn().execute(
            "INSERT INTO style_profiles(id, name, description, created_at, updated_at, status) VALUES (?1,?2,?3,?4,?4,'empty')",
            params![id, name, description, now],
        )?;
        self.get_style_profile(&id)?.ok_or(DbError::NotFound(id))
    }

    fn attach_libraries(&self, mut s: StyleProfile) -> DbResult<StyleProfile> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT library_id FROM style_profile_libraries WHERE style_profile_id = ?1 ORDER BY added_at")?;
        s.library_ids = stmt.query_map([&s.id], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(s)
    }

    pub fn get_style_profile(&self, id: &str) -> DbResult<Option<StyleProfile>> {
        let row = self
            .conn()
            .query_row(&format!("SELECT {STYLE_COLS} FROM style_profiles WHERE id = ?1"), [id], map_style)
            .optional()?;
        row.map(|s| self.attach_libraries(s)).transpose()
    }

    pub fn list_style_profiles(&self) -> DbResult<Vec<StyleProfile>> {
        let rows: Vec<StyleProfile> = {
            let conn = self.conn();
            let mut stmt = conn.prepare(&format!("SELECT {STYLE_COLS} FROM style_profiles ORDER BY created_at"))?;
            let rows = stmt.query_map([], map_style)?;
            rows.collect::<Result<_, _>>()?
        };
        rows.into_iter().map(|s| self.attach_libraries(s)).collect()
    }

    pub fn link_style_library(&self, style_id: &str, library_id: &str) -> DbResult<()> {
        self.conn().execute(
            "INSERT OR IGNORE INTO style_profile_libraries(style_profile_id, library_id, added_at) VALUES (?1, ?2, ?3)",
            params![style_id, library_id, now_rfc3339()],
        )?;
        self.touch_style(style_id)
    }

    pub fn set_style_status(&self, style_id: &str, status: &str) -> DbResult<()> {
        self.conn().execute(
            "UPDATE style_profiles SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![style_id, status, now_rfc3339()],
        )?;
        Ok(())
    }

    fn touch_style(&self, style_id: &str) -> DbResult<()> {
        self.conn()
            .execute("UPDATE style_profiles SET updated_at = ?2 WHERE id = ?1", params![style_id, now_rfc3339()])?;
        Ok(())
    }

    pub fn delete_style_profile(&self, id: &str) -> DbResult<()> {
        let n = self.conn().execute("DELETE FROM style_profiles WHERE id = ?1", [id])?;
        if n == 0 {
            return Err(DbError::NotFound(id.to_string()));
        }
        Ok(())
    }

    // ----- training sets -----------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn create_training_set(
        &self,
        style_id: &str,
        source_query: &Value,
        counts: (i64, i64, i64, i64, i64),
        split_strategy: &str,
        fingerprint: Option<&str>,
    ) -> DbResult<TrainingSet> {
        let id = new_id();
        self.conn().execute(
            "INSERT INTO training_sets(id, style_profile_id, source_query_json, asset_count, valid_pair_count, train_count,
               validation_count, holdout_count, split_strategy, fingerprint, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![id, style_id, source_query.to_string(), counts.0, counts.1, counts.2, counts.3, counts.4, split_strategy, fingerprint, now_rfc3339()],
        )?;
        Ok(self.conn().query_row(
            "SELECT id, style_profile_id, source_query_json, asset_count, valid_pair_count, train_count, validation_count,
                    holdout_count, split_strategy, fingerprint, created_at FROM training_sets WHERE id = ?1",
            [&id],
            |r| {
                Ok(TrainingSet {
                    id: r.get(0)?,
                    style_profile_id: r.get(1)?,
                    source_query: json_col_or_default(r.get(2)?),
                    asset_count: r.get(3)?,
                    valid_pair_count: r.get(4)?,
                    train_count: r.get(5)?,
                    validation_count: r.get(6)?,
                    holdout_count: r.get(7)?,
                    split_strategy: r.get(8)?,
                    fingerprint: r.get(9)?,
                    created_at: r.get(10)?,
                })
            },
        )?)
    }

    // ----- model versions ----------------------------------------------

    pub fn create_model_version(&self, mv: &NewModelVersion) -> DbResult<ModelVersion> {
        let id = new_id();
        self.conn().execute(
            "INSERT INTO model_versions(id, style_profile_id, semantic_version, model_type, feature_schema_version,
               edit_schema_version, training_set_id, training_config_json, metrics_json, artifact_manifest_json, created_at, status, is_active)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,0)",
            params![
                id, mv.style_profile_id, mv.semantic_version, mv.model_type, mv.feature_schema_version, mv.edit_schema_version,
                mv.training_set_id, mv.training_config.to_string(), mv.metrics.to_string(), mv.artifact_manifest.to_string(),
                now_rfc3339(), mv.status
            ],
        )?;
        self.get_model_version(&id)?.ok_or(DbError::NotFound(id))
    }

    /// Immutable: only status/metrics/manifest of a `training` version may be finalized once.
    pub fn finalize_model_version(
        &self,
        id: &str,
        status: &str,
        metrics: &Value,
        manifest: &Value,
    ) -> DbResult<ModelVersion> {
        let n = self.conn().execute(
            "UPDATE model_versions SET status = ?2, metrics_json = ?3, artifact_manifest_json = ?4 WHERE id = ?1 AND status = 'training'",
            params![id, status, metrics.to_string(), manifest.to_string()],
        )?;
        if n == 0 {
            return Err(DbError::Invalid(format!(
                "model version {id} is not in 'training' state; versions are immutable"
            )));
        }
        self.get_model_version(id)?.ok_or_else(|| DbError::NotFound(id.to_string()))
    }

    pub fn set_model_version_training_set(&self, id: &str, training_set_id: &str) -> DbResult<()> {
        self.conn()
            .execute("UPDATE model_versions SET training_set_id = ?2 WHERE id = ?1", params![id, training_set_id])?;
        Ok(())
    }

    /// Record the configuration the trainer actually used (allowed only while training).
    pub fn set_model_version_training_config(&self, id: &str, config: &Value, model_type: &str) -> DbResult<()> {
        self.conn().execute(
            "UPDATE model_versions SET training_config_json = ?2, model_type = ?3 WHERE id = ?1 AND status = 'training'",
            params![id, config.to_string(), model_type],
        )?;
        Ok(())
    }

    pub fn archive_model_version(&self, id: &str) -> DbResult<()> {
        let mv = self.get_model_version(id)?.ok_or_else(|| DbError::NotFound(id.to_string()))?;
        if mv.is_active {
            return Err(DbError::Invalid("cannot archive the active model version; activate another first".into()));
        }
        self.conn()
            .execute("UPDATE model_versions SET status = 'archived' WHERE id = ?1 AND status = 'ready'", [id])?;
        Ok(())
    }

    pub fn get_model_version(&self, id: &str) -> DbResult<Option<ModelVersion>> {
        Ok(self
            .conn()
            .query_row(&format!("SELECT {MV_COLS} FROM model_versions WHERE id = ?1"), [id], map_mv)
            .optional()?)
    }

    pub fn list_model_versions(&self, style_id: &str) -> DbResult<Vec<ModelVersion>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {MV_COLS} FROM model_versions WHERE style_profile_id = ?1 ORDER BY created_at DESC"
        ))?;
        let rows = stmt.query_map([style_id], map_mv)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    pub fn next_model_semver(&self, style_id: &str) -> DbResult<String> {
        let versions = self.list_model_versions(style_id)?;
        let max_minor = versions
            .iter()
            .filter_map(|v| {
                let mut it = v.semantic_version.split('.');
                let major: u64 = it.next()?.parse().ok()?;
                let minor: u64 = it.next()?.parse().ok()?;
                Some((major, minor))
            })
            .max();
        Ok(match max_minor {
            Some((major, minor)) => format!("{major}.{}.0", minor + 1),
            None => "1.0.0".to_string(),
        })
    }

    /// Activate a version: exactly one active per style; only `ready` versions.
    pub fn activate_model_version(&self, id: &str) -> DbResult<ModelVersion> {
        let mv = self.get_model_version(id)?.ok_or_else(|| DbError::NotFound(id.to_string()))?;
        if mv.status != "ready" {
            return Err(DbError::Invalid(format!(
                "model version {id} is '{}', only ready versions can be activated",
                mv.status
            )));
        }
        self.transaction(|tx| {
            tx.execute("UPDATE model_versions SET is_active = 0 WHERE style_profile_id = ?1", [&mv.style_profile_id])?;
            tx.execute("UPDATE model_versions SET is_active = 1 WHERE id = ?1", [id])?;
            tx.execute(
                "UPDATE style_profiles SET active_model_version_id = ?2, status = 'ready', updated_at = ?3 WHERE id = ?1",
                params![mv.style_profile_id, id, now_rfc3339()],
            )?;
            Ok(())
        })?;
        self.get_model_version(id)?.ok_or_else(|| DbError::NotFound(id.to_string()))
    }

    pub fn active_model_version(&self, style_id: &str) -> DbResult<Option<ModelVersion>> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {MV_COLS} FROM model_versions WHERE style_profile_id = ?1 AND is_active = 1"),
                [style_id],
                map_mv,
            )
            .optional()?)
    }

    pub fn add_model_artifact(
        &self,
        model_version_id: &str,
        kind: &str,
        path: &str,
        hash: &str,
        size_bytes: i64,
        format: &str,
    ) -> DbResult<String> {
        let id = new_id();
        self.conn().execute(
            "INSERT INTO model_artifacts(id, model_version_id, kind, path, hash, size_bytes, format, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![id, model_version_id, kind, path, hash, size_bytes, format, now_rfc3339()],
        )?;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mv(style: &str, ver: &str) -> NewModelVersion {
        NewModelVersion {
            style_profile_id: style.into(),
            semantic_version: ver.into(),
            model_type: "hybrid_knn_residual".into(),
            feature_schema_version: "features_v1".into(),
            edit_schema_version: "1.0".into(),
            training_set_id: None,
            training_config: json!({"seed": 42}),
            metrics: json!({}),
            artifact_manifest: json!({}),
            status: "training".into(),
        }
    }

    #[test]
    fn versions_are_immutable_and_single_active() {
        let db = Db::open_in_memory().unwrap();
        let style = db.create_style_profile("Wedding Natural", None).unwrap();
        assert_eq!(db.next_model_semver(&style.id).unwrap(), "1.0.0");
        let v1 = db.create_model_version(&mv(&style.id, "1.0.0")).unwrap();
        assert!(db.activate_model_version(&v1.id).is_err(), "training versions cannot be activated");
        db.finalize_model_version(&v1.id, "ready", &json!({"holdout": {"mae": 0.1}}), &json!({})).unwrap();
        assert!(
            db.finalize_model_version(&v1.id, "ready", &json!({}), &json!({})).is_err(),
            "finalize twice must fail"
        );
        db.activate_model_version(&v1.id).unwrap();
        assert_eq!(db.next_model_semver(&style.id).unwrap(), "1.1.0");
        let v2 = db.create_model_version(&mv(&style.id, "1.1.0")).unwrap();
        db.finalize_model_version(&v2.id, "ready", &json!({}), &json!({})).unwrap();
        db.activate_model_version(&v2.id).unwrap();
        let versions = db.list_model_versions(&style.id).unwrap();
        assert_eq!(versions.iter().filter(|v| v.is_active).count(), 1);
        assert_eq!(db.active_model_version(&style.id).unwrap().unwrap().id, v2.id);
        assert!(db.archive_model_version(&v2.id).is_err());
        db.archive_model_version(&v1.id).unwrap();
        assert_eq!(db.get_model_version(&v1.id).unwrap().unwrap().status, "archived");
        // Rollback: re-activate v1 fails because archived; ready-only rule.
        assert!(db.activate_model_version(&v1.id).is_err());
        let s = db.get_style_profile(&style.id).unwrap().unwrap();
        assert_eq!(s.active_model_version_id.as_deref(), Some(v2.id.as_str()));
        assert!(db.create_model_version(&mv(&style.id, "1.1.0")).is_err(), "duplicate semver must fail");
    }

    #[test]
    fn style_library_links() {
        let db = Db::open_in_memory().unwrap();
        let style = db.create_style_profile("S", Some("d")).unwrap();
        let lib = db.create_library("L", "folder_sidecars", None, None).unwrap();
        db.link_style_library(&style.id, &lib.id).unwrap();
        db.link_style_library(&style.id, &lib.id).unwrap();
        assert_eq!(db.get_style_profile(&style.id).unwrap().unwrap().library_ids, vec![lib.id.clone()]);
        assert_eq!(db.list_style_profiles().unwrap().len(), 1);
        db.delete_style_profile(&style.id).unwrap();
        assert!(db.list_style_profiles().unwrap().is_empty());
    }
}
