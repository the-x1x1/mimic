//! Model providers: what is configured, whether it answers, and credentials.

use serde::Serialize;
use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::secrets::{CredentialState, FileSecretStore};
use crate::SharedState;
use mimic_core::providers::ProviderRegistry;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderState {
    pub providers: Vec<mimic_core::providers::ProviderInfo>,
    pub active: Option<String>,
    /// Which credential keys have a value that opens here. Never the values.
    pub configured_secrets: Vec<String>,
    /// How saved credentials are protected, as the store reports it, and
    /// which saved ones do not open on this account.
    pub credentials: CredentialState,
}

fn provider_state(registry: &ProviderRegistry, chosen: Option<String>, secrets: &FileSecretStore) -> ProviderState {
    ProviderState {
        providers: registry.list(),
        active: chosen.or_else(|| registry.default_id()),
        configured_secrets: secrets.keys(),
        credentials: secrets.credential_state(),
    }
}

#[tauri::command]
pub async fn get_provider_state(state: State<'_, SharedState>) -> CommandResult<ProviderState> {
    let registry = state.providers.read().unwrap_or_else(|p| p.into_inner());
    let chosen = state.db.get_setting::<String>("generation.provider")?;
    Ok(provider_state(&registry, chosen, &state.secrets))
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
    // Nothing is saved unsealed: if Windows will not lock it to the account,
    // the save fails and says so.
    state
        .secrets
        .set(&key, &value)
        .map_err(|e| CommandError::new("secrets", format!("I couldn't save that key: {e}")))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::check_fixture;

    #[test]
    fn provider_state_says_how_credentials_are_kept_and_matches_its_fixture() {
        let dir = tempfile::tempdir().unwrap();
        // One key saved here, and a mailbox password saved on another account.
        std::fs::write(
            dir.path().join("secrets.json"),
            r#"{"format": 2, "values": {"imap:elsewhere": {"protection": "account", "sealed": "00ff"}}}"#,
        )
        .unwrap();
        let secrets = FileSecretStore::open(dir.path()).unwrap();
        secrets.set("provider.anthropic.apiKey", "sk-fixture").unwrap();
        let db = mimic_core::db::Db::open_in_memory().unwrap();
        let registry = ProviderRegistry::new(crate::providers_config::build(&db, &secrets));

        let state = provider_state(&registry, None, &secrets);
        assert_eq!(state.configured_secrets, vec!["provider.anthropic.apiKey"]);
        assert_eq!(state.credentials.locked, vec!["imap:elsewhere"]);
        // Whatever this build does, the state reports that and nothing better.
        #[cfg(windows)]
        assert_eq!(state.credentials.protection, crate::secrets::Protection::Account);
        #[cfg(not(windows))]
        assert_eq!(state.credentials.protection, crate::secrets::Protection::File);
        assert!(!state.credentials.unsealed_left);
        assert_eq!(state.credentials.unreadable, None);
        let json = serde_json::to_value(&state).unwrap();
        assert!(!json.to_string().contains("sk-fixture"), "no value ever leaves the store");
        check_fixture("provider_state.json", &json);
    }
}
