//! Microsoft Teams chats, connected with a sign-in in the user's browser
//! (0.10.0-alpha.67): through the same Microsoft registration mailboxes
//! sign in with, asking Graph's scopes (`teams::provider`). What lasts — the
//! refresh token — goes to the secret store, never the database.

use mimic_core::db::IdentifierKind;
use mimic_core::sources::imap::Auth;
use mimic_core::sources::teams::{self, GraphApi, Me};
use serde_json::json;
use tauri::State;

use crate::commands::sources::{browser_sign_in, client_for, no_refresh_token, SigningIn};
use crate::error::{CommandError, CommandResult};
use crate::SharedState;

fn teams_error(e: teams::TeamsError) -> CommandError {
    CommandError::new("teams", e.to_string())
}

/// Sign in with Microsoft in the browser for Teams, and say who signed in:
/// `(refresh token, who)`.
async fn signed_in(app: &tauri::AppHandle, state: &SharedState) -> CommandResult<(String, Me)> {
    // The client id mailboxes sign in with: this build's or the user's own.
    let (_, client) = client_for(state, Auth::Microsoft)?;
    let signing_in = SigningIn::begin(state)?;
    let provider = teams::provider();
    let tokens = browser_sign_in(app, &signing_in, provider.clone(), client, "").await?;
    let refresh = tokens.refresh_token.clone().ok_or_else(|| no_refresh_token(&provider))?;
    let access = tokens.access_token.clone();
    let me = tauri::async_runtime::spawn_blocking(move || GraphApi::new(&access)?.me())
        .await
        .map_err(|e| CommandError::new("internal", e.to_string()))?
        .map_err(teams_error)?;
    if signing_in.stopped() {
        return Err(CommandError::new("canceled", "Signing in was stopped."));
    }
    Ok((refresh, me))
}

fn check_soon(state: &crate::AppState, source_id: &str) -> CommandResult<()> {
    let queued = state.db.list_jobs(500, true)?.into_iter().any(|j| {
        j.kind == teams::JOB_KIND && j.status == "queued" && j.payload["sourceId"].as_str() == Some(source_id)
    });
    if !queued {
        state.jobs.enqueue(teams::JOB_KIND, json!({ "sourceId": source_id }))?;
    }
    Ok(())
}

/// Sign in for Teams, and connect the account signed in as: it becomes the
/// user's (its Microsoft id, and its address), its sign-in is kept, and the
/// first check starts. Stopped with `cancel_mail_sign_in`.
#[tauri::command]
pub async fn connect_teams(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
) -> CommandResult<mimic_core::db::Source> {
    crate::commands::sources::refuse_without_identity(&state)?;
    let (refresh, me) = signed_in(&app, state.inner()).await?;
    if teams::connected_as(&state.db, &me)?.is_some() {
        return Err(CommandError::new(
            "exists",
            format!("{}'s Teams is already connected. To sign it in again, use Sign in again beside it.", me.name),
        ));
    }
    // Signed in as themselves: whoever Microsoft says it is, is them.
    state.db.add_user_identifier(IdentifierKind::AccountId, &teams::account_id(&me.id))?;
    if let Some(email) = &me.email {
        state.db.add_user_identifier(IdentifierKind::Email, email)?;
    }
    crate::commands::people::reconcile_identity(&state.db, &state.jobs, "connect_teams");
    let new = teams::new_source(&me).map_err(|e| CommandError::new("internal", e.to_string()))?;
    let source = state.db.create_source(&new)?;
    if let Err(e) = state.secrets.set(&teams::secret_key(&source.id), &refresh) {
        let _ = state.db.delete_source_and_contents(&source.id);
        return Err(CommandError::new("secrets", format!("I couldn't store the sign-in: {e}")));
    }
    state.db.set_source_status(&source.id, "ready", None)?;
    check_soon(&state, &source.id)?;
    Ok(state.db.get_source(&source.id)?.unwrap_or(source))
}

/// Sign a connected Teams account in again — Microsoft asked for it, or the
/// saved sign-in can't be unlocked here — as the same account, keeping
/// everything read through it.
#[tauri::command]
pub async fn sign_in_teams_again(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
    source_id: String,
) -> CommandResult<mimic_core::db::Source> {
    let gone = || CommandError::new("not_found", "That Teams account isn't connected any more.");
    let source = state.db.get_source(&source_id)?.filter(|s| s.connector == teams::CONNECTOR).ok_or_else(gone)?;
    let config: teams::TeamsSourceConfig = serde_json::from_value(source.config.clone()).map_err(|_| {
        CommandError::new("unreadable", "I can't read this account's settings; remove it and connect it again.")
    })?;
    let (refresh, me) = signed_in(&app, state.inner()).await?;
    if !config.is(&me) {
        return Err(CommandError::new(
            "not_this",
            format!("You signed in as {}, not {}. Sign in as {} to keep this one.", me.name, config.user, config.user),
        ));
    }
    state
        .secrets
        .set(&teams::secret_key(&source_id), &refresh)
        .map_err(|e| CommandError::new("secrets", format!("I couldn't store the sign-in: {e}")))?;
    // An access token kept from the old sign-in isn't used again.
    state.teams_access.forget(&source_id);
    state.db.set_source_status(&source_id, "ready", None)?;
    check_soon(&state, &source_id)?;
    Ok(state.db.get_source(&source_id)?.unwrap_or(source))
}
