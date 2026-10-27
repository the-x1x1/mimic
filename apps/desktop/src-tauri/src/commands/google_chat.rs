//! Google Chat, connected with a sign-in in the user's browser
//! (0.10.0-alpha.79): through the same Google registration mailboxes sign in
//! with, asking the Chat API's scopes (`google_chat::provider`). What lasts —
//! the refresh token — goes to the secret store, never the database. Only a
//! Google Workspace account's Chat can be read this way.

use mimic_core::db::IdentifierKind;
use mimic_core::sources::google_chat::{self, ChatApi, Me};
use mimic_core::sources::imap::Auth;
use serde_json::json;
use tauri::State;

use crate::commands::sources::{browser_sign_in, client_for, no_refresh_token, SigningIn};
use crate::error::{CommandError, CommandResult};
use crate::SharedState;

fn chat_error(e: google_chat::GoogleChatError) -> CommandError {
    CommandError::new("google_chat", e.to_string())
}

/// Sign in with Google in the browser for Google Chat, and say who signed
/// in: `(refresh token, who)`.
async fn signed_in(app: &tauri::AppHandle, state: &SharedState) -> CommandResult<(String, Me)> {
    // The registration mailboxes sign in with: this build's or the user's
    // own, with its secret.
    let (google, client) = client_for(state, Auth::Google)?;
    let signing_in = SigningIn::begin(state)?;
    let provider = google_chat::provider(&google);
    let tokens = browser_sign_in(app, &signing_in, provider.clone(), client, "").await?;
    let refresh = tokens.refresh_token.clone().ok_or_else(|| no_refresh_token(&provider))?;
    let access = tokens.access_token.clone();
    let me = tauri::async_runtime::spawn_blocking(move || ChatApi::new(&access)?.me())
        .await
        .map_err(|e| CommandError::new("internal", e.to_string()))?
        .map_err(chat_error)?;
    if signing_in.stopped() {
        return Err(CommandError::new("canceled", "Signing in was stopped."));
    }
    Ok((refresh, me))
}

fn check_soon(state: &crate::AppState, source_id: &str) -> CommandResult<()> {
    let queued = state.db.list_jobs(500, true)?.into_iter().any(|j| {
        j.kind == google_chat::JOB_KIND && j.status == "queued" && j.payload["sourceId"].as_str() == Some(source_id)
    });
    if !queued {
        state.jobs.enqueue(google_chat::JOB_KIND, json!({ "sourceId": source_id }))?;
    }
    Ok(())
}

/// Sign in for Google Chat, and connect the account signed in as: it
/// becomes the user's (its Google id, and its address), its sign-in is kept,
/// and the first check starts. Stopped with `cancel_mail_sign_in`.
#[tauri::command]
pub async fn connect_google_chat(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
) -> CommandResult<mimic_core::db::Source> {
    crate::commands::sources::refuse_without_identity(&state)?;
    let (refresh, me) = signed_in(&app, state.inner()).await?;
    if google_chat::connected_as(&state.db, &me)?.is_some() {
        return Err(CommandError::new(
            "exists",
            format!(
                "{}'s Google Chat is already connected. To sign it in again, use Sign in again beside it.",
                me.name
            ),
        ));
    }
    // Signed in as themselves: whoever Google says it is, is them.
    state.db.add_user_identifier(IdentifierKind::AccountId, &google_chat::account_id(&me.id))?;
    if let Some(email) = &me.email {
        state.db.add_user_identifier(IdentifierKind::Email, email)?;
    }
    crate::commands::people::reconcile_identity(&state.db, &state.jobs, "connect_google_chat");
    let new = google_chat::new_source(&me).map_err(|e| CommandError::new("internal", e.to_string()))?;
    let source = state.db.create_source(&new)?;
    if let Err(e) = state.secrets.set(&google_chat::secret_key(&source.id), &refresh) {
        let _ = state.db.delete_source_and_contents(&source.id);
        return Err(CommandError::new("secrets", format!("I couldn't store the sign-in: {e}")));
    }
    state.db.set_source_status(&source.id, "ready", None)?;
    check_soon(&state, &source.id)?;
    Ok(state.db.get_source(&source.id)?.unwrap_or(source))
}

/// Try a connected Google Chat account as a check would, sending nothing:
/// its sign-in, who it is, and its chats.
#[tauri::command]
pub async fn test_google_chat(state: State<'_, SharedState>, source_id: String) -> CommandResult<String> {
    let source = state
        .db
        .get_source(&source_id)?
        .filter(|s| s.connector == google_chat::CONNECTOR)
        .ok_or_else(|| CommandError::new("not_found", "That Google Chat account isn't connected any more."))?;
    let config: google_chat::GoogleChatSourceConfig = serde_json::from_value(source.config)
        .map_err(|_| CommandError::new("unreadable", "I can't read its settings; remove it and connect it again."))?;
    let access = state.google_chat_access.clone();
    let (me, chats) = tauri::async_runtime::spawn_blocking(move || {
        let token = access.token(&source_id).map_err(|why| CommandError::new("google_chat", why))?;
        let found = ChatApi::new(&token).and_then(|api| api.check());
        if matches!(found, Err(google_chat::GoogleChatError::SignIn(_))) {
            access.forget(&source_id);
        }
        found.map_err(chat_error)
    })
    .await
    .map_err(|e| CommandError::new("internal", e.to_string()))??;
    if !config.is(&me) {
        return Err(CommandError::new(
            "not_this",
            format!("Its sign-in is {}'s, not the account you connected.", me.name),
        ));
    }
    Ok(if chats == 0 {
        format!("Signed in as {}; there are no chats to read yet. Nothing was sent.", me.name)
    } else {
        format!("Read your chats as {}. Nothing was sent.", me.name)
    })
}

/// Sign a connected Google Chat account in again — Google asked for it, or
/// the saved sign-in can't be unlocked here — as the same account, keeping
/// everything read through it.
#[tauri::command]
pub async fn sign_in_google_chat_again(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
    source_id: String,
) -> CommandResult<mimic_core::db::Source> {
    let gone = || CommandError::new("not_found", "That Google Chat account isn't connected any more.");
    let source = state.db.get_source(&source_id)?.filter(|s| s.connector == google_chat::CONNECTOR).ok_or_else(gone)?;
    let config: google_chat::GoogleChatSourceConfig = serde_json::from_value(source.config.clone()).map_err(|_| {
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
        .set(&google_chat::secret_key(&source_id), &refresh)
        .map_err(|e| CommandError::new("secrets", format!("I couldn't store the sign-in: {e}")))?;
    // An access token kept from the old sign-in isn't used again.
    state.google_chat_access.forget(&source_id);
    state.db.set_source_status(&source_id, "ready", None)?;
    check_soon(&state, &source_id)?;
    Ok(state.db.get_source(&source_id)?.unwrap_or(source))
}
