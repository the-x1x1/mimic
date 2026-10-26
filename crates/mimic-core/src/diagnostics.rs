//! Diagnostics bundle. Contains no credentials, no message bodies and,
//! unless the user opts in, no full user paths. Since 0.10.0-alpha.38 it
//! holds what was said to a mail server when something failed
//! (`sources::transcript`): with each connected mailbox's last error, and in
//! the recent errors for a failed send, sign-in or look at a mailbox.

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
    pub providers: Value,
    pub update_state: Value,
    pub recent_errors: Vec<Value>,
    pub job_summaries: Vec<Value>,
    /// Each connected mailbox's state and last error, with what was said to
    /// its server when that check failed; not the name it is listed under
    /// (its address), though a server's answer may still say it.
    #[serde(default)]
    pub mailboxes: Vec<Value>,
    pub include_paths: bool,
}

pub fn build_bundle(
    db: &Db,
    engine: Value,
    providers: Value,
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
    let mailboxes = db
        .list_sources()?
        .into_iter()
        .filter(|s| s.connector == crate::sources::imap::CONNECTOR)
        .map(|s| json!({ "status": s.status, "lastError": s.last_error }))
        .collect();
    let mut providers = providers;
    for secret in ["token", "apiKey", "api_key", "password", "secret"] {
        strip_key(&mut providers, secret);
    }
    Ok(DiagnosticsBundle {
        generated_at: crate::ids::now_rfc3339(),
        app_version: crate::APP_VERSION.to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        db_schema_version: db.schema_version()?,
        table_counts: db.table_counts()?,
        engine,
        providers,
        update_state: serde_json::to_value(db.update_state()?)?,
        recent_errors,
        job_summaries,
        mailboxes,
        include_paths,
    })
}

/// Replace anything that looks like an absolute path with its file name —
/// except what a mail server was told and said (`transcript`), where a
/// `https://` link is the server's own and no path of the user's appears.
pub fn redact_paths(v: Value) -> Value {
    match v {
        Value::String(s) => Value::String(redact_str(&s)),
        Value::Array(a) => Value::Array(a.into_iter().map(redact_paths).collect()),
        Value::Object(o) => Value::Object(
            o.into_iter().map(|(k, v)| if k == "transcript" { (k, v) } else { (k, redact_paths(v)) }).collect(),
        ),
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
    fn bundle_has_no_credentials_and_redacts_paths() {
        let db = Db::open_in_memory().unwrap();
        db.log_event(&crate::db::NewEvent::error(
            "engine",
            "boom",
            json!({"path": "D:\\Exports\\Mailbox\\inbox.mbox"}),
        ))
        .unwrap();
        let bundle = build_bundle(
            &db,
            json!({"state": "ready"}),
            json!({"active": "local", "credentials": {"apiKey": "secret", "nested": {"token": "x"}}}),
            false,
        )
        .unwrap();
        let text = serde_json::to_string(&bundle).unwrap();
        assert!(!text.contains("secret"));
        assert!(!text.contains("D:\\\\Exports"));
        assert!(text.contains("<path>/inbox.mbox"));
        assert_eq!(bundle.app_version, crate::APP_VERSION);
        let with_paths = build_bundle(&db, json!({}), json!({}), true).unwrap();
        assert!(serde_json::to_string(&with_paths).unwrap().contains("Mailbox"));
    }

    #[test]
    fn a_failing_mailbox_comes_with_what_its_server_said_and_not_its_name() {
        let db = Db::open_in_memory().unwrap();
        let source = db
            .create_source(&crate::db::NewSource {
                connector: crate::sources::imap::CONNECTOR.into(),
                name: "c@example.com".into(),
                channel: "email".into(),
                location: None,
                config: json!({}),
            })
            .unwrap();
        let said = json!([
            "imap < m1 NO [AUTHENTICATIONFAILED] Invalid credentials",
            "smtp < 535-5.7.8 https://support.google.com/mail/?p=BadCredentials"
        ]);
        db.log_event(&crate::db::NewEvent::warn("mail", "send_failed", json!({ "transcript": said.clone() }))).unwrap();
        db.set_source_status(&source.id, "failed", Some(&json!({ "message": "wrong password", "transcript": said })))
            .unwrap();
        let bundle = build_bundle(&db, json!({}), json!({}), false).unwrap();
        assert_eq!(bundle.mailboxes.len(), 1);
        assert_eq!(bundle.mailboxes[0]["status"], "failed");
        assert_eq!(bundle.mailboxes[0]["lastError"]["transcript"], said);
        assert!(!serde_json::to_string(&bundle).unwrap().contains("c@example.com"));
        assert_eq!(bundle.recent_errors[0]["payload"]["transcript"], said, "a server's link is not a path");
    }
}
