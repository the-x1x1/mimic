//! Model providers: what is configured, whether it answers, and credentials.

use serde::Serialize;
use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderState {
    pub providers: Vec<mimic_core::providers::ProviderInfo>,
    pub active: Option<String>,
    /// Which credential keys have a value. Never the values.
    pub configured_secrets: Vec<String>,
}

#[tauri::command]
pub async fn get_provider_state(state: State<'_, SharedState>) -> CommandResult<ProviderState> {
    let registry = state.providers.read().unwrap_or_else(|p| p.into_inner());
    let chosen = state.db.get_setting::<String>("generation.provider")?;
    Ok(ProviderState {
        providers: registry.list(),
        active: chosen.or_else(|| registry.default_id()),
        configured_secrets: state.secrets.keys(),
    })
}

#[tauri::command]
pub async fn set_active_provider(state: State<'_, SharedState>, provider_id: String) -> CommandResult<()> {
    let registry = state.providers.read().unwrap_or_else(|p| p.into_inner());
    registry.get(&provider_id)?;
    drop(registry);
    state.db.set_setting("generation.provider", &provider_id)?;
    Ok(())
}

/// Store or clear a credential. An empty value clears it.
#[tauri::command]
pub async fn set_provider_secret(state: State<'_, SharedState>, key: String, value: String) -> CommandResult<()> {
    if !key.starts_with("provider.") {
        return Err(CommandError::new("invalid", "credential keys start with provider."));
    }
    state.secrets.set(&key, &value)?;
    state.rebuild_providers();
    Ok(())
}

#[tauri::command]
pub async fn check_provider(state: State<'_, SharedState>, provider_id: String) -> CommandResult<()> {
    let provider = {
        let registry = state.providers.read().unwrap_or_else(|p| p.into_inner());
        registry.get(&provider_id)?
    };
    tauri::async_runtime::spawn_blocking(move || provider.health())
        .await
        .map_err(|e| CommandError::new("internal", e.to_string()))??;
    Ok(())
}
