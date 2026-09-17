//! Updater state persistence and the "is it safe to install now?" guard.
//! The actual check/download/install runs through the Tauri updater plugin
//! from the frontend (signature verification is enforced by the plugin).

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

#[tauri::command]
pub async fn get_update_state(state: State<'_, SharedState>) -> CommandResult<mimic_core::db::UpdateState> {
    Ok(state.db.update_state()?)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePrefs {
    pub channel: Option<String>,
    pub automatic: Option<bool>,
}

#[tauri::command]
pub async fn set_update_preferences(
    state: State<'_, SharedState>,
    prefs: UpdatePrefs,
) -> CommandResult<mimic_core::db::UpdateState> {
    if let Some(ch) = &prefs.channel {
        if !matches!(ch.as_str(), "stable" | "beta") {
            return Err(CommandError::new("invalid", "channel must be stable or beta"));
        }
        state.db.set_setting("updates.channel", ch)?;
        let mut st = state.db.update_state()?;
        st.channel = ch.clone();
        state.db.save_update_state(&st)?;
    }
    if let Some(auto) = prefs.automatic {
        state.db.set_setting("updates.automatic", &auto)?;
    }
    Ok(state.db.update_state()?)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckRecord {
    pub latest_seen_version: Option<String>,
    pub staged_version: Option<String>,
    pub result: String,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn record_update_check(
    state: State<'_, SharedState>,
    record: UpdateCheckRecord,
) -> CommandResult<mimic_core::db::UpdateState> {
    let mut st = state.db.update_state()?;
    st.last_checked_at = Some(mimic_core::ids::now_rfc3339());
    if record.latest_seen_version.is_some() {
        st.latest_seen_version = record.latest_seen_version;
    }
    st.staged_version = record.staged_version;
    st.last_update_result = Some(record.result.clone());
    st.update_error = record.error;
    state.db.save_update_state(&st)?;
    let _ = state.db.log_event(&mimic_core::db::NewEvent::info(
        "update",
        "checked",
        serde_json::json!({"result": record.result, "latest": st.latest_seen_version}),
    ));
    Ok(st)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallGuard {
    pub allowed: bool,
    pub reason: String,
    pub active_jobs: usize,
}

/// Installing must never interrupt ingest, training, prediction or a Lightroom apply.
#[tauri::command]
pub async fn can_install_update_now(state: State<'_, SharedState>) -> CommandResult<InstallGuard> {
    let active = state.db.list_jobs(50, true)?;
    if active.is_empty() {
        Ok(InstallGuard { allowed: true, reason: "Mimic is idle.".into(), active_jobs: 0 })
    } else {
        let kinds: Vec<&str> = active.iter().map(|j| j.kind.as_str()).collect();
        Ok(InstallGuard {
            allowed: false,
            reason: format!("Waiting for {} active job(s) to finish: {}", active.len(), kinds.join(", ")),
            active_jobs: active.len(),
        })
    }
}
