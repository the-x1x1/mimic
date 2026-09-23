use serde_json::{json, Map, Value};
use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

/// Allow-listed settings keys with defaults. Unknown keys are rejected so the
/// UI cannot invent state the app does not read.
pub fn defaults() -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("general.theme".into(), json!("plain"));
    m.insert("performance.workerConcurrency".into(), json!(2));
    m.insert("privacy.networkFeatures".into(), json!(false));
    m.insert("updates.channel".into(), json!("stable"));
    m.insert("updates.automatic".into(), json!(true));
    m.insert("diagnostics.includePaths".into(), json!(false));
    m.insert("generation.provider".into(), json!("local"));
    m.insert("generation.localUrl".into(), json!(crate::providers_config::DEFAULT_LOCAL_URL));
    m.insert("generation.localModel".into(), json!(crate::providers_config::DEFAULT_LOCAL_MODEL));
    m.insert("generation.anthropicModel".into(), json!(crate::providers_config::DEFAULT_ANTHROPIC_MODEL));
    // Off by design: with it on, a model sees incoming messages the user did
    // not personally hand it. See `mimic_core::assist`.
    m.insert(mimic_core::assist::SETTING.into(), json!(false));
    // How often connected mailboxes are checked, in minutes; 0 is off. Only
    // matters once a mailbox is connected.
    m.insert(
        mimic_core::sources::imap::INTERVAL_SETTING.into(),
        json!(mimic_core::sources::imap::DEFAULT_INTERVAL_MINUTES),
    );
    // How many days back a thread can be waiting on a reply; 0 is any age.
    // Older ones are left out as gone quiet, counted and shown on request.
    m.insert(mimic_core::db::WITHIN_DAYS_SETTING.into(), json!(mimic_core::db::DEFAULT_WITHIN_DAYS));
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
    check(&key, &value)?;
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

/// Whether `value` may be stored under `key`: a known key, the same kind of
/// value as its default, and in range where there is one.
fn check(key: &str, value: &Value) -> CommandResult<()> {
    let defaults = defaults();
    let Some(default) = defaults.get(key) else {
        return Err(CommandError::new("unknown_setting", format!("{key} is not a Mimic setting")));
    };
    let same_kind = matches!(
        (default, value),
        (Value::Bool(_), Value::Bool(_)) | (Value::Number(_), Value::Number(_)) | (Value::String(_), Value::String(_))
    );
    if !same_kind {
        return Err(CommandError::new("invalid_setting", format!("{key} expects a {}", kind(default))));
    }
    if key == "updates.channel" && !matches!(value.as_str(), Some("stable") | Some("beta")) {
        return Err(CommandError::new("invalid_setting", "updates.channel must be stable or beta"));
    }
    if key == mimic_core::db::WITHIN_DAYS_SETTING
        && !value.as_i64().is_some_and(|d| (0..=mimic_core::db::MAX_WITHIN_DAYS).contains(&d))
    {
        return Err(CommandError::new(
            "invalid_setting",
            format!("{key} is a whole number of days up to {}, 0 for any age", mimic_core::db::MAX_WITHIN_DAYS),
        ));
    }
    Ok(())
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        _ => "value",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_waiting_window_takes_whole_days_in_range_and_nothing_else() {
        let key = mimic_core::db::WITHIN_DAYS_SETTING;
        assert_eq!(defaults().get(key), Some(&json!(mimic_core::db::DEFAULT_WITHIN_DAYS)));
        for ok in [json!(0), json!(7), json!(30), json!(mimic_core::db::MAX_WITHIN_DAYS)] {
            assert!(check(key, &ok).is_ok(), "{ok}");
        }
        for bad in [json!(-1), json!(7.5), json!(mimic_core::db::MAX_WITHIN_DAYS + 1), json!("30"), json!(true)] {
            assert!(check(key, &bad).is_err(), "{bad}");
        }
        assert!(check("waiting.somethingElse", &json!(1)).is_err());
    }
}
