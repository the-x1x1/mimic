//! Source connectors: what Mimic can read, and importing from it.

use std::sync::atomic::Ordering;

use mimic_core::sources::imap::{self, Auth, Credential, ImapAccount, ImapProbe, Security};
use mimic_core::sources::oauth::{self, OAuthError};
use serde::Serialize;
use serde_json::json;
use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorInfo {
    pub connector: String,
    pub display_name: String,
    pub channel: String,
    pub description: String,
    pub location_kind: String,
    pub extensions: Vec<String>,
}

#[tauri::command]
pub async fn list_connectors() -> CommandResult<Vec<ConnectorInfo>> {
    Ok(mimic_core::sources::all()
        .into_iter()
        .map(|s| {
            let m = s.metadata();
            ConnectorInfo {
                connector: m.connector.into(),
                display_name: m.display_name.into(),
                channel: m.channel.into(),
                description: m.description.into(),
                location_kind: match m.location_kind {
                    mimic_core::sources::LocationKind::File => "file".into(),
                    mimic_core::sources::LocationKind::Folder => "folder".into(),
                },
                extensions: m.extensions.iter().map(|e| e.to_string()).collect(),
            }
        })
        .collect())
}

#[tauri::command]
pub async fn list_sources(state: State<'_, SharedState>) -> CommandResult<Vec<mimic_core::db::Source>> {
    Ok(state.db.list_sources()?)
}

/// Read a file without importing it, so the user can see what Mimic found
/// before committing.
#[tauri::command]
pub async fn validate_source_file(
    connector: String,
    location: String,
) -> CommandResult<mimic_core::sources::ValidationReport> {
    let source = mimic_core::sources::by_connector(&connector)?;
    Ok(source.validate(std::path::Path::new(&location))?)
}

#[tauri::command]
pub async fn create_source(
    state: State<'_, SharedState>,
    connector: String,
    name: String,
    channel: String,
    location: Option<String>,
) -> CommandResult<mimic_core::db::Source> {
    // Fail here rather than at import time, when the user has walked away.
    mimic_core::sources::by_connector(&connector)?;
    let source = state.db.create_source(&mimic_core::db::NewSource {
        connector,
        name,
        channel,
        location,
        config: serde_json::Value::Null,
    })?;
    state.db.set_source_status(&source.id, "ready", None)?;
    Ok(state.db.get_source(&source.id)?.unwrap_or(source))
}

#[tauri::command]
pub async fn start_source_import(
    state: State<'_, SharedState>,
    source_id: String,
) -> CommandResult<mimic_core::db::Job> {
    if state.db.user_identifier_set()?.is_empty() {
        return Err(CommandError::new(
            "no_identity",
            "Tell Mimic which addresses are yours first — otherwise every message imports as 'unknown' and none of it can teach it how you write.",
        ));
    }
    let Some(source) = state.db.get_source(&source_id)? else {
        return Err(CommandError::new("not_found", "That source no longer exists."));
    };
    // A connected mailbox is not read from a file; "import again" means
    // "check it now".
    if source.connector == mimic_core::sources::imap::CONNECTOR {
        return Ok(state.jobs.enqueue(mimic_core::sources::imap::JOB_KIND, json!({ "sourceId": source_id }))?);
    }
    Ok(state.jobs.enqueue(mimic_core::import::JOB_KIND, json!({ "sourceId": source_id }))?)
}

fn mailbox_error(e: mimic_core::sources::imap::ImapError) -> CommandError {
    CommandError::new("mailbox", e.to_string())
}

/// Log in and look around without importing anything: which folders would
/// be read, how much is in them, and anything worth knowing first (no sent
/// folder, a mailbox bigger than the first check reads).
#[tauri::command]
pub async fn probe_mailbox(
    mut account: mimic_core::sources::imap::ImapAccount,
    password: String,
) -> CommandResult<mimic_core::sources::imap::ImapProbe> {
    account.auth = Auth::Password;
    tauri::async_runtime::spawn_blocking(move || imap::probe(&account, &Credential::Password(password)))
        .await
        .map_err(|e| CommandError::new("internal", e.to_string()))?
        .map_err(mailbox_error)
}

/// Connect a mailbox: check the login works, store the password in the
/// secret store (never the database), create the source, and start the
/// first check.
#[tauri::command]
pub async fn connect_mailbox(
    state: State<'_, SharedState>,
    mut account: mimic_core::sources::imap::ImapAccount,
    password: String,
    is_mine: bool,
) -> CommandResult<mimic_core::db::Source> {
    refuse_without_identity(&state)?;
    // What is kept is a password, whatever the page said.
    account.auth = Auth::Password;
    let probe_account = account.clone();
    let probe_password = password.clone();
    let probe = tauri::async_runtime::spawn_blocking(move || {
        imap::probe(&probe_account, &Credential::Password(probe_password))
    })
    .await
    .map_err(|e| CommandError::new("internal", e.to_string()))?
    .map_err(mailbox_error)?;
    add_mailbox(&state, account, probe, is_mine, &password)
}

fn refuse_without_identity(state: &crate::AppState) -> CommandResult<()> {
    if state.db.user_identifier_set()?.is_empty() {
        return Err(CommandError::new(
            "no_identity",
            "Tell me which addresses are yours first, so I can tell your mail from everyone else's.",
        ));
    }
    Ok(())
}

/// A mailbox that has been reached: its address becomes the user's when they
/// said it is theirs, its secret — the password, or the refresh token of a
/// sign-in — goes to the secret store (never the database), and the first
/// check starts.
fn add_mailbox(
    state: &crate::AppState,
    account: ImapAccount,
    probe: ImapProbe,
    is_mine: bool,
    secret: &str,
) -> CommandResult<mimic_core::db::Source> {
    // Only when the user said this mailbox is theirs rather than shared: its
    // address becomes one of theirs, so what is in Sent is read as their
    // writing. A shared mailbox (team@, support@) is left alone — what
    // colleagues sent from it is not the user's.
    if is_mine && account.username.contains('@') {
        state.db.add_user_identifier(mimic_core::db::IdentifierKind::Email, account.username.trim())?;
        // Mail from it already read, from an export of the same mailbox, is
        // folded back if whoever it was filed under is nothing but the user;
        // anyone else is listed in Settings for the user to decide.
        crate::commands::people::reconcile_identity(&state.db, &state.jobs, "connect_mailbox");
    }
    let config = imap::ImapSourceConfig {
        account: account.clone(),
        folders: probe.folders,
        sent_folder: probe.sent_folder,
        marks: Default::default(),
        last_checked_at: None,
    };
    let source = state.db.create_source(&mimic_core::db::NewSource {
        connector: imap::CONNECTOR.into(),
        name: account.username.clone(),
        channel: "email".into(),
        location: None,
        config: serde_json::to_value(&config).map_err(|e| CommandError::new("internal", e.to_string()))?,
    })?;
    if let Err(e) = state.secrets.set(&imap::secret_key(&source.id), secret) {
        // Without it the mailbox can never be checked; do not leave a source
        // behind that looks connected and is not.
        let _ = state.db.delete_source_and_contents(&source.id);
        let what = if account.auth == Auth::Password { "the password" } else { "the sign-in" };
        return Err(CommandError::new("secrets", format!("I couldn't store {what}: {e}")));
    }
    state.db.set_source_status(&source.id, "ready", None)?;
    state.jobs.enqueue(imap::JOB_KIND, json!({ "sourceId": source.id }))?;
    Ok(state.db.get_source(&source.id)?.unwrap_or(source))
}

/// Whether this copy of Mimic can sign in to a mailbox with Microsoft
/// (Outlook.com, Hotmail, Microsoft 365): it needs Mimic's client id with
/// Microsoft, compiled in.
#[tauri::command]
pub async fn mail_sign_in_available() -> CommandResult<bool> {
    Ok(oauth::microsoft_client_id().is_some())
}

/// Where a Microsoft mailbox is, for the address signed in to.
fn microsoft_account(email: &str) -> ImapAccount {
    let provider = oauth::Provider::microsoft();
    ImapAccount {
        host: provider.imap_host.into(),
        port: provider.imap_port,
        username: email.trim().to_string(),
        security: Security::Tls,
        auth: Auth::Microsoft,
    }
}

/// A sign-in under way. The next can't start until every holder has let go:
/// the command that started it, which holds it until it returns — through
/// looking at the mailbox, not just signing in — and the wait on the
/// browser, which may outlive a command whose caller went away.
struct SigningIn(SharedState);

impl SigningIn {
    fn begin(state: &SharedState) -> CommandResult<std::sync::Arc<Self>> {
        if state.sign_in.active.swap(true, Ordering::SeqCst) {
            return Err(CommandError::new(
                "busy",
                "A sign-in is already waiting on your browser. Finish it there, or stop it, first.",
            ));
        }
        state.sign_in.cancel.store(false, Ordering::SeqCst);
        Ok(std::sync::Arc::new(SigningIn(state.clone())))
    }

    /// Whether `cancel_mail_sign_in` stopped this sign-in.
    fn stopped(&self) -> bool {
        self.0.sign_in.cancel.load(Ordering::SeqCst)
    }
}

impl Drop for SigningIn {
    fn drop(&mut self) {
        self.0.sign_in.active.store(false, Ordering::SeqCst);
    }
}

fn sign_in_error(e: OAuthError) -> CommandError {
    match e {
        OAuthError::Canceled => CommandError::new("canceled", e.to_string()),
        other => CommandError::new("sign_in", other.to_string()),
    }
}

/// Mimic's client id with Microsoft, or the refusal that says this copy
/// can't sign in.
fn microsoft_client() -> CommandResult<&'static str> {
    oauth::microsoft_client_id()
        .ok_or_else(|| CommandError::new("not_configured", OAuthError::NotConfigured("Microsoft").to_string()))
}

/// Sign in with Microsoft in the user's browser: stoppable with
/// `cancel_mail_sign_in`, and given up after five minutes.
async fn microsoft_sign_in(
    app: &tauri::AppHandle,
    signing_in: &std::sync::Arc<SigningIn>,
    client: &'static str,
    email: &str,
) -> CommandResult<oauth::Tokens> {
    let (app, waiting, hint) = (app.clone(), signing_in.clone(), email.to_string());
    tauri::async_runtime::spawn_blocking(move || {
        let open = |url: &str| {
            use tauri_plugin_opener::OpenerExt;
            app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
        };
        let stop = || waiting.stopped();
        oauth::sign_in(&oauth::Provider::microsoft(), client, Some(&hint), &open, &stop)
    })
    .await
    .map_err(|e| CommandError::new("internal", e.to_string()))?
    .map_err(sign_in_error)
}

/// Look at a mailbox through a sign-in, as `probe_mailbox` does through a
/// password. A sign-in that works but is not this mailbox's — the user signed
/// in as someone else — is said so.
async fn probe_signed_in(account: ImapAccount, access_token: String) -> CommandResult<ImapProbe> {
    let email = account.username.clone();
    tauri::async_runtime::spawn_blocking(move || imap::probe(&account, &Credential::AccessToken(access_token)))
        .await
        .map_err(|e| CommandError::new("internal", e.to_string()))?
        .map_err(|e| match e {
            imap::ImapError::SignIn(_) => CommandError::new(
                "sign_in",
                format!(
                    "That sign-in doesn't open {email}'s mailbox. Sign in as {email}. If you did, IMAP may be turned off for this mailbox; for a work or school account, your admin can turn it on."
                ),
            ),
            other => mailbox_error(other),
        })
}

fn no_refresh_token() -> CommandError {
    CommandError::new(
        "sign_in",
        "Microsoft signed you in, but gave me no way to stay signed in, so I couldn't keep checking this mailbox.",
    )
}

/// Sign in to a mailbox with Microsoft in the browser, and look around it,
/// without importing anything. What the sign-in brought back is held in
/// memory until `connect_signed_in_mailbox`, and nowhere else.
#[tauri::command]
pub async fn sign_in_to_mailbox(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
    email: String,
) -> CommandResult<ImapProbe> {
    let email = email.trim().to_string();
    if !email.contains('@') {
        return Err(CommandError::new("invalid", "That doesn't look like an email address."));
    }
    let client = microsoft_client()?;
    let signing_in = SigningIn::begin(state.inner())?;
    *state.sign_in.pending.lock().unwrap_or_else(|p| p.into_inner()) = None;
    let tokens = microsoft_sign_in(&app, &signing_in, client, &email).await?;
    let refresh_token = tokens.refresh_token.clone().ok_or_else(no_refresh_token)?;
    let probe = probe_signed_in(microsoft_account(&email), tokens.access_token.clone()).await?;
    // Stopped while the mailbox was being looked at: the dialog is gone, so
    // nothing is held for it. Decided under the lock `cancel_mail_sign_in`
    // takes, so a stop can't land between the look and the keeping.
    let mut pending = state.sign_in.pending.lock().unwrap_or_else(|p| p.into_inner());
    if signing_in.stopped() {
        return Err(sign_in_error(OAuthError::Canceled));
    }
    *pending = Some(crate::app_state::PendingSignIn { email, refresh_token, probe: probe.clone() });
    Ok(probe)
}

/// Connect the mailbox just signed in to, as `connect_mailbox` connects one
/// with a password.
#[tauri::command]
pub async fn connect_signed_in_mailbox(
    state: State<'_, SharedState>,
    email: String,
    is_mine: bool,
) -> CommandResult<mimic_core::db::Source> {
    refuse_without_identity(&state)?;
    let pending = state.sign_in.pending.lock().unwrap_or_else(|p| p.into_inner()).take();
    let Some(signed_in) = pending.filter(|p| p.email.eq_ignore_ascii_case(email.trim())) else {
        return Err(CommandError::new("sign_in", "I no longer have that sign-in. Sign in again."));
    };
    add_mailbox(&state, microsoft_account(&signed_in.email), signed_in.probe, is_mine, &signed_in.refresh_token)
}

/// Stop a sign-in waiting on the browser, and forget one that is waiting to
/// be connected.
#[tauri::command]
pub async fn cancel_mail_sign_in(state: State<'_, SharedState>) -> CommandResult<()> {
    let mut pending = state.sign_in.pending.lock().unwrap_or_else(|p| p.into_inner());
    state.sign_in.cancel.store(true, Ordering::SeqCst);
    *pending = None;
    Ok(())
}

/// Sign a connected Microsoft mailbox in again — Microsoft asked for it, or
/// the saved sign-in can't be unlocked on this Windows account — without
/// removing it and everything read from it. The new sign-in is tried on the
/// mailbox first, and kept only if it opens it.
#[tauri::command]
pub async fn sign_in_mailbox_again(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
    source_id: String,
) -> CommandResult<mimic_core::db::Source> {
    let source = state
        .db
        .get_source(&source_id)?
        .filter(|s| s.connector == imap::CONNECTOR)
        .ok_or_else(|| CommandError::new("not_found", "That mailbox isn't connected any more."))?;
    let config: imap::ImapSourceConfig = serde_json::from_value(source.config.clone()).map_err(|_| {
        CommandError::new("mailbox", "I can't read this mailbox's settings; remove it and connect it again.")
    })?;
    if config.account.auth != Auth::Microsoft {
        return Err(CommandError::new("mailbox", "This mailbox uses a password. Give it a new one instead."));
    }
    let client = microsoft_client()?;
    let signing_in = SigningIn::begin(state.inner())?;
    let tokens = microsoft_sign_in(&app, &signing_in, client, &config.account.username).await?;
    let refresh_token = tokens.refresh_token.clone().ok_or_else(no_refresh_token)?;
    probe_signed_in(config.account.clone(), tokens.access_token.clone()).await?;
    // Stopped while the mailbox was being looked at: the old sign-in stays.
    if signing_in.stopped() {
        return Err(sign_in_error(OAuthError::Canceled));
    }
    let (waiting, id) = (state.inner().clone(), source_id.clone());
    tauri::async_runtime::spawn_blocking(move || {
        waiting.mail_credentials.settle(&id, || waiting.secrets.set(&imap::secret_key(&id), &refresh_token))
    })
    .await
    .map_err(|e| CommandError::new("internal", e.to_string()))?
    .map_err(|e| CommandError::new("secrets", format!("I couldn't keep the sign-in: {e}")))?;
    if source.status == "failed" {
        state.db.set_source_status(&source_id, "ready", None)?;
    }
    check_again(&state, &source_id)?;
    state
        .db
        .get_source(&source_id)?
        .ok_or_else(|| CommandError::new("not_found", "That mailbox isn't connected any more."))
}

/// Check a mailbox now — unless a check is already waiting to start, which
/// will read the new password or sign-in. One already running read the old
/// one, so another is queued behind it.
fn check_again(state: &crate::AppState, source_id: &str) -> CommandResult<()> {
    let queued =
        state.db.list_jobs(500, true)?.iter().any(|j| {
            j.kind == imap::JOB_KIND && j.status == "queued" && j.payload["sourceId"].as_str() == Some(source_id)
        });
    if !queued {
        state.jobs.enqueue(imap::JOB_KIND, json!({ "sourceId": source_id }))?;
    }
    Ok(())
}

/// Give a connected mailbox its password again — after the provider's app
/// password was replaced, or when the saved one cannot be unlocked on this
/// Windows account — without removing the mailbox and everything imported
/// through it. The login is tried first, and nothing is saved if it fails.
#[tauri::command]
pub async fn set_mailbox_password(
    state: State<'_, SharedState>,
    source_id: String,
    password: String,
) -> CommandResult<mimic_core::db::Source> {
    let (db, secrets, id) = (state.db.clone(), state.secrets.clone(), source_id.clone());
    tauri::async_runtime::spawn_blocking(move || {
        imap::replace_password(&db, &id, &password, |pw| {
            secrets.set(&imap::secret_key(&id), pw).map_err(|e| e.to_string())
        })
    })
    .await
    .map_err(|e| CommandError::new("internal", e.to_string()))?
    .map_err(|e| match e {
        imap::PasswordError::Login(e) => mailbox_error(e),
        other => CommandError::new("mailbox", other.to_string()),
    })?;
    check_again(&state, &source_id)?;
    state
        .db
        .get_source(&source_id)?
        .ok_or_else(|| CommandError::new("not_found", "That mailbox isn't connected any more."))
}

/// Remove a source and everything imported through it.
#[tauri::command]
pub async fn delete_source(
    state: State<'_, SharedState>,
    source_id: String,
) -> CommandResult<mimic_core::privacy::DeletionReport> {
    crate::commands::voice::stop_measuring(&state);
    let report = state.db.delete_source_and_contents(&source_id)?;
    // A disconnected mailbox's password, or sign-in, goes with it — after any
    // check still trading that sign-in for a token has saved what it was
    // handed, so nothing is left behind. The mail is already gone, so a
    // failure here does not undo that; it is logged, never with the password.
    let waiting = state.inner().clone();
    let removed = tauri::async_runtime::spawn_blocking(move || {
        waiting.mail_credentials.settle(&source_id, || waiting.secrets.remove(&imap::secret_key(&source_id)))
    })
    .await;
    match removed {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            tracing::warn!(target: "secrets", error = %e, "a removed mailbox's password could not be removed")
        }
        Err(e) => tracing::warn!(target: "secrets", error = %e, "removing a removed mailbox's password stopped"),
    }
    Ok(report)
}

#[tauri::command]
pub async fn pick_source_file(
    app: tauri::AppHandle,
    title: Option<String>,
    extensions: Vec<String>,
) -> CommandResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let exts: Vec<&str> = extensions.iter().map(String::as_str).collect();
    app.dialog()
        .file()
        .set_title(title.unwrap_or_else(|| "Choose a file".into()))
        .add_filter("Supported exports", &exts)
        .pick_file(move |p| {
            let _ = tx.send(p.map(|f| f.to_string()));
        });
    Ok(rx.await.unwrap_or(None))
}
