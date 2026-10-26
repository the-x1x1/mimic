//! A Slack workspace's direct messages, connected with the user's own token
//! (0.10.0-alpha.64). The token goes to the secret store, never the
//! database; it is tried before anything is kept.

use mimic_core::db::IdentifierKind;
use mimic_core::sources::slack::{self, Identity, SlackApi, SlackError};
use serde_json::json;
use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

const NOT_A_USER_TOKEN: &str = "That isn't a user token. Copy the User OAuth Token from your Slack app's OAuth & Permissions page: it starts with xoxp-.";

fn slack_error(e: SlackError) -> CommandError {
    CommandError::new("slack", e.to_string())
}

/// Whose `token` is, once Slack has answered one of everything a check asks.
async fn checked(token: &str) -> CommandResult<Identity> {
    if !slack::is_user_token(token) {
        return Err(CommandError::new("token", NOT_A_USER_TOKEN));
    }
    let token = token.to_string();
    tauri::async_runtime::spawn_blocking(move || SlackApi::new(&token)?.check())
        .await
        .map_err(|e| CommandError::new("internal", e.to_string()))?
        .map_err(slack_error)
}

fn check_soon(state: &crate::AppState, source_id: &str) -> CommandResult<()> {
    let queued = state.db.list_jobs(500, true)?.into_iter().any(|j| {
        j.kind == slack::JOB_KIND && j.status == "queued" && j.payload["sourceId"].as_str() == Some(source_id)
    });
    if !queued {
        state.jobs.enqueue(slack::JOB_KIND, json!({ "sourceId": source_id }))?;
    }
    Ok(())
}

/// Connect a workspace: try the token, make its account one of the user's,
/// keep the token in the secret store, and start the first check.
#[tauri::command]
pub async fn connect_slack(state: State<'_, SharedState>, token: String) -> CommandResult<mimic_core::db::Source> {
    crate::commands::sources::refuse_without_identity(&state)?;
    let token = token.trim().to_string();
    let me = checked(&token).await?;
    if slack::connected_as(&state.db, &me)?.is_some() {
        return Err(CommandError::new(
            "exists",
            format!(
                "{} in {} is already connected. To give me a new token for it, use New token beside it.",
                me.user, me.team
            ),
        ));
    }
    // The token is the user's own, so whoever it says it is, is them.
    state.db.add_user_identifier(IdentifierKind::AccountId, &slack::account_id(&me.team_id, &me.user_id))?;
    crate::commands::people::reconcile_identity(&state.db, &state.jobs, "connect_slack");
    let new = slack::new_source(&me).map_err(|e| CommandError::new("internal", e.to_string()))?;
    let source = state.db.create_source(&new)?;
    if let Err(e) = state.secrets.set(&slack::secret_key(&source.id), &token) {
        // Without it the workspace can never be read; nothing is left
        // behind that looks connected and isn't.
        let _ = state.db.delete_source_and_contents(&source.id);
        return Err(CommandError::new("secrets", format!("I couldn't store the token: {e}")));
    }
    state.db.set_source_status(&source.id, "ready", None)?;
    check_soon(&state, &source.id)?;
    Ok(state.db.get_source(&source.id)?.unwrap_or(source))
}

/// Try a connected workspace as a check would, sending nothing
/// (0.10.0-alpha.69): its token, one direct message, its newest message and
/// who the user is. A sentence saying what was found, or why not.
#[tauri::command]
pub async fn test_slack(state: State<'_, SharedState>, source_id: String) -> CommandResult<String> {
    use mimic_core::providers::SecretStore;
    let source = state
        .db
        .get_source(&source_id)?
        .filter(|s| s.connector == slack::CONNECTOR)
        .ok_or_else(|| CommandError::new("not_found", "That workspace isn't connected any more."))?;
    let token = state
        .secrets
        .get(&slack::secret_key(&source_id))
        .filter(|t| !t.is_empty())
        .ok_or_else(|| CommandError::new("slack", "I don't have a token for it. Give me one with New token."))?;
    let config: slack::SlackSourceConfig = serde_json::from_value(source.config)
        .map_err(|_| CommandError::new("unreadable", "I can't read its settings; remove it and connect it again."))?;
    let me = checked(&token).await?;
    if !config.is(&me) {
        return Err(CommandError::new(
            "not_this",
            format!("Its token is {}'s in {}, not the account you connected.", me.user, me.team),
        ));
    }
    Ok(format!("Read your direct messages in {} as {}. Nothing was sent.", me.team, me.user))
}

/// Give a connected workspace a new token — for the same account in the same
/// workspace — keeping everything read through it.
#[tauri::command]
pub async fn set_slack_token(
    state: State<'_, SharedState>,
    source_id: String,
    token: String,
) -> CommandResult<mimic_core::db::Source> {
    let gone = || CommandError::new("not_found", "That workspace isn't connected any more.");
    let source = state.db.get_source(&source_id)?.ok_or_else(gone)?;
    if source.connector != slack::CONNECTOR {
        return Err(gone());
    }
    let config: slack::SlackSourceConfig = serde_json::from_value(source.config.clone()).map_err(|_| {
        CommandError::new("unreadable", "I can't read this workspace's settings; remove it and connect it again.")
    })?;
    let token = token.trim().to_string();
    let me = checked(&token).await?;
    if !config.is(&me) {
        return Err(CommandError::new(
            "not_this",
            format!(
                "That token is {}'s in {}, not {}'s in {}. Make it in the workspace you connected, signed in as yourself.",
                me.user, me.team, config.user, config.team
            ),
        ));
    }
    state
        .secrets
        .set(&slack::secret_key(&source_id), &token)
        .map_err(|e| CommandError::new("secrets", format!("I couldn't store the token: {e}")))?;
    state.db.set_source_status(&source_id, "ready", None)?;
    check_soon(&state, &source_id)?;
    Ok(state.db.get_source(&source_id)?.unwrap_or(source))
}
