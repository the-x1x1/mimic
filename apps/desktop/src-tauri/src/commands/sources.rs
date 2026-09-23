//! Source connectors: what Mimic can read, and importing from it.

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
    account: mimic_core::sources::imap::ImapAccount,
    password: String,
) -> CommandResult<mimic_core::sources::imap::ImapProbe> {
    tauri::async_runtime::spawn_blocking(move || mimic_core::sources::imap::probe(&account, &password))
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
    account: mimic_core::sources::imap::ImapAccount,
    password: String,
    is_mine: bool,
) -> CommandResult<mimic_core::db::Source> {
    if state.db.user_identifier_set()?.is_empty() {
        return Err(CommandError::new(
            "no_identity",
            "Tell me which addresses are yours first, so I can tell your mail from everyone else's.",
        ));
    }
    let probe_account = account.clone();
    let probe_password = password.clone();
    let probe =
        tauri::async_runtime::spawn_blocking(move || mimic_core::sources::imap::probe(&probe_account, &probe_password))
            .await
            .map_err(|e| CommandError::new("internal", e.to_string()))?
            .map_err(mailbox_error)?;
    // Only after the login worked, and only when the user said this mailbox
    // is theirs rather than shared: its address becomes one of theirs, so
    // what is in Sent is read as their writing. A shared mailbox (team@,
    // support@) is left alone — what colleagues sent from it is not the user's.
    if is_mine && account.username.contains('@') {
        state.db.add_user_identifier(mimic_core::db::IdentifierKind::Email, account.username.trim())?;
        // Mail from it already read, from an export of the same mailbox, is
        // folded back if whoever it was filed under is nothing but the user;
        // anyone else is listed in Settings for the user to decide.
        crate::commands::people::reconcile_identity(&state.db, &state.jobs, "connect_mailbox");
    }
    let config = mimic_core::sources::imap::ImapSourceConfig {
        account: account.clone(),
        folders: probe.folders,
        sent_folder: probe.sent_folder,
        marks: Default::default(),
        last_checked_at: None,
    };
    let source = state.db.create_source(&mimic_core::db::NewSource {
        connector: mimic_core::sources::imap::CONNECTOR.into(),
        name: account.username.clone(),
        channel: "email".into(),
        location: None,
        config: serde_json::to_value(&config).map_err(|e| CommandError::new("internal", e.to_string()))?,
    })?;
    if let Err(e) = state.secrets.set(&mimic_core::sources::imap::secret_key(&source.id), &password) {
        // Without the password the mailbox can never be checked; do not leave
        // a source behind that looks connected and is not.
        let _ = state.db.delete_source_and_contents(&source.id);
        return Err(CommandError::new("secrets", format!("I couldn't store the password: {e}")));
    }
    state.db.set_source_status(&source.id, "ready", None)?;
    state.jobs.enqueue(mimic_core::sources::imap::JOB_KIND, json!({ "sourceId": source.id }))?;
    Ok(state.db.get_source(&source.id)?.unwrap_or(source))
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
    use mimic_core::sources::imap;
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
    // Checked with the new password straight away — unless a check is
    // already waiting to start, which will read it. One already running read
    // the old one, so another is queued behind it.
    let queued = state.db.list_jobs(500, true)?.iter().any(|j| {
        j.kind == imap::JOB_KIND && j.status == "queued" && j.payload["sourceId"].as_str() == Some(source_id.as_str())
    });
    if !queued {
        state.jobs.enqueue(imap::JOB_KIND, json!({ "sourceId": source_id }))?;
    }
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
    // A disconnected mailbox's password goes with it. The mail is already
    // gone, so a failure here does not undo that; it is logged, never with
    // the password.
    if let Err(e) = state.secrets.remove(&mimic_core::sources::imap::secret_key(&source_id)) {
        tracing::warn!(target: "secrets", error = %e, "a removed mailbox's password could not be removed");
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
