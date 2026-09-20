use serde_json::{json, Map, Value};
use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

/// Allow-listed settings keys with defaults. Unknown keys are rejected so the
/// UI cannot invent state the app does not read.
pub fn defaults() -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("general.theme".into(), json!("dark"));
    m.insert("performance.workerConcurrency".into(), json!(2));
    m.insert("privacy.networkFeatures".into(), json!(false));
    m.insert("updates.channel".into(), json!("stable"));
    m.insert("updates.automatic".into(), json!(true));
    m.insert("diagnostics.includePaths".into(), json!(false));
    m.insert("generation.provider".into(), json!("local"));
    m.insert("generation.localUrl".into(), json!(crate::providers_config::DEFAULT_LOCAL_URL));
    m.insert("generation.localModel".into(), json!(crate::providers_config::DEFAULT_LOCAL_MODEL));
    m.insert("generation.anthropicModel".into(), json!(crate::providers_config::DEFAULT_ANTHROPIC_MODEL));
    m.insert("onboarding.completed".into(), json!(false));
    m
}

#[tauri::command]
pub async fn get_settings(state: State<'_, SharedState>) -> CommandResult<Map<String, Value>> {
    let mut out = defaults();
    for (k, v) in state.db.all_settings()? {
        if out.contains_key(&k) {
            out.insert(k, v);
        }
    }
    Ok(out)
}

#[tauri::command]
pub async fn set_setting(
    state: State<'_, SharedState>,
    key: String,
    value: Value,
) -> CommandResult<Map<String, Value>> {
    let defaults = defaults();
    let Some(default) = defaults.get(&key) else {
        return Err(CommandError::new("unknown_setting", format!("{key} is not a Mimic setting")));
    };
    let same_kind = matches!(
        (default, &value),
        (Value::Bool(_), Value::Bool(_)) | (Value::Number(_), Value::Number(_)) | (Value::String(_), Value::String(_))
    );
    if !same_kind {
        return Err(CommandError::new("invalid_setting", format!("{key} expects a {}", kind(default))));
    }
    if key == "updates.channel" && !matches!(value.as_str(), Some("stable") | Some("beta")) {
        return Err(CommandError::new("invalid_setting", "updates.channel must be stable or beta"));
    }
    state.db.set_setting(&key, &value)?;
    if key == "updates.channel" {
        let mut st = state.db.update_state()?;
        st.channel = value.as_str().unwrap_or("stable").to_string();
        state.db.save_update_state(&st)?;
    }
    if key.starts_with("generation.") {
        state.rebuild_providers();
    }
    get_settings(state).await
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        _ => "value",
    }
}
