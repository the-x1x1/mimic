//! A Telegram account, connected as the user (0.10.0-alpha.74): the app id
//! and hash they registered at my.telegram.org, then their phone number, the
//! code Telegram sends and, if they set one, their password — which goes to
//! Telegram and is kept nowhere. Between those steps the connection that
//! asked is held in memory (`AppState.telegram`); what the sign-in leaves,
//! and the app's id and hash, go to the secret store, never the database.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mimic_core::db::IdentifierKind;
use mimic_core::providers::SecretStore;
use mimic_core::sources::telegram_account::{
    self as telegram, AppCredentials, KeptSession, Live, LoginToken, Me, PasswordToken, Step, TelegramError,
    TelegramPost,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

/// How long a sign-in waits for its code or password.
const PENDING_FOR: Duration = Duration::from_secs(15 * 60);

/// A sign-in under way: the connection that asked for the code, and what
/// Telegram handed back to finish it with.
pub struct PendingTelegram {
    live: Live,
    login: LoginToken,
    password: Option<PasswordToken>,
    started: Instant,
    /// Which sign-in it is (`SIGN_IN`).
    number: u64,
}

/// Moves on with each sign-in started, and each one stopped: a step that
/// finds it moved on while Telegram was being asked belongs to a sign-in the
/// user stopped, or started again, and goes no further (0.10.0-alpha.77).
static SIGN_IN: AtomicU64 = AtomicU64::new(0);

const STOPPED: &str = "You stopped that sign-in, so I didn't connect the account.";

/// Whether the app's registration is kept, and its id (never its hash).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramApp {
    pub saved: bool,
    pub api_id: Option<i32>,
}

/// Where a sign-in stands: `code` (Telegram sent one), `password` (the
/// account has one; `hint`, and whether the last one tried was `wrong`), or
/// `done`, with the account's source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramStep {
    pub step: String,
    pub hint: Option<String>,
    pub wrong: bool,
    pub source: Option<mimic_core::db::Source>,
}

impl TelegramStep {
    fn code() -> Self {
        TelegramStep { step: "code".into(), hint: None, wrong: false, source: None }
    }
    fn password(hint: Option<String>, wrong: bool) -> Self {
        TelegramStep { step: "password".into(), hint, wrong, source: None }
    }
    fn done(source: mimic_core::db::Source) -> Self {
        TelegramStep { step: "done".into(), hint: None, wrong: false, source: Some(source) }
    }
}

fn telegram_error(e: TelegramError) -> CommandError {
    let code = match e {
        TelegramError::BadCode => "bad_code",
        TelegramError::App(_) => "app",
        _ => "telegram",
    };
    CommandError::new(code, e.to_string())
}

fn saved_app(state: &crate::AppState) -> Option<AppCredentials> {
    state.secrets.get(telegram::APP_KEY).and_then(|raw| serde_json::from_str(&raw).ok())
}

pub(crate) const NO_SESSION: &str =
    "I don't have a Telegram sign-in I can use for this account. Sign in again under Your mail.";

/// What sends into an account, or asks it before a reply goes by itself
/// (0.10.0-alpha.75): the app's registration and the account's session, as
/// kept — and the session as it was read, so a newer one kept meanwhile
/// isn't written over (`keep_session`, 0.10.0-alpha.77).
pub(crate) struct AccountSession {
    app: AppCredentials,
    session: KeptSession,
    read: String,
}

pub(crate) fn session_for(state: &crate::AppState, source_id: &str) -> Option<AccountSession> {
    let app = saved_app(state)?;
    let read = state.secrets.get(&telegram::secret_key(source_id))?;
    let session = KeptSession::from_kept(&read).ok()?;
    Some(AccountSession { app, session, read })
}

/// Keep what Telegram changed in a session after it was used — unless the
/// account was removed meanwhile, or another session was kept for it since
/// this one was read (a sign-in again, a check): the newer one stands
/// (0.10.0-alpha.77).
fn keep_session(state: &crate::AppState, source_id: &str, session: &KeptSession, read: &str) {
    if state.db.get_source(source_id).ok().flatten().is_none() {
        return;
    }
    let key = telegram::secret_key(source_id);
    if state.secrets.get(&key).as_deref() != Some(read) {
        return;
    }
    if let Ok(kept) = session.to_kept() {
        if let Err(e) = state.secrets.set(&key, &kept) {
            tracing::warn!(target: "telegram", error = %e, "couldn't keep the session");
        }
    }
}

/// A connection through an account's session that, when it is dropped,
/// keeps what Telegram changed in the session — not once Telegram said it
/// is signed out (0.10.0-alpha.77) — and closes (0.10.0-alpha.76).
pub(crate) struct KeptPost {
    post: Option<telegram::LivePost>,
    state: SharedState,
    source_id: String,
    read: String,
    signed_out: AtomicBool,
}

impl KeptPost {
    pub(crate) fn start(
        handle: tokio::runtime::Handle,
        state: SharedState,
        source_id: &str,
        account: AccountSession,
    ) -> Self {
        let post = telegram::LivePost::start(handle, &account.app, Arc::new(account.session));
        KeptPost {
            post: Some(post),
            state,
            source_id: source_id.to_string(),
            read: account.read,
            signed_out: AtomicBool::new(false),
        }
    }

    fn live(&self) -> Result<&telegram::LivePost, TelegramError> {
        self.post.as_ref().ok_or_else(|| TelegramError::Network("the connection was closed".into()))
    }

    fn noted<T>(&self, answer: Result<T, TelegramError>) -> Result<T, TelegramError> {
        if matches!(answer, Err(TelegramError::SignIn(_))) {
            self.signed_out.store(true, Ordering::SeqCst);
        }
        answer
    }
}

impl TelegramPost for KeptPost {
    fn me(&self) -> Result<Me, TelegramError> {
        self.noted(self.live()?.me())
    }
    fn post(&self, chat: &telegram::ChatRef, text: &str) -> Result<(i32, i64), telegram::PostError> {
        let posted = self.live().map_err(telegram::PostError::NotSent)?.post(chat, text);
        if matches!(posted, Err(telegram::PostError::NotSent(TelegramError::SignIn(_)))) {
            self.signed_out.store(true, Ordering::SeqCst);
        }
        posted
    }
    fn after(&self, chat: &telegram::ChatRef, after: i32) -> Result<Vec<telegram::Said>, TelegramError> {
        self.noted(self.live()?.after(chat, after))
    }
}

impl Drop for KeptPost {
    fn drop(&mut self) {
        if let Some(post) = self.post.take() {
            if !self.signed_out.load(Ordering::SeqCst) {
                keep_session(&self.state, &self.source_id, post.session(), &self.read);
            }
            post.close();
        }
    }
}

/// The sign-in under way, taken out to be worked on, if there is one still
/// young enough.
fn take_pending(state: &crate::AppState) -> Option<PendingTelegram> {
    let mut held = state.telegram.lock().unwrap_or_else(|p| p.into_inner());
    held.take().filter(|p| p.started.elapsed() < PENDING_FOR)
}

fn put_back(state: &crate::AppState, pending: PendingTelegram) {
    *state.telegram.lock().unwrap_or_else(|p| p.into_inner()) = Some(pending);
}

const START_AGAIN: &str = "That sign-in has ended. Start again: I'll ask Telegram for a new code.";

/// Whether the Telegram app registration is kept, and its id.
#[tauri::command]
pub fn telegram_app(state: State<'_, SharedState>) -> TelegramApp {
    let app = saved_app(&state);
    TelegramApp { saved: app.is_some(), api_id: app.map(|a| a.api_id) }
}

/// Ask Telegram to send a sign-in code to `phone`, with the app registration
/// given (kept once Telegram has taken it) or the one kept before.
#[tauri::command]
pub async fn start_telegram_sign_in(
    state: State<'_, SharedState>,
    api_id: Option<String>,
    api_hash: Option<String>,
    phone: String,
) -> CommandResult<TelegramStep> {
    crate::commands::sources::refuse_without_identity(&state)?;
    let given = match (api_id.as_deref().map(str::trim), api_hash.as_deref().map(str::trim)) {
        (Some(id), Some(hash)) if !id.is_empty() || !hash.is_empty() => {
            Some(AppCredentials::checked(id, hash).map_err(|why| CommandError::new("invalid", why))?)
        }
        _ => None,
    };
    let Some(app) = given.clone().or_else(|| saved_app(&state)) else {
        return Err(CommandError::new(
            "app",
            "First give me the App api_id and App api_hash from my.telegram.org → API development tools.",
        ));
    };
    let phone = phone.trim().to_string();
    if phone.chars().filter(char::is_ascii_digit).count() < 6 {
        return Err(CommandError::new("invalid", "Your phone number, with its country code, like +1 808 555 0123."));
    }
    // One sign-in at a time: an earlier one is closed, and a step of it
    // still waiting on Telegram goes no further.
    let number = SIGN_IN.fetch_add(1, Ordering::SeqCst) + 1;
    if let Some(earlier) = take_pending(&state) {
        earlier.live.close().await;
    }
    let live = Live::start(&app, Arc::new(KeptSession::default()));
    let asked = live.request_code(&phone, &app.api_hash).await;
    if SIGN_IN.load(Ordering::SeqCst) != number {
        live.close().await;
        return Err(CommandError::new("ended", STOPPED));
    }
    match asked {
        Ok(login) => {
            // Telegram took the registration: it is kept for next time.
            if given.is_some() {
                let raw = serde_json::to_string(&app).map_err(|e| CommandError::new("internal", e.to_string()))?;
                state
                    .secrets
                    .set(telegram::APP_KEY, &raw)
                    .map_err(|e| CommandError::new("secrets", format!("I couldn't keep the app's id and hash: {e}")))?;
            }
            put_back(&state, PendingTelegram { live, login, password: None, started: Instant::now(), number });
            Ok(TelegramStep::code())
        }
        Err(e) => {
            live.close().await;
            Err(telegram_error(e))
        }
    }
}

/// Sign in with the code Telegram sent.
#[tauri::command]
pub async fn telegram_sign_in_code(state: State<'_, SharedState>, code: String) -> CommandResult<TelegramStep> {
    let pending = take_pending(&state).ok_or_else(|| CommandError::new("ended", START_AGAIN))?;
    let step = pending.live.sign_in(&pending.login, &code).await;
    if SIGN_IN.load(Ordering::SeqCst) != pending.number {
        return Err(stopped(pending, matches!(step, Ok(Step::Done(_)))).await);
    }
    match step {
        Ok(Step::Done(me)) => finish(state.inner(), pending, me).await,
        Ok(Step::Password { token, hint, wrong }) => {
            put_back(&state, PendingTelegram { password: Some(token), ..pending });
            Ok(TelegramStep::password(hint, wrong))
        }
        // Another try at the code, with the same sign-in.
        Err(TelegramError::BadCode) => {
            put_back(&state, pending);
            Err(telegram_error(TelegramError::BadCode))
        }
        Err(e) => {
            pending.live.close().await;
            Err(telegram_error(e))
        }
    }
}

/// Sign in with the account's password. It is handed to Telegram's check
/// and kept nowhere, not even in memory once this returns.
#[tauri::command]
pub async fn telegram_sign_in_password(state: State<'_, SharedState>, password: String) -> CommandResult<TelegramStep> {
    let mut pending = take_pending(&state).ok_or_else(|| CommandError::new("ended", START_AGAIN))?;
    let Some(token) = pending.password.take() else {
        put_back(&state, pending);
        return Err(CommandError::new("ended", "Telegram hasn't asked for a password. Give it the code first."));
    };
    let step = pending.live.check_password(token, &password).await;
    drop(password);
    if SIGN_IN.load(Ordering::SeqCst) != pending.number {
        return Err(stopped(pending, matches!(step, Ok(Step::Done(_)))).await);
    }
    match step {
        Ok(Step::Done(me)) => finish(state.inner(), pending, me).await,
        Ok(Step::Password { token, hint, .. }) => {
            put_back(&state, PendingTelegram { password: Some(token), ..pending });
            Ok(TelegramStep::password(hint, true))
        }
        Err(e) => {
            pending.live.close().await;
            Err(telegram_error(e))
        }
    }
}

/// Try a connected account's session as a reply would use it, sending
/// nothing (0.10.0-alpha.76): who it is, and that it is the account
/// connected. A sentence saying what was found.
#[tauri::command]
pub async fn test_telegram(state: State<'_, SharedState>, source_id: String) -> CommandResult<String> {
    let source = state
        .db
        .get_source(&source_id)?
        .filter(|s| s.connector == telegram::CONNECTOR)
        .ok_or_else(|| CommandError::new("not_found", "That account isn't connected any more."))?;
    let config: telegram::TelegramSourceConfig = serde_json::from_value(source.config)
        .map_err(|_| CommandError::new("unreadable", "I can't read its settings; remove it and connect it again."))?;
    let account = session_for(&state, &source_id).ok_or_else(|| CommandError::new("telegram", NO_SESSION))?;
    let shared = state.inner().clone();
    let handle = tokio::runtime::Handle::current();
    let me = tauri::async_runtime::spawn_blocking(move || {
        let post = KeptPost::start(handle, shared, &source_id, account);
        post.me()
    })
    .await
    .map_err(|e| CommandError::new("internal", e.to_string()))?
    .map_err(telegram_error)?;
    if me.id != config.user_id {
        return Err(CommandError::new(
            "not_this",
            format!("Its sign-in is {}'s, not the account you connected. Sign in again.", me.name),
        ));
    }
    Ok(format!("Signed in to Telegram as {}. Nothing was sent.", me.name))
}

/// Stop a sign-in under way — also one whose code or password Telegram is
/// checking right now (0.10.0-alpha.77).
#[tauri::command]
pub async fn cancel_telegram_sign_in(state: State<'_, SharedState>) -> CommandResult<()> {
    SIGN_IN.fetch_add(1, Ordering::SeqCst);
    if let Some(pending) = take_pending(&state) {
        pending.live.close().await;
    }
    Ok(())
}

/// A sign-in the user stopped while Telegram was checking its code or
/// password: closed, and if Telegram had already let it in, signed out
/// again, so it isn't left among the account's sessions.
async fn stopped(pending: PendingTelegram, signed_in: bool) -> CommandError {
    if signed_in {
        pending.live.sign_out().await;
    }
    pending.live.close().await;
    CommandError::new("ended", STOPPED)
}

fn check_soon(state: &crate::AppState, source_id: &str) -> CommandResult<()> {
    let queued = state.db.list_jobs(500, true)?.into_iter().any(|j| {
        j.kind == telegram::JOB_KIND && j.status == "queued" && j.payload["sourceId"].as_str() == Some(source_id)
    });
    if !queued {
        state.jobs.enqueue(telegram::JOB_KIND, json!({ "sourceId": source_id }))?;
    }
    Ok(())
}

/// Signed in as `me`: keep the session — for the account already connected
/// as them (signing in again), or a new one, which becomes the user's — and
/// start a check.
async fn finish(state: &SharedState, pending: PendingTelegram, me: Me) -> CommandResult<TelegramStep> {
    let kept = pending.live.session().to_kept().map_err(|e| CommandError::new("internal", e.to_string()));
    pending.live.close().await;
    let kept = kept?;
    if let Some(source_id) = telegram::connected_as(&state.db, me.id)? {
        state
            .secrets
            .set(&telegram::secret_key(&source_id), &kept)
            .map_err(|e| CommandError::new("secrets", format!("I couldn't keep the sign-in: {e}")))?;
        state.db.set_source_status(&source_id, "ready", None)?;
        check_soon(state, &source_id)?;
        let source = state
            .db
            .get_source(&source_id)?
            .ok_or_else(|| CommandError::new("not_found", "That account isn't connected any more."))?;
        return Ok(TelegramStep::done(source));
    }
    // Signed in as them, so whoever it is, is them.
    state.db.add_user_identifier(IdentifierKind::AccountId, &telegram::account_id(me.id))?;
    crate::commands::people::reconcile_identity(&state.db, &state.jobs, "connect_telegram");
    let new = telegram::new_source(&me).map_err(|e| CommandError::new("internal", e.to_string()))?;
    let source = state.db.create_source(&new)?;
    if let Err(e) = state.secrets.set(&telegram::secret_key(&source.id), &kept) {
        // Without it the account can never be read; nothing is left behind
        // that looks connected and isn't.
        let _ = state.db.delete_source_and_contents(&source.id);
        return Err(CommandError::new("secrets", format!("I couldn't keep the sign-in: {e}")));
    }
    state.db.set_source_status(&source.id, "ready", None)?;
    check_soon(state, &source.id)?;
    Ok(TelegramStep::done(state.db.get_source(&source.id)?.unwrap_or(source)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_telegram_sign_in_shows_matches_its_fixture() {
        let app = TelegramApp { saved: true, api_id: Some(1234567) };
        crate::fixtures::check_fixture("telegram_app.json", &serde_json::to_value(&app).unwrap());
        let step = TelegramStep::password(Some("the usual".into()), true);
        let json = serde_json::to_value(&step).unwrap();
        assert!(json.to_string().contains("\"wrong\":true"), "{json}");
        crate::fixtures::check_fixture("telegram_step.json", &json);
    }

    #[test]
    fn a_wrong_code_says_so_in_its_own_words() {
        assert_eq!(telegram_error(TelegramError::BadCode).code, "bad_code");
        assert_eq!(telegram_error(TelegramError::App("API_ID_INVALID".into())).code, "app");
        assert_eq!(telegram_error(TelegramError::NoAccount).code, "telegram");
    }
}
