//! Sending replies through the user's own mailbox (0.10.0-alpha.23), and how
//! each mailbox sends.
//!
//! Nothing here sends anything the user has not seen: `reply_envelope` works
//! out who a reply goes to, through which mailbox and under what subject, and
//! `send_reply` sends exactly what the Send panel showed and the user
//! approved. Sending blocks on the network and runs off the async runtime.

use mimic_core::db::OutboxEntry;
use mimic_core::sending::{self, ReplyEnvelope, SendRequest};
use mimic_core::sources::imap::{self, ImapSourceConfig};
use mimic_core::sources::outgoing;
use mimic_core::sources::smtp::{self, SmtpAccount};
use serde::Serialize;
use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

/// Who a reply to this draft goes to, how and through what — or why it can't
/// be sent from here. Changes nothing.
#[tauri::command]
pub async fn reply_envelope(state: State<'_, SharedState>, draft_id: String) -> CommandResult<ReplyEnvelope> {
    Ok(sending::envelope_for(&state.db, &draft_id)?)
}

/// Send a reply the user approved, exactly as approved. Never logged: the
/// words, the addresses and the subject stay in the database.
#[tauri::command]
pub async fn send_reply(state: State<'_, SharedState>, request: SendRequest) -> CommandResult<OutboxEntry> {
    let (db, credentials) = (state.db.clone(), state.mail_credentials.clone());
    tauri::async_runtime::spawn_blocking(move || sending::send_reply(&db, &credentials, &request))
        .await
        .map_err(|e| CommandError::new("internal", e.to_string()))?
        .map_err(CommandError::from)
}

/// What Mimic sent or holds to send, newest first.
#[tauri::command]
pub async fn list_sent(state: State<'_, SharedState>, limit: Option<usize>) -> CommandResult<Vec<OutboxEntry>> {
    Ok(state.db.recent_outgoing(limit.unwrap_or(25).clamp(1, 200))?)
}

/// The user looked in their Sent folder after a reply that may have gone,
/// and it didn't: its draft can be sent again.
#[tauri::command]
pub async fn reply_did_not_go(state: State<'_, SharedState>, outbox_id: String) -> CommandResult<OutboxEntry> {
    Ok(state.db.outgoing_did_not_go(&outbox_id)?)
}

/// How a mailbox sends: as the user set it, or the server its provider is
/// known to send from, and which of the two.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxSending {
    pub sending: Option<SmtpAccount>,
    pub set_by_user: bool,
    /// Its provider puts what is sent in the Sent folder itself, so no copy
    /// is made unless the user asks (`smtp::keeps_sent`).
    pub keeps_sent: bool,
}

impl MailboxSending {
    fn of(sending: Option<SmtpAccount>, set_by_user: bool) -> Self {
        let keeps_sent = sending.as_ref().is_some_and(|s| smtp::keeps_sent(&s.host));
        MailboxSending { sending, set_by_user, keeps_sent }
    }
}

fn mailbox_config(state: &crate::AppState, source_id: &str) -> CommandResult<ImapSourceConfig> {
    let source = state
        .db
        .get_source(source_id)?
        .filter(|s| s.connector == imap::CONNECTOR)
        .ok_or_else(|| CommandError::new("not_found", sending::MAILBOX_GONE))?;
    serde_json::from_value(source.config).map_err(|_| CommandError::new("invalid", sending::UNREADABLE))
}

#[tauri::command]
pub async fn get_mailbox_sending(state: State<'_, SharedState>, source_id: String) -> CommandResult<MailboxSending> {
    let config = mailbox_config(&state, &source_id)?;
    Ok(MailboxSending::of(config.sending_account(), config.sending.is_some()))
}

/// Send through `sending` from now on: checked first — connected to and
/// signed in to with the mailbox's own password or sign-in, sending nothing —
/// and kept only if that worked.
#[tauri::command]
pub async fn set_mailbox_sending(
    state: State<'_, SharedState>,
    source_id: String,
    mut sending: SmtpAccount,
) -> CommandResult<MailboxSending> {
    sending.host = sending.host.trim().to_string();
    sending.username = sending.username.trim().to_string();
    sending.from_address = sending.from_address.trim().to_string();
    if sending.host.is_empty() || sending.port == 0 {
        return Err(CommandError::new("invalid", "Give the mail server's name and its port."));
    }
    if !outgoing::is_address(&sending.from_address) {
        return Err(CommandError::new("invalid", sending::NO_FROM));
    }
    let key = mimic_core::db::IdentifierInput::new(mimic_core::db::IdentifierKind::Email, &sending.from_address).key();
    if !state.db.user_identifier_set()?.contains(&key) {
        return Err(CommandError::new("not_yours", sending::NOT_YOURS));
    }
    let config = mailbox_config(&state, &source_id)?;
    let (credentials, auth, id) = (state.mail_credentials.clone(), config.account.auth, source_id.clone());
    let account = sending.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let credential = credentials.credential(auth, &id).map_err(|why| CommandError::new("mailbox", why))?;
        smtp::probe(&account, &credential).map_err(|e| {
            if matches!(e, smtp::SmtpError::SignIn(_)) {
                credentials.forget(&id);
            }
            CommandError::new("send_failed", e.to_string())
        })
    })
    .await
    .map_err(|e| CommandError::new("internal", e.to_string()))??;
    // Read again: a check may have moved the mailbox on while this one was
    // being tried.
    let mut latest = mailbox_config(&state, &source_id)?;
    latest.sending = Some(sending);
    state.db.set_source_config(&source_id, &serde_json::to_value(&latest)?)?;
    Ok(MailboxSending::of(latest.sending, true))
}

/// Forget how the user set a mailbox to send, and use its provider's known
/// server again (none, for a provider Mimic doesn't know).
#[tauri::command]
pub async fn reset_mailbox_sending(state: State<'_, SharedState>, source_id: String) -> CommandResult<MailboxSending> {
    let mut config = mailbox_config(&state, &source_id)?;
    config.sending = None;
    state.db.set_source_config(&source_id, &serde_json::to_value(&config)?)?;
    Ok(MailboxSending::of(config.sending_account(), false))
}
