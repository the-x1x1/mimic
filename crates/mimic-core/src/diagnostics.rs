//! Diagnostics bundle (spec §28). Contains no tokens and, unless the user
//! opts in, no full user paths.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::db::Db;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsBundle {
    pub generated_at: String,
    pub app_version: String,
    pub os: String,
    pub arch: String,
    pub db_schema_version: i64,
    pub table_counts: Vec<(String, i64)>,
    pub engine: Value,
    pub bridge: Value,
    pub update_state: Value,
    pub recent_errors: Vec<Value>,
    pub job_summaries: Vec<Value>,
    pub include_paths: bool,
}

pub fn build_bundle(
    db: &Db,
    engine: Value,
    bridge: Value,
    include_paths: bool,
) -> Result<DiagnosticsBundle, crate::db::DbError> {
    let recent_errors = db
        .recent_events(50, Some("warn"))?
        .into_iter()
        .map(|e| {
            let payload: Value = serde_json::from_str(&e.payload_json).unwrap_or(Value::Null);
            json!({
                "at": e.created_at,
                "level": e.level,
                "category": e.category,
                "type": e.event_type,
                "entity": e.entity_type,
                "payload": if include_paths { payload } else { redact_paths(payload) },
            })
        })
        .collect();
    let job_summaries = db
        .list_jobs(25, false)?
        .into_iter()
        .map(|j| {
            json!({
                "id": j.id,
                "type": j.kind,
                "status": j.status,
                "phase": j.phase,
                "progress": [j.progress_current, j.progress_total],
                "createdAt": j.created_at,
                "completedAt": j.completed_at,
                "error": j.error.map(|e| if include_paths { e } else { redact_paths(e) }),
            })
        })
        .collect();
    let mut bridge = bridge;
    strip_key(&mut bridge, "token");
    Ok(DiagnosticsBundle {
        generated_at: crate::ids::now_rfc3339(),
        app_version: crate::APP_VERSION.to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        db_schema_version: db.schema_version()?,
        table_counts: db.table_counts()?,
        engine,
        bridge,
        update_state: serde_json::to_value(db.update_state()?)?,
        recent_errors,
        job_summaries,
        include_paths,
    })
}

/// Replace anything that looks like an absolute path with its file name.
pub fn redact_paths(v: Value) -> Value {
    match v {
        Value::String(s) => Value::String(redact_str(&s)),
        Value::Array(a) => Value::Array(a.into_iter().map(redact_paths).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, redact_paths(v))).collect()),
        other => other,
    }
}

fn redact_str(s: &str) -> String {
    let looks_like_path = s.contains(":\\") || s.contains(":/") || s.starts_with('/') || s.starts_with("\\\\");
    if !looks_like_path || s.len() < 4 {
        return s.to_string();
    }
    let name = s.rsplit(['/', '\\']).next().unwrap_or("");
    format!("<path>/{name}")
}

fn strip_key(v: &mut Value, key: &str) {
    match v {
        Value::Object(o) => {
            o.remove(key);
            for child in o.values_mut() {
                strip_key(child, key);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|c| strip_key(c, key)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_has_no_token_and_redacts_paths() {
        let db = Db::open_in_memory().unwrap();
        db.log_event(&crate::db::NewEvent::error("engine", "boom", json!({"path": "D:\\Photos\\Wedding\\IMG_1.CR3"})))
            .unwrap();
        let bundle = build_bundle(
            &db,
            json!({"state": "ready"}),
            json!({"token": "secret", "connection": {"token": "x", "catalogFingerprint": "c"}}),
            false,
        )
        .unwrap();
        let text = serde_json::to_string(&bundle).unwrap();
        assert!(!text.contains("secret"));
        assert!(!text.contains("D:\\\\Photos"));
        assert!(text.contains("<path>/IMG_1.CR3"));
        assert_eq!(bundle.app_version, crate::APP_VERSION);
        let with_paths = build_bundle(&db, json!({}), json!({}), true).unwrap();
        assert!(serde_json::to_string(&with_paths).unwrap().contains("Wedding"));
    }
}
