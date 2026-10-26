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
use mimic_core::sources::transcript;
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
    // Into Slack, with the workspace's own token (0.10.0-alpha.65); into
    // Teams, with the account's own sign-in (0.10.0-alpha.68).
    let connector = state.db.get_source(&request.source_id)?.map(|s| s.connector);
    if connector.as_deref() == Some(mimic_core::sources::teams::CONNECTOR) {
        let (db, access) = (state.db.clone(), state.teams_access.clone());
        return tauri::async_runtime::spawn_blocking(move || {
            let token = access.token(&request.source_id).map_err(sending::SendError::Cannot)?;
            let api = mimic_core::sources::teams::GraphApi::new(&token)
                .map_err(|e| sending::SendError::Service(e.to_string()))?;
            sending::teams::send_reply(&db, &api, &request)
        })
        .await
        .map_err(|e| CommandError::new("internal", e.to_string()))?
        .map_err(CommandError::from);
    }
    if connector.as_deref() == Some(mimic_core::sources::slack::CONNECTOR) {
        use mimic_core::providers::SecretStore;
        let token = state
            .secrets
            .get(&mimic_core::sources::slack::secret_key(&request.source_id))
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                CommandError::new(
                    "cannot_send",
                    "I don't have a Slack token I can use for this workspace. Give me one under Your mail.",
                )
            })?;
        let db = state.db.clone();
        return tauri::async_runtime::spawn_blocking(move || {
            let api = mimic_core::sources::slack::SlackApi::new(&token)
                .map_err(|e| sending::SendError::Service(e.to_string()))?;
            sending::slack::send_reply(&db, &api, &request)
        })
        .await
        .map_err(|e| CommandError::new("internal", e.to_string()))?
        .map_err(CommandError::from);
    }
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
    /// What goes under each reply sent from it (0.10.0-alpha.36).
    pub signature: Option<String>,
}

impl MailboxSending {
    /// How `config`'s mailbox sends, as it is stored.
    fn of(config: &ImapSourceConfig) -> Self {
        let sending = config.sending_account();
        let keeps_sent = sending.as_ref().is_some_and(|s| smtp::keeps_sent(&s.host));
        MailboxSending {
            sending,
            set_by_user: config.sending.is_some(),
            keeps_sent,
            signature: config.signature().map(str::to_string),
        }
    }
}

/// The longest signature kept: a few lines of name, role and number, not a
/// letter.
const SIGNATURE_MOST: usize = 1000;

pub(crate) fn mailbox_config(state: &crate::AppState, source_id: &str) -> CommandResult<ImapSourceConfig> {
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
    Ok(MailboxSending::of(&config))
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
    let db = state.db.clone();
    let account = sending.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let credential = credentials.credential(auth, &id).map_err(|why| CommandError::new("mailbox", why))?;
        transcript::kept_if_it_fails(&db, "sending_probe_failed", &account.host.clone(), || {
            smtp::probe(&account, &credential)
        })
        .map_err(|e| {
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
    Ok(MailboxSending::of(&latest))
}

/// What goes under each reply sent from a mailbox, or nothing (empty).
/// Kept as given, trimmed; nothing is tried, since nothing is sent.
#[tauri::command]
pub async fn set_mailbox_signature(
    state: State<'_, SharedState>,
    source_id: String,
    signature: String,
) -> CommandResult<MailboxSending> {
    let signature = signature.trim().replace("\r\n", "\n");
    if signature.chars().count() > SIGNATURE_MOST {
        return Err(CommandError::new(
            "invalid",
            format!("That's longer than a signature ({SIGNATURE_MOST} characters at most)."),
        ));
    }
    let mut config = mailbox_config(&state, &source_id)?;
    config.signature = (!signature.is_empty()).then_some(signature);
    state.db.set_source_config(&source_id, &serde_json::to_value(&config)?)?;
    Ok(MailboxSending::of(&config))
}

/// Sign in to the server a mailbox's replies go out through — its provider's
/// usual one, or the one the user set — and send nothing: whether a reply
/// could go now. The server's name when it worked (0.10.0-alpha.32).
#[tauri::command]
pub async fn test_mailbox_sending(state: State<'_, SharedState>, source_id: String) -> CommandResult<String> {
    let config = mailbox_config(&state, &source_id)?;
    let Some(account) = config.sending_account() else {
        return Err(CommandError::new("cannot_send", sending::NOT_SET_UP));
    };
    let host = account.host.clone();
    let (credentials, auth, id) = (state.mail_credentials.clone(), config.account.auth, source_id.clone());
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let credential = credentials.credential(auth, &id).map_err(|why| CommandError::new("mailbox", why))?;
        transcript::kept_if_it_fails(&db, "sending_probe_failed", &account.host.clone(), || {
            smtp::probe(&account, &credential)
        })
        .map_err(|e| {
            if matches!(e, smtp::SmtpError::SignIn(_)) {
                credentials.forget(&id);
            }
            CommandError::new("send_failed", e.to_string())
        })
    })
    .await
    .map_err(|e| CommandError::new("internal", e.to_string()))??;
    Ok(host)
}

/// Sign in to a connected mailbox with its own credential and look at its
/// folders, opening no message and changing nothing (0.10.0-alpha.51):
/// whether a check would work now. What the server said is kept if it fails,
/// as a probe's is.
#[tauri::command]
pub async fn test_mailbox_reading(state: State<'_, SharedState>, source_id: String) -> CommandResult<imap::ImapProbe> {
    let config = mailbox_config(&state, &source_id)?;
    let (credentials, id, db) = (state.mail_credentials.clone(), source_id.clone(), state.db.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let credential =
            credentials.credential(config.account.auth, &id).map_err(|why| CommandError::new("mailbox", why))?;
        transcript::kept_if_it_fails(&db, "probe_failed", &config.account.host.clone(), || {
            imap::probe(&config.account, &credential)
        })
        .map_err(|e| {
            if matches!(e, imap::ImapError::SignIn(_)) {
                credentials.forget(&id);
            }
            CommandError::new("mailbox", e.to_string())
        })
    })
    .await
    .map_err(|e| CommandError::new("internal", e.to_string()))?
}

/// Forget how the user set a mailbox to send, and use its provider's known
/// server again (none, for a provider Mimic doesn't know).
#[tauri::command]
pub async fn reset_mailbox_sending(state: State<'_, SharedState>, source_id: String) -> CommandResult<MailboxSending> {
    let mut config = mailbox_config(&state, &source_id)?;
    config.sending = None;
    state.db.set_source_config(&source_id, &serde_json::to_value(&config)?)?;
    Ok(MailboxSending::of(&config))
}
