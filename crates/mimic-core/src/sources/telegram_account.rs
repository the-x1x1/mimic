//! The `telegram_account` connector: a Telegram account's chats, read as the
//! user while they happen (0.10.0-alpha.74), beside `telegram`, which reads
//! an export of them.
//!
//! Telegram lets an app act as a person only through its own API (MTProto),
//! under an app id and hash the person registers at my.telegram.org, after a
//! sign-in with their phone number, the code Telegram sends them and, when
//! they set one, their password — which goes to Telegram and nowhere else.
//! What the sign-in leaves (the session's keys, `KeptSession`) and the app's
//! id and hash are kept in the secret store (`secret_key`, `APP_KEY`), never
//! in the source's settings. The client is grammers (`live`).
//!
//! A check (the `check_telegram` job) asks Telegram for the account's chats,
//! reads what is new in each one-to-one chat and private group after the
//! last message read (`marks`; the newest `FIRST_READ` the first time), and
//! imports it (`check`) under the ids an export of the same chat has —
//! conversations `telegram:chat:<id>`, messages `telegram:<chat>:<id>`,
//! writers `telegram:user<id>` — so a chat read both ways is one
//! conversation (`import::SAME_IDS_FAMILIES`). Bots, channels, public groups,
//! notices, forwarded messages and messages without words are left out.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use chrono::{SecondsFormat, TimeZone, Utc};
use grammers_session::types::{ChannelState, DcOption, PeerId, PeerInfo, UpdateState, UpdatesState};
use grammers_session::{BoxFuture, Session, SessionData};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{AuthorRef, DiscoveredConversation, RawMessage};
use crate::db::repo_people::IdentifierInput;
use crate::db::{Db, DbError, IdentifierKind};
use crate::import::{ImportError, Importer};

pub use grammers_client::client::{LoginToken, PasswordToken};

pub const CONNECTOR: &str = "telegram_account";
pub const JOB_KIND: &str = "check_telegram";
/// Where the app id and hash the user registered are kept: one for every
/// account they connect.
pub const APP_KEY: &str = "telegram:app";

/// How many of a chat's newest messages the first check reads.
pub const FIRST_READ: usize = 300;
/// How many new messages one check reads from a chat at most; more marks
/// the check partial.
pub const MOST_NEW: usize = 2_000;
/// How many chats one check reads, most recent first; the rest are read by
/// the next. Chats with nothing new, and bots, channels and public groups
/// passed over, don't count (0.10.0-alpha.77).
pub const MOST_CHATS: usize = 200;
/// How far down the chat list a check looks at all.
const MOST_DIALOGS: usize = 1_000;
/// How far below the newest message the last full read saw a chat's own
/// newest may sit and still be looked at: messages of the same moment, by
/// Telegram's one clock (0.10.0-alpha.78).
const SAME_MOMENT: i64 = 60;
/// How long one request to Telegram may take.
const TIMEOUT: Duration = Duration::from_secs(60);
/// The longest Telegram's "slow down" the client waits out by itself, well
/// inside `TIMEOUT`: a longer one is an answer (`RateLimited`).
const WAITS_OUT: Duration = Duration::from_secs(20);
/// Telegram's own accounts, which send login codes and security notices
/// (777000, and 333000 before it): not conversations, and never imported.
const TELEGRAM_ITSELF: [i64; 2] = [777_000, 333_000];

/// Where an account's session is kept in the secret store.
pub fn secret_key(source_id: &str) -> String {
    format!("telegram:{source_id}")
}

/// A person's address on Telegram: as an export names them (`from_id`).
pub fn account_id(user_id: i64) -> String {
    format!("telegram:user{user_id}")
}

/// The app the user registered at my.telegram.org.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppCredentials {
    pub api_id: i32,
    pub api_hash: String,
}

impl AppCredentials {
    /// What the user pasted, checked: a positive id, and a hash of 32
    /// hexadecimal digits.
    pub fn checked(api_id: &str, api_hash: &str) -> Result<Self, &'static str> {
        let api_id: i32 = api_id.trim().parse().map_err(|_| "The App api_id is a number, like 1234567.")?;
        let api_hash = api_hash.trim().to_ascii_lowercase();
        if api_id <= 0 {
            return Err("The App api_id is a number, like 1234567.");
        }
        if api_hash.len() != 32 || !api_hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("The App api_hash is 32 letters and digits, from the same page.");
        }
        Ok(AppCredentials { api_id, api_hash })
    }
}

/// Why Telegram couldn't be read, or a sign-in didn't go through.
#[derive(Debug, thiserror::Error)]
pub enum TelegramError {
    #[error("Telegram wants you to sign in again: connect the account again under Your mail ({0}).")]
    SignIn(String),
    #[error("Telegram didn't accept the app's api_id and api_hash ({0}). Copy both again from my.telegram.org.")]
    App(String),
    #[error("That code isn't right, or it has expired. Ask for a new one.")]
    BadCode,
    #[error("This phone number has no Telegram account. Sign up in Telegram's own app first.")]
    NoAccount,
    #[error("Telegram said no: {0}")]
    Refused(String),
    #[error("I couldn't reach Telegram: {0}")]
    Network(String),
    #[error("Telegram asked me to slow down; I'll read the rest on the next check.")]
    RateLimited,
    #[error("{0}")]
    Import(String),
    #[error("canceled")]
    Canceled,
}

impl From<DbError> for TelegramError {
    fn from(e: DbError) -> Self {
        TelegramError::Import(e.to_string())
    }
}

impl From<ImportError> for TelegramError {
    fn from(e: ImportError) -> Self {
        TelegramError::Import(e.to_string())
    }
}

/// Telegram's answer, as Mimic reads it: the sign-in (401), slowing down
/// (420 for longer than the client waits by itself), the app's registration,
/// its own failures (500 and up, and the -500s it says "Timeout" with),
/// which pass, like the network's (0.10.0-alpha.78), and anything else
/// Telegram refused, by its name.
fn read_error(e: grammers_client::InvocationError) -> TelegramError {
    use grammers_client::InvocationError as E;
    match &e {
        E::Rpc(rpc) if rpc.code == 401 => TelegramError::SignIn(rpc.name.clone()),
        E::Rpc(rpc) if rpc.code == 420 => TelegramError::RateLimited,
        E::Rpc(rpc) if rpc.code >= 500 || rpc.code <= -500 => TelegramError::Network(rpc.name.clone()),
        E::Rpc(rpc) if rpc.name.starts_with("API_ID") => TelegramError::App(rpc.name.clone()),
        E::Rpc(rpc) => TelegramError::Refused(rpc.name.clone()),
        _ => TelegramError::Network(e.to_string()),
    }
}

/// Why a reply didn't go, or may have: Telegram's own failure (500 and up,
/// or the -500s it says "Timeout" with), its saying it has this request
/// already, and anything that isn't Telegram's answer (the connection) may
/// all come after it took the message; any other refusal means it didn't
/// (0.10.0-alpha.77).
fn post_failed(e: grammers_client::InvocationError) -> PostError {
    use grammers_client::InvocationError as E;
    match &e {
        E::Rpc(rpc) if rpc.code >= 500 || rpc.code <= -500 => PostError::Unsure(rpc.name.clone()),
        E::Rpc(rpc) if rpc.name == "RANDOM_ID_DUPLICATE" => PostError::Unsure(rpc.name.clone()),
        E::Rpc(_) => PostError::NotSent(read_error(e)),
        _ => PostError::Unsure(e.to_string()),
    }
}

/// Whose account a sign-in, or a session, is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub username: Option<String>,
}

/// An account connected for its chats.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramSourceConfig {
    pub user_id: i64,
    pub name: String,
    #[serde(default)]
    pub username: Option<String>,
    /// The newest message read, per chat.
    #[serde(default)]
    pub marks: BTreeMap<String, i32>,
    #[serde(default)]
    pub last_checked_at: Option<String>,
    /// When the last check that read everything through began.
    #[serde(default)]
    pub last_read_at: Option<String>,
    /// The newest message, by Telegram's clock (seconds since 1970), that
    /// the last check that read everything through saw: a chat whose own
    /// newest is older was read then (0.10.0-alpha.78).
    #[serde(default)]
    pub read_through: Option<i64>,
}

impl TelegramSourceConfig {
    pub fn for_account(me: &Me) -> Self {
        TelegramSourceConfig {
            user_id: me.id,
            name: me.name.clone(),
            username: me.username.clone(),
            ..Default::default()
        }
    }
}

/// A new account's source, read as chat.
pub fn new_source(me: &Me) -> Result<crate::db::NewSource, serde_json::Error> {
    Ok(crate::db::NewSource {
        connector: CONNECTOR.into(),
        name: format!("{} (Telegram)", me.name),
        channel: "chat".into(),
        location: None,
        config: serde_json::to_value(TelegramSourceConfig::for_account(me))?,
    })
}

/// The source already connected as this account, if one is.
pub fn connected_as(db: &Db, user_id: i64) -> Result<Option<String>, DbError> {
    Ok(db
        .list_sources()?
        .into_iter()
        .filter(|s| s.connector == CONNECTOR)
        .find(|s| serde_json::from_value::<TelegramSourceConfig>(s.config.clone()).is_ok_and(|c| c.user_id == user_id))
        .map(|s| s.id))
}

// ------------------------------------------------------------ what was read

/// One message, as a check needs it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Said {
    pub id: i32,
    /// Seconds since 1970, by Telegram's clock.
    pub at: i64,
    pub text: String,
    pub outgoing: bool,
    /// Who wrote it, when Telegram said and it was a person: id and name.
    pub from: Option<(i64, String)>,
    pub bot: bool,
    /// A join, a call, a pinned message: Telegram's own line.
    pub notice: bool,
    /// A photo, a file, a voice message, a sticker, a link's preview.
    pub media: bool,
    pub forwarded: bool,
}

/// One chat's new messages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chat {
    /// Telegram's own id for the chat: the other person's for a one-to-one.
    pub id: i64,
    pub title: String,
    /// One-to-one, with a person.
    pub direct: bool,
    /// What Telegram calls it: `user`, `chat` (a small group) or `channel`
    /// (a supergroup) — which a reply needs to say where it goes.
    pub kind: String,
    /// Its newest message, read or not.
    pub newest: i32,
    pub messages: Vec<Said>,
    /// Everything new was read (not more than `MOST_NEW`).
    pub complete: bool,
}

/// What one look at the account found, before anything is imported.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fetched {
    pub me: Option<Me>,
    pub chats: Vec<Chat>,
    /// Chats whose newest message was already read.
    pub unchanged: usize,
    /// Bots, channels, public groups, Saved Messages: not read.
    pub left: usize,
    /// Chats Telegram wouldn't give, by its reason.
    pub refused: Vec<String>,
    /// Telegram asked to slow down before every chat was read.
    pub partial: bool,
    /// The newest message among the chats listed, by Telegram's clock.
    pub newest_at: Option<i64>,
    /// The list was read down to where the last full read had read
    /// everything: the rest had nothing new (0.10.0-alpha.78).
    pub rest_unchanged: bool,
}

/// What one check did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramSummary {
    pub chats: usize,
    pub fetched: usize,
    pub inserted: usize,
    pub duplicates: usize,
    pub from_self: usize,
    pub left_out: usize,
    pub unchanged: usize,
    pub unreadable: usize,
    pub partial: bool,
}

fn at_time(secs: i64) -> Option<String> {
    Utc.timestamp_opt(secs, 0).single().map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true))
}

/// Mark the account as being read and hand back its settings, or say why
/// it can't be read at all.
pub fn begin(db: &Db, source_id: &str) -> Result<TelegramSourceConfig, TelegramError> {
    let source = db.get_source(source_id)?.ok_or_else(|| TelegramError::Import(format!("no source {source_id}")))?;
    match serde_json::from_value::<TelegramSourceConfig>(source.config) {
        Ok(config) => {
            db.set_source_status(source_id, "importing", None)?;
            Ok(config)
        }
        Err(e) => {
            let err = TelegramError::Import(format!(
                "this account's settings are unreadable ({e}); remove it and connect it again"
            ));
            db.set_source_status(source_id, "failed", Some(&json!({ "message": err.to_string() })))?;
            Err(err)
        }
    }
}

/// Import what a look at the account found (or record why there was none),
/// move each chat's mark past what was read, and say how it went.
pub fn finish(
    db: &Db,
    source_id: &str,
    mut config: TelegramSourceConfig,
    began: &str,
    fetched: Result<Fetched, TelegramError>,
) -> Result<TelegramSummary, TelegramError> {
    let newest_at = fetched.as_ref().ok().and_then(|f| f.newest_at);
    let result = fetched.and_then(|f| import(db, source_id, &mut config, f));
    config.last_checked_at = Some(crate::ids::now_rfc3339());
    if matches!(&result, Ok(s) if !s.partial) {
        config.last_read_at = Some(began.to_string());
        if let Some(at) = newest_at {
            config.read_through = Some(config.read_through.map_or(at, |was| was.max(at)));
        }
    }
    db.set_source_config(source_id, &serde_json::to_value(&config).unwrap_or(Value::Null))?;
    match &result {
        Ok(_) => db.set_source_status(source_id, "imported", None)?,
        Err(TelegramError::Canceled) => db.set_source_status(source_id, "ready", None)?,
        Err(e) => db.set_source_status(source_id, "failed", Some(&json!({ "message": e.to_string() })))?,
    }
    result
}

/// `begin` and `finish` around what was found: a check as the tests see it.
pub fn check(
    db: &Db,
    source_id: &str,
    fetched: Result<Fetched, TelegramError>,
) -> Result<TelegramSummary, TelegramError> {
    let config = begin(db, source_id)?;
    let began = crate::ids::now_rfc3339();
    finish(db, source_id, config, &began, fetched)
}

fn import(
    db: &Db,
    source_id: &str,
    config: &mut TelegramSourceConfig,
    fetched: Fetched,
) -> Result<TelegramSummary, TelegramError> {
    if let Some(me) = &fetched.me {
        if me.id != config.user_id {
            return Err(TelegramError::Refused(format!(
                "this sign-in is {}'s, not the account this was connected with",
                me.name
            )));
        }
    }
    let mut summary = TelegramSummary {
        unchanged: fetched.unchanged,
        unreadable: fetched.refused.len(),
        partial: fetched.partial,
        ..Default::default()
    };
    // Not one chat Telegram would give, and none passed over for having
    // nothing new or not being a conversation: that is the account, not a
    // chat.
    let gave_some = fetched.unchanged > 0 || fetched.left > 0 || fetched.rest_unchanged;
    if fetched.chats.is_empty() && !fetched.refused.is_empty() && !gave_some {
        return Err(TelegramError::Refused(format!(
            "it gave none of your chats ({})",
            fetched.refused.last().cloned().unwrap_or_default()
        )));
    }
    let me = account_id(config.user_id);
    let mut importer = Importer::begin(db, source_id)?;
    for chat in fetched.chats {
        summary.fetched += chat.messages.len();
        if !chat.complete {
            summary.partial = true;
        }
        let mut messages: Vec<RawMessage> = Vec::new();
        for m in &chat.messages {
            let words = m.text.trim();
            if m.notice || m.bot || m.forwarded || words.is_empty() {
                summary.left_out += 1;
                continue;
            }
            let author = if m.outgoing {
                AuthorRef {
                    display_name: config.name.clone(),
                    identifiers: vec![IdentifierInput::new(IdentifierKind::AccountId, me.clone())],
                }
            } else {
                // In a one-to-one chat, whoever else wrote is the other
                // person, whether or not Telegram said so on the message.
                let from = m.from.clone().or_else(|| chat.direct.then(|| (chat.id, chat.title.clone())));
                let Some((id, name)) = from else {
                    summary.left_out += 1;
                    continue;
                };
                AuthorRef {
                    display_name: if name.trim().is_empty() { format!("user{id}") } else { name },
                    identifiers: vec![IdentifierInput::new(IdentifierKind::AccountId, account_id(id))],
                }
            };
            messages.push(RawMessage {
                external_id: format!("telegram:{}:{}", chat.id, m.id),
                author,
                sent_at: at_time(m.at),
                body: words.to_string(),
                // Which chat it is in, whether it is one-to-one (only those
                // are answered by itself), and whether something came with
                // it a reply couldn't have read.
                // And whose account it was read through: a chat's
                // messages are numbered per account (`import::account_of`).
                metadata: json!({
                    "telegramChat": chat.id.to_string(),
                    "telegramMessage": m.id,
                    "telegramDirect": chat.direct,
                    "telegramKind": chat.kind,
                    "telegramAccount": me,
                    "attachments": m.media,
                }),
            });
        }
        messages.sort_by_key(|m| m.external_id.rsplit(':').next().and_then(|id| id.parse::<i64>().ok()).unwrap_or(0));
        if !messages.is_empty() {
            if let Err(e) = importer.take(DiscoveredConversation {
                external_id: format!("telegram:chat:{}", chat.id),
                subject: Some(chat.title.clone()),
                channel: "chat".into(),
                messages,
            }) {
                importer.abandon();
                return Err(e.into());
            }
            summary.chats += 1;
        }
        // Read through: past the chat's newest. Cut short (more than
        // `MOST_NEW`, or the check stopped in it): only past what was read,
        // the oldest first, so the next check reads on from there and the
        // newest are never passed over (0.10.0-alpha.77).
        let read = chat.messages.iter().map(|m| m.id).max().unwrap_or(0);
        let mark = config.marks.entry(chat.id.to_string()).or_insert(0);
        *mark = if chat.complete { (*mark).max(chat.newest).max(read) } else { (*mark).max(read) };
    }
    let imported = importer.finish()?;
    summary.inserted = imported.inserted;
    summary.duplicates = imported.duplicates;
    summary.from_self = imported.from_self;
    Ok(summary)
}

/// A check that couldn't start (no sign-in, no app): recorded as an attempt,
/// so the schedule waits before the next.
pub fn record_failure(db: &Db, source_id: &str, message: &str) -> Result<(), DbError> {
    if let Some(source) = db.get_source(source_id)? {
        if let Ok(mut config) = serde_json::from_value::<TelegramSourceConfig>(source.config) {
            config.last_checked_at = Some(crate::ids::now_rfc3339());
            db.set_source_config(source_id, &serde_json::to_value(&config).unwrap_or(Value::Null))?;
        }
        db.set_source_status(source_id, "failed", Some(&json!({ "message": message })))?;
    }
    Ok(())
}

// -------------------------------------------------------------- schedule

/// Accounts due a check at `now`, on the mailboxes' interval: connected,
/// not already queued or being checked, last tried longer ago than that.
pub fn due(db: &Db, now: chrono::DateTime<Utc>) -> Result<Vec<String>, DbError> {
    let minutes = super::imap::interval_minutes(db)?;
    if minutes == 0 {
        return Ok(Vec::new());
    }
    let active: Vec<String> = db
        .list_jobs(500, true)?
        .into_iter()
        .filter(|j| j.kind == JOB_KIND)
        .filter_map(|j| j.payload["sourceId"].as_str().map(str::to_string))
        .collect();
    let mut out = Vec::new();
    for s in db.list_sources()? {
        if s.connector != CONNECTOR || active.contains(&s.id) {
            continue;
        }
        let Ok(config) = serde_json::from_value::<TelegramSourceConfig>(s.config.clone()) else { continue };
        let wait = if s.status == "failed" { (minutes * 4).min(360) } else { minutes };
        let last = config
            .last_checked_at
            .as_deref()
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok().map(|d| d.with_timezone(&Utc)));
        if last.is_none_or(|at| now.signed_duration_since(at) >= chrono::Duration::minutes(wait)) {
            out.push(s.id);
        }
    }
    Ok(out)
}

pub fn due_now(db: &Db) -> Result<Vec<String>, DbError> {
    due(db, Utc::now())
}

/// At start-up: an account left "importing" by a check that never finished
/// is ready again, unless a check for it is queued.
pub fn recover(db: &Db) -> Result<usize, DbError> {
    let active: Vec<String> = db
        .list_jobs(500, true)?
        .into_iter()
        .filter(|j| j.kind == JOB_KIND)
        .filter_map(|j| j.payload["sourceId"].as_str().map(str::to_string))
        .collect();
    let mut n = 0;
    for s in db.list_sources()? {
        if s.connector == CONNECTOR && s.status == "importing" && !active.contains(&s.id) {
            db.set_source_status(&s.id, "ready", None)?;
            n += 1;
        }
    }
    Ok(n)
}

// --------------------------------------------------------------- session

/// What the session keeps between runs: the datacenter the account lives
/// in, each datacenter's address and key, and who it has met.
#[derive(Default, Serialize, Deserialize)]
struct Kept {
    home_dc: i32,
    dc_options: Vec<DcOption>,
    #[serde(default)]
    peers: Vec<PeerInfo>,
    #[serde(default)]
    updates: UpdatesState,
}

struct State {
    home_dc: i32,
    dc: HashMap<i32, DcOption>,
    peers: HashMap<PeerId, PeerInfo>,
    updates: UpdatesState,
}

/// The lock on a session was poisoned by a panic.
#[derive(Debug, thiserror::Error)]
#[error("the Telegram session's lock was poisoned")]
pub struct SessionPoisoned;

/// A Telegram session that can be written out whole (`to_kept`) for the
/// secret store and read back (`from_kept`): grammers' own in-memory one
/// can't be read out.
pub struct KeptSession(Mutex<State>);

impl Default for KeptSession {
    fn default() -> Self {
        let fresh = SessionData::default();
        KeptSession(Mutex::new(State {
            home_dc: fresh.home_dc,
            dc: fresh.dc_options,
            peers: HashMap::new(),
            updates: UpdatesState::default(),
        }))
    }
}

impl KeptSession {
    fn state(&self) -> Result<MutexGuard<'_, State>, SessionPoisoned> {
        self.0.lock().map_err(|_| SessionPoisoned)
    }

    /// A session kept before: over the datacenters Telegram is known to
    /// have, those it was told about and the keys it made.
    pub fn from_kept(kept: &str) -> Result<Self, serde_json::Error> {
        let kept: Kept = serde_json::from_str(kept)?;
        let session = KeptSession::default();
        if let Ok(mut state) = session.state() {
            state.home_dc = kept.home_dc;
            for option in kept.dc_options {
                state.dc.insert(option.id, option);
            }
            for peer in kept.peers {
                state.peers.insert(peer.id(), peer);
            }
            state.updates = kept.updates;
        }
        Ok(session)
    }

    /// The whole session, to keep. It holds the keys that sign in as the
    /// account: only for the secret store.
    /// Written out in one order, so a session nothing changed in is the
    /// same text as when it was read (0.10.0-alpha.78).
    pub fn to_kept(&self) -> Result<String, SessionPoisoned> {
        let state = self.state()?;
        let mut dc_options: Vec<DcOption> = state.dc.values().cloned().collect();
        dc_options.sort_by_key(|o| o.id);
        let mut peers: Vec<PeerInfo> = state.peers.values().cloned().collect();
        peers.sort_by_cached_key(|p| serde_json::to_string(p).unwrap_or_default());
        let mut updates = state.updates.clone();
        updates.channels.sort_by_key(|c| c.id);
        let kept = Kept { home_dc: state.home_dc, dc_options, peers, updates };
        serde_json::to_string(&kept).map_err(|_| SessionPoisoned)
    }
}

impl Session for KeptSession {
    type Error = SessionPoisoned;

    fn home_dc_id(&self) -> Result<i32, SessionPoisoned> {
        Ok(self.state()?.home_dc)
    }

    fn set_home_dc_id(&self, dc_id: i32) -> BoxFuture<'_, Result<(), SessionPoisoned>> {
        Box::pin(async move {
            self.state()?.home_dc = dc_id;
            Ok(())
        })
    }

    fn dc_option(&self, dc_id: i32) -> Result<Option<DcOption>, SessionPoisoned> {
        Ok(self.state()?.dc.get(&dc_id).cloned())
    }

    fn set_dc_option(&self, dc_option: &DcOption) -> BoxFuture<'_, Result<(), SessionPoisoned>> {
        let dc_option = dc_option.clone();
        Box::pin(async move {
            self.state()?.dc.insert(dc_option.id, dc_option);
            Ok(())
        })
    }

    fn peer(&self, peer: PeerId) -> BoxFuture<'_, Result<Option<PeerInfo>, SessionPoisoned>> {
        Box::pin(async move {
            let state = self.state()?;
            // The account itself is asked for by a stand-in id.
            if peer == PeerId::self_user() {
                return Ok(state
                    .peers
                    .values()
                    .find(|p| matches!(p, PeerInfo::User { is_self: Some(true), .. }))
                    .cloned());
            }
            Ok(state.peers.get(&peer).cloned())
        })
    }

    fn cache_peer(&self, peer: PeerInfo) -> BoxFuture<'_, Result<(), SessionPoisoned>> {
        Box::pin(async move {
            let mut state = self.state()?;
            let known = state.peers.entry(peer.id()).or_insert_with(|| peer.clone());
            let _ = known.extend_info(&peer);
            Ok(())
        })
    }

    fn updates_state(&self) -> BoxFuture<'_, Result<UpdatesState, SessionPoisoned>> {
        Box::pin(async move { Ok(self.state()?.updates.clone()) })
    }

    fn set_update_state(&self, update: UpdateState) -> BoxFuture<'_, Result<(), SessionPoisoned>> {
        Box::pin(async move {
            let mut state = self.state()?;
            match update {
                UpdateState::All(all) => state.updates = all,
                UpdateState::Primary { pts, date, seq } => {
                    state.updates.pts = pts;
                    state.updates.date = date;
                    state.updates.seq = seq;
                }
                UpdateState::Secondary { qts } => state.updates.qts = qts,
                UpdateState::Channel { id, pts } => {
                    state.updates.channels.retain(|c| c.id != id);
                    state.updates.channels.push(ChannelState { id, pts });
                }
            }
            Ok(())
        })
    }
}

// --------------------------------------------------------------- sending

/// A chat a reply goes into: Telegram's id for it, and what it is (`user`,
/// `chat` or `channel`, as a check recorded it in `telegramKind`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatRef {
    pub id: i64,
    pub kind: String,
}

impl ChatRef {
    /// The chat a message read from Telegram came from.
    pub fn of(metadata: &Value) -> Option<ChatRef> {
        let id = metadata.get("telegramChat").and_then(Value::as_str)?.parse().ok()?;
        let kind = match metadata.get("telegramKind").and_then(Value::as_str) {
            Some(kind) => kind.to_string(),
            // One-to-one is with a user; anything else is said by its kind.
            None if metadata.get("telegramDirect").and_then(Value::as_bool) == Some(true) => "user".to_string(),
            None => return None,
        };
        Some(ChatRef { id, kind })
    }

    fn peer_id(&self) -> Option<PeerId> {
        match self.kind.as_str() {
            "user" => PeerId::user(self.id),
            "chat" => PeerId::chat(self.id),
            "channel" => PeerId::channel(self.id),
            _ => None,
        }
    }
}

/// Why a reply didn't go, or may have.
#[derive(Debug, thiserror::Error)]
pub enum PostError {
    /// Telegram said no, or it was never asked: nothing went.
    #[error(transparent)]
    NotSent(TelegramError),
    /// Asked, and the answer was lost, or didn't say what it made: it may
    /// have gone.
    #[error("Telegram may have taken it: {0}.")]
    Unsure(String),
}

/// What sending a reply into Telegram, and checking before one goes by
/// itself, asks of it — real (`LivePost`) or scripted in tests.
pub trait TelegramPost {
    /// Whose account the session is.
    fn me(&self) -> Result<Me, TelegramError>;
    /// Send `text` into `chat` as the account, as plain text: the message's
    /// id and when it went (seconds since 1970, Telegram's clock).
    fn post(&self, chat: &ChatRef, text: &str) -> Result<(i32, i64), PostError>;
    /// What was said in `chat` after message `after`, oldest first (at most
    /// fifty).
    fn after(&self, chat: &ChatRef, after: i32) -> Result<Vec<Said>, TelegramError>;
}

/// `Live`, asked from a thread that may block (`spawn_blocking`), on the
/// runtime it runs on.
pub struct LivePost {
    handle: tokio::runtime::Handle,
    live: Live,
}

impl LivePost {
    /// Connect as the session's account, on `handle`'s runtime.
    pub fn start(handle: tokio::runtime::Handle, app: &AppCredentials, session: Arc<KeptSession>) -> LivePost {
        let live = {
            let _inside = handle.enter();
            Live::start_sending(app, session)
        };
        LivePost { handle, live }
    }

    pub fn session(&self) -> &Arc<KeptSession> {
        self.live.session()
    }

    /// Disconnect, and wait for the connection to end.
    pub fn close(self) {
        let LivePost { handle, live } = self;
        handle.block_on(live.close());
    }
}

impl TelegramPost for LivePost {
    fn me(&self) -> Result<Me, TelegramError> {
        self.handle.block_on(self.live.me())
    }
    fn post(&self, chat: &ChatRef, text: &str) -> Result<(i32, i64), PostError> {
        self.handle.block_on(self.live.post(chat, text))
    }
    fn after(&self, chat: &ChatRef, after: i32) -> Result<Vec<Said>, TelegramError> {
        self.handle.block_on(self.live.after(chat, after))
    }
}

// ------------------------------------------------------------------ live

/// Where a sign-in stands after a step.
pub enum Step {
    /// Signed in as this account.
    Done(Me),
    /// The account has a password: ask for it, with its hint.
    Password { token: PasswordToken, hint: Option<String>, wrong: bool },
}

/// A connection to Telegram as the user, through grammers: started inside a
/// tokio runtime, and closed when done.
pub struct Live {
    client: grammers_client::Client,
    runner: tokio::task::JoinHandle<()>,
    session: Arc<KeptSession>,
}

/// `fut`, given at most `TIMEOUT`: grammers waits as long as Telegram does.
async fn asked<T>(
    fut: impl std::future::Future<Output = Result<T, grammers_client::InvocationError>>,
) -> Result<T, TelegramError> {
    match tokio::time::timeout(TIMEOUT, fut).await {
        Ok(answer) => answer.map_err(read_error),
        Err(_) => Err(TelegramError::Network("Telegram didn't answer in time".into())),
    }
}

fn me_from(user: &grammers_client::peer::User) -> Result<Me, TelegramError> {
    let id = user.id().bare_id().ok_or_else(|| TelegramError::Refused("Telegram didn't say who this is".into()))?;
    Ok(Me { id, name: user.full_name(), username: user.username().map(str::to_string) })
}

/// One message, as a check needs it. In a one-to-one chat Telegram may not
/// say who wrote an incoming message: `import` takes it as the other person.
fn said(m: &grammers_client::message::Message) -> Said {
    use grammers_client::peer::Peer;
    use grammers_client::tl;
    let (from, bot) = match m.sender() {
        Some(Peer::User(u)) => (u.id().bare_id().map(|id| (id, u.full_name())), u.is_bot()),
        _ => (None, false),
    };
    Said {
        id: m.id(),
        at: m.date().as_second(),
        text: m.text().to_string(),
        outgoing: m.outgoing(),
        from,
        bot: bot || m.via_bot_id().is_some(),
        notice: m.action().is_some(),
        media: matches!(&m.raw, tl::enums::Message::Message(x) if x.media.is_some()),
        forwarded: m.forward_header().is_some(),
    }
}

impl Live {
    /// Connect as the session's account (or as no one yet, for a sign-in).
    /// Must be called inside a tokio runtime: the connection runs on it. A
    /// request whose connection dropped is asked once more, a second later.
    pub fn start(app: &AppCredentials, session: Arc<KeptSession>) -> Live {
        Live::connect(app, session, Some(Duration::from_secs(1)))
    }

    /// `start`, for sending (0.10.0-alpha.77): a request whose connection
    /// dropped is never asked again. Telegram may have taken it before the
    /// connection went, and its answer to the same request again
    /// (`RANDOM_ID_DUPLICATE`) would read as a refusal: what the reply's
    /// fate was is then unknown, and said so (`PostError::Unsure`).
    pub fn start_sending(app: &AppCredentials, session: Arc<KeptSession>) -> Live {
        Live::connect(app, session, None)
    }

    fn connect(app: &AppCredentials, session: Arc<KeptSession>, retry_dropped: Option<Duration>) -> Live {
        use grammers_client::sender::{AutoSleep, ConnectionParams};
        let params = ConnectionParams {
            retry_policy: Box::new(AutoSleep {
                tries: std::num::NonZeroU32::MIN,
                threshold: WAITS_OUT,
                io_errors_as_flood_of: retry_dropped,
            }),
            ..Default::default()
        };
        let grammers_client::SenderPool { runner, handle, .. } =
            grammers_client::SenderPool::with_configuration(Arc::clone(&session), app.api_id, params);
        let client = grammers_client::Client::new(handle);
        let runner = tokio::spawn(runner.run());
        Live { client, runner, session }
    }

    pub fn session(&self) -> &Arc<KeptSession> {
        &self.session
    }

    /// Ask Telegram to send a sign-in code to `phone`.
    pub async fn request_code(&self, phone: &str, api_hash: &str) -> Result<LoginToken, TelegramError> {
        asked(self.client.request_login_code(phone.trim(), api_hash)).await
    }

    /// Sign in with the code Telegram sent.
    pub async fn sign_in(&self, token: &LoginToken, code: &str) -> Result<Step, TelegramError> {
        use grammers_client::SignInError as S;
        match tokio::time::timeout(TIMEOUT, self.client.sign_in(token, code.trim())).await {
            Err(_) => Err(TelegramError::Network("Telegram didn't answer in time".into())),
            Ok(Ok(user)) => Ok(Step::Done(me_from(&user)?)),
            Ok(Err(S::PasswordRequired(token))) => {
                let hint = token.hint().map(str::to_string);
                Ok(Step::Password { token, hint, wrong: false })
            }
            Ok(Err(S::InvalidCode)) => Err(TelegramError::BadCode),
            Ok(Err(S::SignUpRequired)) => Err(TelegramError::NoAccount),
            Ok(Err(S::InvalidPassword(token))) => {
                let hint = token.hint().map(str::to_string);
                Ok(Step::Password { token, hint, wrong: true })
            }
            Ok(Err(S::Other(e))) => Err(read_error(e)),
        }
    }

    /// Sign in with the account's password. It goes to Telegram, which
    /// checks it without being sent it (SRP), and is kept nowhere. Asked in
    /// a task of its own, so the client giving up on what Telegram sent
    /// (it panics) is an error, not the end of the app.
    pub async fn check_password(&self, token: PasswordToken, password: &str) -> Result<Step, TelegramError> {
        use grammers_client::SignInError as S;
        let (client, password) = (self.client.clone(), password.as_bytes().to_vec());
        let mut checked = tokio::spawn(async move { client.check_password(token, password).await });
        let answer = match tokio::time::timeout(TIMEOUT, &mut checked).await {
            // Not left to finish on its own (0.10.0-alpha.78).
            Err(_) => {
                checked.abort();
                return Err(TelegramError::Network("Telegram didn't answer in time".into()));
            }
            Ok(Err(_)) => return Err(TelegramError::Refused("its answer about the password couldn't be read".into())),
            Ok(Ok(answer)) => answer,
        };
        match answer {
            Ok(user) => Ok(Step::Done(me_from(&user)?)),
            // Another try is asked with what Telegram gives for it now: what
            // the wrong one was checked against may not be taken twice.
            Err(S::InvalidPassword(token) | S::PasswordRequired(token)) => {
                let token = self.password_again().await.unwrap_or(token);
                let hint = token.hint().map(str::to_string);
                Ok(Step::Password { token, hint, wrong: true })
            }
            Err(S::InvalidCode) => Err(TelegramError::BadCode),
            Err(S::SignUpRequired) => Err(TelegramError::NoAccount),
            Err(S::Other(e)) => Err(read_error(e)),
        }
    }

    /// What a new try at the password is checked against.
    async fn password_again(&self) -> Option<PasswordToken> {
        use grammers_client::tl;
        let got = asked(self.client.invoke(&tl::functions::account::GetPassword {})).await.ok()?;
        let password: tl::types::account::Password = got.into();
        Some(PasswordToken::new(password))
    }

    /// End this sign-in at Telegram, so it isn't left among the account's
    /// sessions: one finished after the user stopped it (0.10.0-alpha.77).
    pub async fn sign_out(&self) {
        let _ = tokio::time::timeout(Duration::from_secs(10), self.client.sign_out()).await;
    }

    /// Whose account this is.
    pub async fn me(&self) -> Result<Me, TelegramError> {
        let user = asked(self.client.get_me()).await?;
        me_from(&user)
    }

    /// Where `chat` is, with the key Telegram wants for it: as the session
    /// met it in a check, or else found among the account's chats.
    async fn peer_ref(&self, chat: &ChatRef) -> Result<grammers_session::types::PeerRef, TelegramError> {
        let id = chat.peer_id().ok_or_else(|| TelegramError::Refused("that isn't a chat I can send into".into()))?;
        if let Ok(Some(known)) = self.session.peer_ref(id).await {
            return Ok(known);
        }
        let mut dialogs = self.client.iter_dialogs().limit(MOST_CHATS);
        while let Some(dialog) = asked(dialogs.next()).await? {
            if dialog.peer_id() == id {
                return Ok(dialog.peer_ref());
            }
        }
        Err(TelegramError::Refused("that chat isn't among your Telegram chats any more".into()))
    }

    /// Send `text` into `chat` as plain text, without a link's preview,
    /// through a connection that never asks twice (`start_sending`).
    pub async fn post(&self, chat: &ChatRef, text: &str) -> Result<(i32, i64), PostError> {
        let peer = self.peer_ref(chat).await.map_err(PostError::NotSent)?;
        match tokio::time::timeout(TIMEOUT, self.client.send_message(peer, text)).await {
            Err(_) => Err(PostError::Unsure("its answer didn't come in time".into())),
            Ok(Ok(sent)) if sent.id() != 0 => Ok((sent.id(), sent.date().as_second())),
            Ok(Ok(_)) => Err(PostError::Unsure("it answered without saying what it made".into())),
            Ok(Err(e)) => Err(post_failed(e)),
        }
    }

    /// What was said in `chat` after message `after`, oldest first.
    pub async fn after(&self, chat: &ChatRef, after: i32) -> Result<Vec<Said>, TelegramError> {
        let peer = self.peer_ref(chat).await?;
        let mut messages = self.client.iter_messages(peer).offset_id(after).reverse(true).limit(50);
        let mut got = Vec::new();
        while let Some(m) = asked(messages.next()).await? {
            got.push(said(&m));
        }
        Ok(got)
    }

    /// Look at the account's chats: the new messages in each one-to-one
    /// chat and private group after its mark, the newest `FIRST_READ` of one
    /// never read. Run as its own task, so a panic in the client is an
    /// error, not the end of the job runner.
    pub async fn fetch(
        &self,
        marks: BTreeMap<String, i32>,
        read_through: Option<i64>,
        stop: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<Fetched, TelegramError> {
        let client = self.client.clone();
        match tokio::spawn(fetch_with(client, marks, read_through, stop)).await {
            Ok(fetched) => fetched,
            Err(e) => Err(TelegramError::Import(format!("reading Telegram went wrong inside Mimic ({e})"))),
        }
    }

    /// Disconnect, and wait for the connection to end.
    pub async fn close(self) {
        self.client.disconnect();
        let _ = tokio::time::timeout(Duration::from_secs(10), self.runner).await;
    }
}

/// `Live::fetch`: `read_through` is the newest message the last check that
/// read everything through saw, by Telegram's clock, if one has.
async fn fetch_with(
    client: grammers_client::Client,
    marks: BTreeMap<String, i32>,
    read_through: Option<i64>,
    stop: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<Fetched, TelegramError> {
    use grammers_client::peer::Peer;
    use grammers_client::tl;
    let me = me_from(&asked(client.get_me()).await?)?;
    let mut fetched = Fetched { me: Some(me), ..Default::default() };
    let mut dialogs = client.iter_dialogs().limit(MOST_DIALOGS);
    // Chats whose messages were asked for: what `MOST_CHATS` counts.
    let mut read = 0;
    'chats: loop {
        let dialog = match asked(dialogs.next()).await {
            Ok(Some(dialog)) => dialog,
            Ok(None) => break,
            // Without the list there is nothing to read; past its first
            // page, what was read is kept and the rest waits for the next
            // check (0.10.0-alpha.77).
            Err(e @ TelegramError::SignIn(_)) => return Err(e),
            Err(e) if read == 0 && fetched.unchanged == 0 && fetched.left == 0 => return Err(e),
            Err(_) => {
                fetched.partial = true;
                break;
            }
        };
        if stop() {
            return Err(TelegramError::Canceled);
        }
        // The list is newest first, by Telegram's clock, but for pinned
        // chats, a chat with a draft (placed by the draft's time) and the
        // archive's entry: below the newest message the last full read
        // saw, every chat was read then (0.10.0-alpha.78).
        let in_order = matches!(&dialog.raw, tl::enums::Dialog::Dialog(d) if !d.pinned && d.draft.is_none());
        let newest_at = dialog.last_message.as_ref().map(|m| m.date().as_second());
        if let Some(at) = newest_at {
            fetched.newest_at = Some(fetched.newest_at.map_or(at, |was| was.max(at)));
        }
        if in_order && matches!((read_through, newest_at), (Some(seen), Some(at)) if at < seen - SAME_MOMENT) {
            fetched.rest_unchanged = true;
            break;
        }
        let (id, title, direct, kind) = match dialog.peer() {
            Peer::User(u) => {
                let itself = u.id().bare_id().is_some_and(|id| TELEGRAM_ITSELF.contains(&id));
                if u.is_bot() || u.is_self() || u.deleted() || itself {
                    fetched.left += 1;
                    continue;
                }
                (u.id().bare_id(), u.full_name(), true, "user")
            }
            Peer::Group(g) => {
                // A public group is everyone's, not a conversation of the user's.
                if matches!(&g.raw, tl::enums::Chat::Channel(c) if c.username.is_some()) {
                    fetched.left += 1;
                    continue;
                }
                let kind = if matches!(&g.raw, tl::enums::Chat::Channel(_)) { "channel" } else { "chat" };
                (g.id().bare_id(), g.title().unwrap_or("Group").to_string(), false, kind)
            }
            _ => {
                fetched.left += 1;
                continue;
            }
        };
        let Some(id) = id else {
            fetched.left += 1;
            continue;
        };
        let newest = dialog.last_message.as_ref().map(|m| m.id()).unwrap_or(0);
        let mark = marks.get(&id.to_string()).copied();
        if mark.is_some_and(|mark| newest <= mark) {
            fetched.unchanged += 1;
            continue;
        }
        // As many chats as one check reads: the next reads on, and this one
        // says it didn't read everything.
        if read >= MOST_CHATS {
            fetched.partial = true;
            break;
        }
        read += 1;
        let peer = dialog.peer_ref();
        let mut messages = match mark {
            Some(mark) => client.iter_messages(peer).offset_id(mark).reverse(true).limit(MOST_NEW + 1),
            None => client.iter_messages(peer).limit(FIRST_READ),
        };
        let mut got: Vec<Said> = Vec::new();
        let mut cut_short = false;
        loop {
            match asked(messages.next()).await {
                Ok(Some(m)) => got.push(said(&m)),
                Ok(None) => break,
                Err(TelegramError::Refused(why)) => {
                    fetched.refused.push(why);
                    continue 'chats;
                }
                Err(e @ TelegramError::SignIn(_)) => return Err(e),
                // Slowing down, or the connection: what this chat gave so far
                // is kept, read up to there, and the rest waits for the next
                // check (0.10.0-alpha.77).
                Err(_) => {
                    fetched.partial = true;
                    cut_short = true;
                    break;
                }
            }
        }
        // Newest first the first time: only a whole read stands for the chat.
        if cut_short && mark.is_none() {
            break 'chats;
        }
        let complete = !cut_short && (mark.is_none() || got.len() <= MOST_NEW);
        got.truncate(MOST_NEW);
        if !got.is_empty() || complete {
            fetched.chats.push(Chat { id, title, direct, kind: kind.to_string(), newest, messages: got, complete });
        }
        if cut_short {
            break 'chats;
        }
    }
    Ok(fetched)
}

// -------------------------------------------------------------- the job

/// Reads a secret from wherever the app keeps them, and keeps one there.
pub type SecretFor = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;
pub type KeepSecret = Arc<dyn Fn(&str, &str) -> Result<(), String> + Send + Sync>;

pub struct CheckTelegramExecutor {
    secret_for: SecretFor,
    keep: KeepSecret,
}

impl CheckTelegramExecutor {
    pub fn shared(secret_for: SecretFor, keep: KeepSecret) -> Arc<dyn crate::jobs::JobExecutor> {
        Arc::new(CheckTelegramExecutor { secret_for, keep })
    }
}

impl crate::jobs::JobExecutor for CheckTelegramExecutor {
    fn kinds(&self) -> &'static [&'static str] {
        &[JOB_KIND]
    }
    fn resumable(&self, _kind: &str) -> bool {
        true
    }
    fn execute(&self, ctx: crate::jobs::JobContext) -> crate::jobs::JobFuture {
        let (secret_for, keep) = (self.secret_for.clone(), self.keep.clone());
        Box::pin(async move {
            let source_id = ctx.job.payload["sourceId"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| crate::jobs::JobError::Failed("no account was named".into()))?;
            let app = secret_for(APP_KEY).and_then(|raw| serde_json::from_str::<AppCredentials>(&raw).ok());
            let read = secret_for(&secret_key(&source_id));
            let session = read.as_deref().and_then(|raw| KeptSession::from_kept(raw).ok());
            let (Some(app), Some(session), Some(read)) = (app, session, read) else {
                let why =
                    "I don't have a Telegram sign-in I can use for this account. Connect it again under Your mail.";
                record_failure(&ctx.db, &source_id, why)?;
                return Err(crate::jobs::JobError::Failed(why.into()));
            };
            let config = tokio::task::block_in_place(|| begin(&ctx.db, &source_id))
                .map_err(|e| crate::jobs::JobError::Failed(e.to_string()))?;
            let began = crate::ids::now_rfc3339();
            let live = Live::start(&app, Arc::new(session));
            let stop_ctx = ctx.clone();
            let fetched = live
                .fetch(config.marks.clone(), config.read_through, Arc::new(move || stop_ctx.check_cancel().is_err()))
                .await;
            // What Telegram changed in the session (a datacenter's key, who
            // was met) is kept; one it says is signed out is not, nor one of
            // an account removed while it was read, nor over another kept
            // meanwhile (a sign-in again, a reply sent; 0.10.0-alpha.77).
            let still_here = ctx.db.get_source(&source_id).ok().flatten().is_some();
            let unchanged = secret_for(&secret_key(&source_id)).as_deref() == Some(read.as_str());
            if still_here && unchanged && !matches!(fetched, Err(TelegramError::SignIn(_))) {
                // Written out the same way each time: kept again only when
                // something in it changed.
                if let Ok(Some(kept)) = live.session().to_kept().map(|kept| (kept != read).then_some(kept)) {
                    if let Err(e) = keep(&secret_key(&source_id), &kept) {
                        tracing::warn!(target: "telegram", error = %e, "couldn't keep the session");
                    }
                }
            }
            live.close().await;
            let db = ctx.db.clone();
            let result = tokio::task::block_in_place(|| finish(&db, &source_id, config, &began, fetched));
            match result {
                Ok(summary) => Ok(serde_json::to_value(summary).unwrap_or(Value::Null)),
                Err(TelegramError::Canceled) => Err(crate::jobs::JobError::Canceled),
                Err(e) => Err(crate::jobs::JobError::Failed(e.to_string())),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(db: &Db) -> String {
        db.set_setting(crate::db::WITHIN_DAYS_SETTING, &0).unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::AccountId, &account_id(7)).unwrap();
        let me = Me { id: 7, name: "C Haynes".into(), username: Some("c".into()) };
        db.create_source(&new_source(&me).unwrap()).unwrap().id
    }

    fn said(id: i32, minutes: i64, outgoing: bool, text: &str) -> Said {
        Said {
            id,
            at: 1_770_110_000 + minutes * 60,
            text: text.into(),
            outgoing,
            from: (!outgoing).then(|| (4242, "Ada Lovelace".to_string())),
            ..Default::default()
        }
    }

    fn me() -> Option<Me> {
        Some(Me { id: 7, name: "C Haynes".into(), username: Some("c".into()) })
    }

    fn ada(messages: Vec<Said>) -> Chat {
        let newest = messages.iter().map(|m| m.id).max().unwrap_or(0);
        Chat {
            id: 4242,
            title: "Ada Lovelace".into(),
            direct: true,
            kind: "user".into(),
            newest,
            messages,
            complete: true,
        }
    }

    #[test]
    fn what_the_app_registration_is_checked_for() {
        assert!(AppCredentials::checked("1234567", "0123456789abcdef0123456789ABCDEF").is_ok());
        assert_eq!(
            AppCredentials::checked(" 1234567 ", "0123456789abcdef0123456789ABCDEF").unwrap().api_hash,
            "0123456789abcdef0123456789abcdef"
        );
        assert!(AppCredentials::checked("my app", "0123456789abcdef0123456789abcdef").is_err());
        assert!(AppCredentials::checked("-5", "0123456789abcdef0123456789abcdef").is_err());
        assert!(AppCredentials::checked("1234567", "not a hash").is_err());
    }

    #[test]
    fn a_chat_is_read_both_sides_under_the_ids_an_export_has_then_only_whats_new() {
        let db = Db::open_in_memory().unwrap();
        let source = account(&db);
        let first = Fetched {
            me: me(),
            chats: vec![ada(vec![
                said(1, 0, false, "lunch on thursday?"),
                said(2, 5, true, "yes! where?"),
                said(3, 6, false, "the place by the river?"),
            ])],
            ..Default::default()
        };
        let summary = check(&db, &source, Ok(first)).unwrap();
        assert_eq!((summary.inserted, summary.from_self, summary.chats), (3, 1, 1));
        let ids: Vec<String> = db
            .conn()
            .prepare("SELECT external_id FROM messages ORDER BY sent_at")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(ids, vec!["telegram:4242:1", "telegram:4242:2", "telegram:4242:3"]);
        let (conversation, metadata): (String, String) = db
            .conn()
            .query_row(
                "SELECT c.external_id, m.metadata_json FROM messages m JOIN conversations c ON c.id = m.conversation_id
                 WHERE m.external_id = 'telegram:4242:3'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(conversation, "telegram:chat:4242");
        let metadata: Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(
            (metadata["telegramDirect"].clone(), metadata["telegramChat"].clone()),
            (json!(true), json!("4242"))
        );
        assert_eq!(metadata["telegramAccount"], json!("telegram:user7"));
        let config: TelegramSourceConfig =
            serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        assert_eq!(config.marks["4242"], 3);
        assert!(config.last_read_at.is_some());
        let people = db.list_participants(10).unwrap();
        assert!(people.iter().any(|p| p.participant.display_name == "Ada Lovelace"));

        // Again: nothing new is unchanged; a new message is all that comes.
        let again = Fetched {
            me: me(),
            chats: vec![ada(vec![said(4, 30, false, "see you then")])],
            unchanged: 2,
            ..Default::default()
        };
        let summary = check(&db, &source, Ok(again)).unwrap();
        assert_eq!((summary.inserted, summary.unchanged), (1, 2));
        assert_eq!(db.count_conversations().unwrap(), 1);
    }

    #[test]
    fn notices_bots_forwards_and_messages_without_words_are_left_out_and_a_photo_is_attached() {
        let db = Db::open_in_memory().unwrap();
        let source = account(&db);
        let chat = ada(vec![
            Said { notice: true, ..said(1, 0, false, "") },
            Said { bot: true, ..said(2, 1, false, "your code is 1234") },
            Said { forwarded: true, ..said(3, 2, false, "look at this") },
            Said { media: true, ..said(4, 3, false, "") },
            Said { media: true, ..said(5, 4, false, "this one?") },
        ]);
        let summary = check(&db, &source, Ok(Fetched { me: me(), chats: vec![chat], ..Default::default() })).unwrap();
        assert_eq!((summary.inserted, summary.left_out), (1, 4));
        let metadata: String = db
            .conn()
            .query_row("SELECT metadata_json FROM messages WHERE external_id = 'telegram:4242:5'", [], |r| r.get(0))
            .unwrap();
        assert!(metadata.contains(r#""attachments":true"#), "{metadata}");
        let config: TelegramSourceConfig =
            serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        assert_eq!(config.marks["4242"], 5, "what was left out is still read past");
    }

    #[test]
    fn a_chat_read_from_an_export_and_live_is_one_conversation() {
        let db = Db::open_in_memory().unwrap();
        let live = account(&db);
        let export = db
            .create_source(&crate::db::NewSource {
                connector: "telegram".into(),
                name: "result.json".into(),
                channel: "chat".into(),
                location: None,
                config: json!({}),
            })
            .unwrap()
            .id;
        let mut importer = Importer::begin(&db, &export).unwrap();
        importer
            .take(DiscoveredConversation {
                external_id: "telegram:chat:4242".into(),
                subject: Some("Ada Lovelace".into()),
                channel: "chat".into(),
                messages: vec![RawMessage {
                    external_id: "telegram:4242:1".into(),
                    author: AuthorRef {
                        display_name: "Ada Lovelace".into(),
                        identifiers: vec![IdentifierInput::new(IdentifierKind::AccountId, "telegram:user4242")],
                    },
                    sent_at: at_time(1_770_110_000),
                    body: "lunch on thursday?".into(),
                    metadata: json!({}),
                }],
            })
            .unwrap();
        importer.finish().unwrap();
        let fetched = Fetched {
            me: me(),
            chats: vec![ada(vec![said(1, 0, false, "lunch on thursday?"), said(2, 5, true, "yes! where?")])],
            ..Default::default()
        };
        let summary = check(&db, &live, Ok(fetched)).unwrap();
        assert_eq!((summary.inserted, summary.duplicates), (1, 1), "{summary:?}");
        assert_eq!(db.count_conversations().unwrap(), 1);
        // Later, only what is new: still the same conversation.
        let later = Fetched { me: me(), chats: vec![ada(vec![said(3, 9, false, "noon?")])], ..Default::default() };
        assert_eq!(check(&db, &live, Ok(later)).unwrap().inserted, 1);
        assert_eq!(db.count_conversations().unwrap(), 1);
    }

    #[test]
    fn another_account_or_none_of_the_chats_fails_the_check_and_one_refused_chat_is_passed_over() {
        let db = Db::open_in_memory().unwrap();
        let source = account(&db);
        let someone = Fetched { me: Some(Me { id: 8, name: "Bob".into(), username: None }), ..Default::default() };
        let err = check(&db, &source, Ok(someone)).unwrap_err();
        assert!(err.to_string().contains("Bob's"), "{err}");
        assert_eq!(db.get_source(&source).unwrap().unwrap().status, "failed");

        let none = Fetched { me: me(), refused: vec!["CHANNEL_PRIVATE".into()], ..Default::default() };
        let err = check(&db, &source, Ok(none)).unwrap_err();
        assert!(err.to_string().contains("none of your chats (CHANNEL_PRIVATE)"), "{err}");

        let one = Fetched {
            me: me(),
            chats: vec![ada(vec![said(1, 0, false, "hi")])],
            refused: vec!["CHANNEL_PRIVATE".into()],
            ..Default::default()
        };
        let summary = check(&db, &source, Ok(one)).unwrap();
        assert_eq!((summary.inserted, summary.unreadable), (1, 1));
        assert_eq!(db.get_source(&source).unwrap().unwrap().status, "imported");

        let quiet = Fetched { me: me(), refused: vec!["CHANNEL_PRIVATE".into()], unchanged: 1, ..Default::default() };
        assert!(check(&db, &source, Ok(quiet)).is_ok(), "beside a quiet chat, it is only that chat");

        let signed_out = check(&db, &source, Err(TelegramError::SignIn("AUTH_KEY_UNREGISTERED".into()))).unwrap_err();
        assert!(signed_out.to_string().contains("sign in again"), "{signed_out}");
        assert_eq!(db.get_source(&source).unwrap().unwrap().status, "failed");
    }

    #[test]
    fn slowing_down_or_too_much_new_is_partial_and_doesnt_count_as_read_through() {
        let db = Db::open_in_memory().unwrap();
        let source = account(&db);
        let mut chat = ada(vec![said(1, 0, false, "hi")]);
        chat.complete = false;
        let summary = check(&db, &source, Ok(Fetched { me: me(), chats: vec![chat], ..Default::default() })).unwrap();
        assert!(summary.partial);
        let config: TelegramSourceConfig =
            serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        assert!(config.last_read_at.is_none());
        assert!(config.last_checked_at.is_some());
    }

    #[test]
    fn a_chat_cut_short_is_read_on_from_where_it_stopped_not_from_its_newest() {
        let db = Db::open_in_memory().unwrap();
        let source = account(&db);
        let mut chat = ada(vec![said(10, 0, false, "first"), said(11, 1, false, "second")]);
        (chat.newest, chat.complete) = (5_000, false);
        check(&db, &source, Ok(Fetched { me: me(), chats: vec![chat], ..Default::default() })).unwrap();
        let config: TelegramSourceConfig =
            serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        assert_eq!(config.marks["4242"], 11, "the next check reads on from 11, not past 5000");

        // Read through: past the newest, even what was left out.
        let mut chat = ada(vec![said(12, 2, false, "third")]);
        chat.newest = 13;
        check(&db, &source, Ok(Fetched { me: me(), chats: vec![chat], ..Default::default() })).unwrap();
        let config: TelegramSourceConfig =
            serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        assert_eq!(config.marks["4242"], 13);
    }

    #[test]
    fn two_accounts_talking_to_the_same_person_are_two_conversations_each_whole() {
        let db = Db::open_in_memory().unwrap();
        let personal = account(&db);
        db.add_user_identifier(IdentifierKind::AccountId, &account_id(8)).unwrap();
        let work_me = Me { id: 8, name: "C at work".into(), username: None };
        let work = db.create_source(&new_source(&work_me).unwrap()).unwrap().id;
        // Telegram numbers each account's messages its own way: both have a 1.
        check(
            &db,
            &personal,
            Ok(Fetched { me: me(), chats: vec![ada(vec![said(1, 0, false, "lunch?")])], ..Default::default() }),
        )
        .unwrap();
        let summary = check(
            &db,
            &work,
            Ok(Fetched {
                me: Some(work_me),
                chats: vec![ada(vec![said(1, 0, false, "the report?"), said(2, 1, true, "on its way")])],
                ..Default::default()
            }),
        )
        .unwrap();
        assert_eq!((summary.inserted, summary.duplicates), (2, 0), "{summary:?}");
        assert_eq!(db.count_conversations().unwrap(), 2);
    }

    #[test]
    fn an_export_of_another_account_is_its_own_conversation() {
        let db = Db::open_in_memory().unwrap();
        let live = account(&db);
        let export = db
            .create_source(&crate::db::NewSource {
                connector: "telegram".into(),
                name: "result.json".into(),
                channel: "chat".into(),
                location: None,
                config: json!({}),
            })
            .unwrap()
            .id;
        let mut importer = Importer::begin(&db, &export).unwrap();
        importer
            .take(DiscoveredConversation {
                external_id: "telegram:chat:4242".into(),
                subject: Some("Ada Lovelace".into()),
                channel: "chat".into(),
                messages: vec![RawMessage {
                    external_id: "telegram:4242:1".into(),
                    author: AuthorRef {
                        display_name: "Ada Lovelace".into(),
                        identifiers: vec![IdentifierInput::new(IdentifierKind::AccountId, "telegram:user4242")],
                    },
                    sent_at: at_time(1_769_000_000),
                    body: "long ago, from the other account".into(),
                    metadata: json!({ "telegramAccount": "telegram:user8" }),
                }],
            })
            .unwrap();
        importer.finish().unwrap();
        let fetched =
            Fetched { me: me(), chats: vec![ada(vec![said(1, 0, false, "lunch on thursday?")])], ..Default::default() };
        let summary = check(&db, &live, Ok(fetched)).unwrap();
        assert_eq!((summary.inserted, summary.duplicates), (1, 0), "{summary:?}");
        assert_eq!(db.count_conversations().unwrap(), 2);
    }

    #[test]
    fn a_reply_whose_fate_telegram_left_unknown_is_never_called_refused() {
        use grammers_client::sender::RpcError;
        use grammers_client::InvocationError as E;
        let rpc = |code: i32, message: &str| {
            E::Rpc(RpcError::from(grammers_client::tl::types::RpcError {
                error_code: code,
                error_message: message.into(),
            }))
        };
        // Taken before the connection went, then asked again.
        assert!(matches!(post_failed(rpc(400, "RANDOM_ID_DUPLICATE")), PostError::Unsure(_)));
        assert!(matches!(post_failed(rpc(-503, "Timeout")), PostError::Unsure(_)));
        assert!(matches!(post_failed(rpc(500, "INTERNAL")), PostError::Unsure(_)));
        assert!(matches!(post_failed(E::Io(std::io::Error::other("connection reset"))), PostError::Unsure(_)));
        // Refusals: nothing went.
        assert!(matches!(post_failed(rpc(403, "CHAT_WRITE_FORBIDDEN")), PostError::NotSent(TelegramError::Refused(_))));
        assert!(matches!(post_failed(rpc(420, "FLOOD_WAIT_30")), PostError::NotSent(TelegramError::RateLimited)));
        assert!(matches!(post_failed(rpc(401, "AUTH_KEY_UNREGISTERED")), PostError::NotSent(TelegramError::SignIn(_))));
        // Reading: Telegram's own failure passes, like the network's, so the
        // check is cut short rather than the chat passed over for good.
        assert!(matches!(read_error(rpc(500, "HISTORY_GET_FAILED")), TelegramError::Network(_)));
        assert!(matches!(read_error(rpc(-503, "Timeout")), TelegramError::Network(_)));
        assert!(matches!(read_error(rpc(400, "CHANNEL_PRIVATE")), TelegramError::Refused(_)));
    }

    #[test]
    fn the_newest_message_a_full_read_saw_is_where_the_next_stops_and_a_partial_read_moves_nothing() {
        let db = Db::open_in_memory().unwrap();
        let source = account(&db);
        let config = |db: &Db| -> TelegramSourceConfig {
            serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap()
        };
        let full = Fetched {
            me: me(),
            chats: vec![ada(vec![said(1, 0, false, "hi")])],
            newest_at: Some(1_770_110_000),
            ..Default::default()
        };
        check(&db, &source, Ok(full)).unwrap();
        assert_eq!(config(&db).read_through, Some(1_770_110_000));

        let mut cut = ada(vec![said(2, 5, false, "and?")]);
        cut.complete = false;
        let partial = Fetched { me: me(), chats: vec![cut], newest_at: Some(1_770_200_000), ..Default::default() };
        check(&db, &source, Ok(partial)).unwrap();
        assert_eq!(config(&db).read_through, Some(1_770_110_000), "a partial read leaves it where it was");

        // A chat Telegram refuses, above where the last full read stopped,
        // is that chat, not the account.
        let refused = Fetched {
            me: me(),
            refused: vec!["CHANNEL_PRIVATE".into()],
            rest_unchanged: true,
            newest_at: Some(1_770_300_000),
            ..Default::default()
        };
        assert!(check(&db, &source, Ok(refused)).is_ok());
        assert_eq!(config(&db).read_through, Some(1_770_300_000));
    }

    #[test]
    fn a_session_nothing_changed_in_is_written_out_the_same_way() {
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let fill = |order: &[i64]| {
            let session = KeptSession::default();
            rt.block_on(async {
                for &id in order {
                    session
                        .cache_peer(PeerInfo::User { id, auth: None, bot: Some(false), is_self: Some(false) })
                        .await
                        .unwrap();
                    session.set_update_state(UpdateState::Channel { id, pts: 1 }).await.unwrap();
                }
            });
            session.to_kept().unwrap()
        };
        let one = fill(&[3, 1, 2, 9, 5]);
        assert_eq!(one, fill(&[9, 5, 2, 1, 3]));
        assert_eq!(KeptSession::from_kept(&one).unwrap().to_kept().unwrap(), one, "read back, it is the same text");
    }

    #[test]
    fn a_session_is_kept_whole_and_read_back() {
        let session = KeptSession::default();
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        rt.block_on(async {
            session.set_home_dc_id(4).await.unwrap();
            let mut option = session.dc_option(4).unwrap().expect("Telegram's datacenters are known");
            option.auth_key = Some([7u8; 256]);
            session.set_dc_option(&option).await.unwrap();
            session
                .cache_peer(PeerInfo::User { id: 7, auth: None, bot: Some(false), is_self: Some(true) })
                .await
                .unwrap();
            session.set_update_state(UpdateState::Secondary { qts: 9 }).await.unwrap();
        });
        let kept = session.to_kept().unwrap();
        let back = KeptSession::from_kept(&kept).unwrap();
        assert_eq!(back.home_dc_id().unwrap(), 4);
        assert_eq!(back.dc_option(4).unwrap().unwrap().auth_key, Some([7u8; 256]));
        assert!(back.dc_option(1).unwrap().is_some(), "the others are still known");
        rt.block_on(async {
            let me = back.peer(PeerId::self_user()).await.unwrap();
            assert!(matches!(me, Some(PeerInfo::User { id: 7, .. })), "{me:?}");
            assert_eq!(back.updates_state().await.unwrap().qts, 9);
        });
    }
}
