//! The `google_chat` connector: Google Chat — direct messages and group
//! chats — read through the Chat API as the user (0.10.0-alpha.79).
//!
//! The Chat API reads someone's chats only with their sign-in, and only for
//! a Google Workspace account (a personal Gmail has no Chat API): the user
//! signs in with Google in their browser, through the registration their
//! mailboxes sign in with (`provider`), granting the Chat API's scopes —
//! spaces, messages (read and, for a reply they approve, later, write) and
//! memberships — and who they are (`openid email profile`). What lasts —
//! the refresh token — is kept in the secret store (`secret_key`), never in
//! the source's settings; each check trades it for an access token
//! (`Access`), saving one Google hands back.
//!
//! A check (`sync`, the `check_google_chat` job) lists the user's direct
//! messages and group chats (`/spaces`), reads each one's messages after
//! the last one read (`marks`, oldest first, so a long absence is read on
//! from where it stopped; the newest `FIRST_READ` the first time), and
//! imports them, the user's and everyone else's. Named spaces (rooms) and
//! direct messages with apps aren't read; apps' messages, deleted ones and
//! those without words are left out. Being asked to slow down ends the
//! check early, keeping what was read.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::imap::{PasswordFor, SaveSecret};
use super::oauth::{self, OAuthError, Provider};
use super::{AuthorRef, DiscoveredConversation, RawMessage};
use crate::db::repo_people::IdentifierInput;
use crate::db::{Db, DbError, IdentifierKind};
use crate::import::{ImportError, Importer};

pub const CONNECTOR: &str = "google_chat";
pub const JOB_KIND: &str = "check_google_chat";
/// The Chat API.
pub const CHAT: &str = "https://chat.googleapis.com/v1";
/// Who signed in: Google's OpenID Connect user info.
pub const USERINFO: &str = "https://openidconnect.googleapis.com/v1/userinfo";

/// What the sign-in asks for.
pub const SCOPE: &str = "openid email profile https://www.googleapis.com/auth/chat.spaces.readonly https://www.googleapis.com/auth/chat.messages https://www.googleapis.com/auth/chat.memberships.readonly";

const FIRST_READ: usize = 300;
/// All that is new, in practice; more marks the check partial, and the next
/// reads on from where it stopped.
const MOST_NEW: usize = 2000;
/// Messages asked for a page at a time (the API's most is 1000).
const PAGE: usize = 500;
const MAX_PAGES: usize = 40;
const TIMEOUT: Duration = Duration::from_secs(30);
/// An access token is used until this long before it lapses.
const SPARE: Duration = Duration::from_secs(300);

/// Where a Google Chat sign-in is kept in the secret store.
pub fn secret_key(source_id: &str) -> String {
    format!("google_chat:{source_id}")
}

/// Someone in Google Chat, by their user's name in the API (`users/123`),
/// which is their Google account's id.
pub fn account_id(user: &str) -> String {
    format!("google_chat:{user}")
}

/// Signing in for Google Chat: the way mailboxes sign in with Google
/// (`google`: its address, and this build's or the user's own secret), with
/// the Chat API's scopes.
pub fn provider(google: &Provider) -> Provider {
    Provider { key: "google_chat", scope: SCOPE, ..google.clone() }
}

#[derive(Debug, thiserror::Error)]
pub enum GoogleChatError {
    #[error("Google wants you to sign in to Google Chat again: use Sign in again beside it under Your mail.")]
    SignIn(String),
    #[error("Google Chat said no ({0}). Only a Google Workspace account's Chat can be read this way, and only once the Google Chat API is turned on in the Google Cloud project Mimic signs in through.")]
    Forbidden(String),
    #[error("Google Chat said no: {0}")]
    Refused(String),
    #[error("I couldn't reach Google Chat: {0}")]
    Network(String),
    #[error("Google Chat asked me to slow down; I'll read the rest on the next check.")]
    RateLimited,
    #[error("{0}")]
    Import(String),
    #[error("canceled")]
    Canceled,
}

impl From<DbError> for GoogleChatError {
    fn from(e: DbError) -> Self {
        GoogleChatError::Import(e.to_string())
    }
}

impl From<ImportError> for GoogleChatError {
    fn from(e: ImportError) -> Self {
        GoogleChatError::Import(e.to_string())
    }
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(300).collect()
}

/// Who signed in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    /// As the Chat API names them: `users/<their Google account's id>`.
    pub id: String,
    pub name: String,
    pub email: Option<String>,
}

/// A direct message or group chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Space {
    /// `spaces/…`.
    pub name: String,
    /// `DIRECT_MESSAGE`, `GROUP_CHAT` or `SPACE`.
    pub kind: String,
    pub display_name: Option<String>,
    /// A direct message with an app, not a person.
    pub with_app: bool,
    /// When its newest message was posted, as the list says: a space with
    /// nothing after its mark isn't read.
    pub last: Option<String>,
}

/// The Chat API with one access token.
pub struct ChatApi {
    base: String,
    userinfo: String,
    token: String,
    client: reqwest::blocking::Client,
}

/// A query's value in a URL.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A resource's name in a path (`spaces/AAA`): each part encoded, the
/// slashes kept.
fn path(name: &str) -> String {
    name.split('/').map(encode).collect::<Vec<_>>().join("/")
}

impl ChatApi {
    pub fn new(token: &str) -> Result<Self, GoogleChatError> {
        Self::at(CHAT, USERINFO, token)
    }

    /// At other addresses: a test's.
    pub fn at(base: &str, userinfo: &str, token: &str) -> Result<Self, GoogleChatError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| GoogleChatError::Network(one_line(&e.to_string())))?;
        Ok(ChatApi {
            base: base.trim_end_matches('/').to_string(),
            userinfo: userinfo.to_string(),
            token: token.to_string(),
            client,
        })
    }

    fn get(&self, url: &str) -> Result<Value, GoogleChatError> {
        let mut waits = 0;
        let response = loop {
            let response = self
                .client
                .get(url)
                .bearer_auth(&self.token)
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .map_err(|e| GoogleChatError::Network(one_line(&e.to_string())))?;
            if response.status().as_u16() != 429 {
                break response;
            }
            // Asked to wait a moment: as long as asked, then again.
            match super::slack::asked_to_wait(&response) {
                Some(wait) if waits < 4 => {
                    waits += 1;
                    std::thread::sleep(wait);
                }
                _ => return Err(GoogleChatError::RateLimited),
            }
        };
        let status = response.status().as_u16();
        let value: Value = response.json().unwrap_or(Value::Null);
        if (200..300).contains(&status) {
            return Ok(value);
        }
        Err(refusal(status, &value))
    }

    /// Who the token is.
    pub fn me(&self) -> Result<Me, GoogleChatError> {
        let v = self.get(&self.userinfo)?;
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        let sub = s("sub").ok_or_else(|| GoogleChatError::Refused("Google didn't say who you are".into()))?;
        let email = s("email").filter(|e| e.contains('@'));
        Ok(Me {
            id: format!("users/{sub}"),
            name: s("name").or_else(|| email.clone()).unwrap_or_else(|| sub.clone()),
            email,
        })
    }

    /// Whether this sign-in can do what a check does, asking for one of
    /// each: who it is, and the first of their chats.
    pub fn check(&self) -> Result<(Me, usize), GoogleChatError> {
        let me = self.me()?;
        let v = self.get(&format!("{}/spaces?pageSize=1", self.base))?;
        let spaces = v.get("spaces").and_then(Value::as_array).map_or(0, Vec::len);
        Ok((me, spaces))
    }

    /// The user's direct messages and group chats, each with when its newest
    /// message was posted.
    pub fn spaces(&self) -> Result<Vec<Space>, GoogleChatError> {
        let filter = encode(r#"spaceType = "DIRECT_MESSAGE" OR spaceType = "GROUP_CHAT""#);
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let mut url = format!("{}/spaces?pageSize=1000&filter={filter}", self.base);
            if let Some(t) = &token {
                url.push_str(&format!("&pageToken={}", encode(t)));
            }
            let v = self.get(&url)?;
            for s in v.get("spaces").and_then(Value::as_array).into_iter().flatten() {
                let Some(name) = s.get("name").and_then(Value::as_str) else { continue };
                let text =
                    |k: &str| s.get(k).and_then(Value::as_str).filter(|t| !t.trim().is_empty()).map(str::to_string);
                out.push(Space {
                    name: name.to_string(),
                    kind: text("spaceType").unwrap_or_default(),
                    display_name: text("displayName"),
                    with_app: s.get("singleUserBotDm").and_then(Value::as_bool).unwrap_or(false),
                    last: text("lastActiveTime"),
                });
            }
            token = v.get("nextPageToken").and_then(Value::as_str).filter(|t| !t.is_empty()).map(str::to_string);
            if token.is_none() {
                break;
            }
        }
        Ok(out)
    }

    /// A space's members: each person's user name and display name.
    pub fn members(&self, space: &str) -> Result<Vec<(String, String)>, GoogleChatError> {
        let v = self.get(&format!("{}/{}/members?pageSize=1000", self.base, path(space)))?;
        Ok(v.get("memberships")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|m| {
                let member = m.get("member")?;
                let name = member.get("name").and_then(Value::as_str)?.to_string();
                let shown = member.get("displayName").and_then(Value::as_str).filter(|n| !n.trim().is_empty())?;
                Some((name, shown.to_string()))
            })
            .collect())
    }

    /// A space's messages posted after `after`, oldest first, at most `most`,
    /// and whether that was all of them; with no `after`, its newest `most`.
    pub fn messages(
        &self,
        space: &str,
        after: Option<&str>,
        most: usize,
    ) -> Result<(Vec<Value>, bool), GoogleChatError> {
        let Some(after) = after else {
            // The first time: the newest, newest first, turned around.
            let url = format!(
                "{}/{}/messages?pageSize={}&orderBy={}",
                self.base,
                path(space),
                most.clamp(1, 1000),
                encode("createTime desc")
            );
            let v = self.get(&url)?;
            let mut out: Vec<Value> = v.get("messages").and_then(Value::as_array).cloned().unwrap_or_default();
            out.truncate(most);
            out.reverse();
            return Ok((out, true));
        };
        let filter = encode(&format!(r#"createTime > "{after}""#));
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        let mut complete = false;
        for _ in 0..MAX_PAGES {
            let mut url = format!("{}/{}/messages?pageSize={PAGE}&filter={filter}", self.base, path(space));
            if let Some(t) = &token {
                url.push_str(&format!("&pageToken={}", encode(t)));
            }
            let v = self.get(&url)?;
            out.extend(v.get("messages").and_then(Value::as_array).cloned().unwrap_or_default());
            token = v.get("nextPageToken").and_then(Value::as_str).filter(|t| !t.is_empty()).map(str::to_string);
            if out.len() > most {
                break;
            }
            if token.is_none() {
                complete = true;
                break;
            }
        }
        let complete = complete && out.len() <= most;
        out.truncate(most);
        Ok((out, complete))
    }
}

/// A Chat API time (RFC 3339, to the nanosecond) as a moment, for order.
fn when(t: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(t).ok()
}

/// Whether time `a` is after `b`.
fn later(a: &str, b: &str) -> bool {
    match (when(a), when(b)) {
        (Some(a), Some(b)) => a > b,
        _ => a > b,
    }
}

/// Why the Chat API said no, from its status and answer.
fn refusal(status: u16, value: &Value) -> GoogleChatError {
    let error = value.get("error");
    let code = error.and_then(|e| e.get("status")).and_then(Value::as_str).unwrap_or("").to_string();
    let message = error.and_then(|e| e.get("message")).and_then(Value::as_str).map(one_line).unwrap_or_default();
    let said = if message.is_empty() { format!("{status} {code}") } else { format!("{code}: {message}") };
    match status {
        401 => GoogleChatError::SignIn(said),
        403 => GoogleChatError::Forbidden(said),
        500.. => GoogleChatError::Network(said),
        _ => GoogleChatError::Refused(said),
    }
}

// ---------------------------------------------------------------- tokens

/// The way mailboxes sign in with Google now — its provider, with this
/// build's or the user's own secret, and the client id — when there is one.
pub type RegistrationFor = Arc<dyn Fn() -> Option<(Provider, String)> + Send + Sync>;

/// Access tokens for each connected Google Chat account: kept in memory
/// until `SPARE` before they lapse, otherwise traded for the refresh token in
/// the secret store, saving one Google hands back.
pub struct Access {
    registration: RegistrationFor,
    read: PasswordFor,
    write: SaveSecret,
    cache: Mutex<HashMap<String, (String, Instant)>>,
}

pub const NOT_SIGNED_IN: &str =
    "I don't have a Google Chat sign-in I can use for this account. Use Sign in again beside it under Your mail.";
pub const NO_CLIENT: &str = "This copy of Mimic isn't set up to sign in with Google yet. Settings, under Signing in with Microsoft and Google, says how.";

impl Access {
    pub fn new(registration: RegistrationFor, read: PasswordFor, write: SaveSecret) -> Self {
        Access { registration, read, write, cache: Mutex::new(HashMap::new()) }
    }

    /// An access token for `source_id`'s account, or why not, in words.
    pub fn token(&self, source_id: &str) -> Result<String, String> {
        if let Some((token, until)) = self.cache.lock().unwrap_or_else(|p| p.into_inner()).get(source_id) {
            if Instant::now() + SPARE < *until {
                return Ok(token.clone());
            }
        }
        let key = secret_key(source_id);
        let refresh = (self.read)(&key).filter(|t| !t.is_empty()).ok_or_else(|| NOT_SIGNED_IN.to_string())?;
        let (google, client) = (self.registration)().ok_or_else(|| NO_CLIENT.to_string())?;
        match oauth::refresh(&provider(&google), &client, &refresh) {
            Ok(tokens) => {
                if let Some(new) = tokens.refresh_token.as_deref().filter(|t| *t != refresh) {
                    if let Err(e) = (self.write)(&key, new) {
                        tracing::warn!(target: "google_chat", error = %e, "a new Google Chat sign-in could not be saved");
                    }
                }
                let until = Instant::now() + Duration::from_secs(tokens.expires_in.max(60));
                self.cache
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(source_id.to_string(), (tokens.access_token.clone(), until));
                Ok(tokens.access_token)
            }
            Err(OAuthError::SignInAgain(_)) => Err(NOT_SIGNED_IN.to_string()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Forget a token Google turned down.
    pub fn forget(&self, source_id: &str) {
        self.cache.lock().unwrap_or_else(|p| p.into_inner()).remove(source_id);
    }
}

// ---------------------------------------------------------------- reading

/// A Google Chat account connected for its chats.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleChatSourceConfig {
    /// `users/…`.
    pub user_id: String,
    pub user: String,
    #[serde(default)]
    pub email: Option<String>,
    /// When the newest message read was posted, per space.
    #[serde(default)]
    pub marks: BTreeMap<String, String>,
    #[serde(default)]
    pub last_checked_at: Option<String>,
    /// When the last check that read everything through began.
    #[serde(default)]
    pub last_read_at: Option<String>,
}

impl GoogleChatSourceConfig {
    pub fn for_me(me: &Me) -> Self {
        GoogleChatSourceConfig {
            user_id: me.id.clone(),
            user: me.name.clone(),
            email: me.email.clone(),
            ..Default::default()
        }
    }

    pub fn is(&self, me: &Me) -> bool {
        self.user_id == me.id
    }
}

pub fn new_source(me: &Me) -> Result<crate::db::NewSource, serde_json::Error> {
    Ok(crate::db::NewSource {
        connector: CONNECTOR.into(),
        name: format!("{} (Google Chat)", me.email.as_deref().unwrap_or(&me.name)),
        channel: "chat".into(),
        location: None,
        config: serde_json::to_value(GoogleChatSourceConfig::for_me(me))?,
    })
}

/// The Google Chat source already connected as this account, if one is.
pub fn connected_as(db: &Db, me: &Me) -> Result<Option<String>, DbError> {
    Ok(db
        .list_sources()?
        .into_iter()
        .filter(|s| s.connector == CONNECTOR)
        .find(|s| serde_json::from_value::<GoogleChatSourceConfig>(s.config.clone()).is_ok_and(|c| c.is(me)))
        .map(|s| s.id))
}

/// What one check did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleChatSummary {
    pub chats: usize,
    pub fetched: usize,
    pub inserted: usize,
    pub duplicates: usize,
    pub from_self: usize,
    /// Apps' messages, deleted ones, and ones without words.
    pub left_out: usize,
    /// Named spaces (rooms) and direct messages with apps, which aren't read.
    pub rooms: usize,
    /// Asked to slow down, or a chat had more new than a check reads.
    pub partial: bool,
    /// Chats Google Chat wouldn't give: passed over, the rest read.
    pub unreadable: usize,
    /// Chats with nothing new since they were last read, by the list's
    /// account of their newest message: not asked for.
    pub unchanged: usize,
}

/// Check an account: read what is new in each chat, import it, and move
/// each chat's mark past what was read.
pub fn sync(
    db: &Db,
    source_id: &str,
    api: &ChatApi,
    should_stop: &dyn Fn() -> bool,
) -> Result<GoogleChatSummary, GoogleChatError> {
    let source = db.get_source(source_id)?.ok_or_else(|| GoogleChatError::Import(format!("no source {source_id}")))?;
    let mut config: GoogleChatSourceConfig = match serde_json::from_value(source.config.clone()) {
        Ok(c) => c,
        Err(e) => {
            let err = GoogleChatError::Import(format!(
                "this account's settings are unreadable ({e}); remove it and connect it again"
            ));
            db.set_source_status(source_id, "failed", Some(&json!({ "message": err.to_string() })))?;
            return Err(err);
        }
    };
    db.set_source_status(source_id, "importing", None)?;
    let began = crate::ids::now_rfc3339();
    let result = sync_inner(db, source_id, &mut config, api, should_stop);
    config.last_checked_at = Some(crate::ids::now_rfc3339());
    if matches!(&result, Ok(s) if !s.partial) {
        config.last_read_at = Some(began);
    }
    db.set_source_config(source_id, &serde_json::to_value(&config).unwrap_or(Value::Null))?;
    match &result {
        Ok(_) => db.set_source_status(source_id, "imported", None)?,
        Err(GoogleChatError::Canceled) => db.set_source_status(source_id, "ready", None)?,
        Err(e) => db.set_source_status(source_id, "failed", Some(&json!({ "message": e.to_string() })))?,
    }
    result
}

/// A check that couldn't start (no sign-in): recorded as an attempt.
pub fn record_failure(db: &Db, source_id: &str, message: &str) -> Result<(), DbError> {
    if let Some(source) = db.get_source(source_id)? {
        if let Ok(mut config) = serde_json::from_value::<GoogleChatSourceConfig>(source.config) {
            config.last_checked_at = Some(crate::ids::now_rfc3339());
            db.set_source_config(source_id, &serde_json::to_value(&config).unwrap_or(Value::Null))?;
        }
        db.set_source_status(source_id, "failed", Some(&json!({ "message": message })))?;
    }
    Ok(())
}

/// Whether a message is one a check keeps: a person's, not deleted, with
/// words.
fn kept(m: &Value) -> bool {
    let sender = m.get("sender");
    sender.and_then(|s| s.get("name")).and_then(Value::as_str).is_some()
        && sender.and_then(|s| s.get("type")).and_then(Value::as_str).is_none_or(|t| t == "HUMAN")
        && m.get("deletionMetadata").is_none_or(Value::is_null)
        && m.get("deleteTime").is_none_or(Value::is_null)
        && m.get("text").and_then(Value::as_str).is_some_and(|t| !t.trim().is_empty())
}

/// A message's time, as stored: UTC to the second.
fn sent_at(created: &str) -> Option<String> {
    when(created).map(|t| t.with_timezone(&chrono::Utc).to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn sync_inner(
    db: &Db,
    source_id: &str,
    config: &mut GoogleChatSourceConfig,
    api: &ChatApi,
    should_stop: &dyn Fn() -> bool,
) -> Result<GoogleChatSummary, GoogleChatError> {
    let me = api.me()?;
    if !config.is(&me) {
        return Err(GoogleChatError::Refused(format!(
            "this sign-in is {}'s, not the account this was connected with",
            me.name
        )));
    }
    let spaces = api.spaces()?;
    let mut summary = GoogleChatSummary::default();
    let mut tried = 0usize;
    let mut last_refusal = GoogleChatError::Refused("no chat could be read".into());
    let mut importer = Importer::begin(db, source_id)?;
    for space in spaces {
        if should_stop() {
            importer.abandon();
            return Err(GoogleChatError::Canceled);
        }
        if (space.kind != "DIRECT_MESSAGE" && space.kind != "GROUP_CHAT") || space.with_app {
            summary.rooms += 1;
            continue;
        }
        let mark = config.marks.get(&space.name).cloned();
        // Nothing after the mark, by the list's account: not asked.
        if let (Some(last), Some(mark)) = (space.last.as_deref(), mark.as_deref()) {
            if !later(last, mark) {
                summary.unchanged += 1;
                continue;
            }
        }
        let most = if mark.is_some() { MOST_NEW } else { FIRST_READ };
        tried += 1;
        let raw = match api.messages(&space.name, mark.as_deref(), most) {
            Ok((raw, complete)) => {
                if !complete {
                    summary.partial = true;
                }
                raw
            }
            Err(GoogleChatError::RateLimited) => {
                summary.partial = true;
                break;
            }
            // The sign-in, or Google itself: every chat would fail the same
            // way, and what was read is kept for the next check.
            Err(e @ GoogleChatError::SignIn(_)) => {
                importer.abandon();
                return Err(e);
            }
            Err(GoogleChatError::Network(_)) => {
                summary.partial = true;
                break;
            }
            // This chat alone: passed over, the rest read.
            Err(e) => {
                tracing::warn!(target: "google_chat", error = %e, "a chat couldn't be read");
                last_refusal = e;
                summary.unreadable += 1;
                continue;
            }
        };
        // Read up to its newest — or, cut short, up to the last one read:
        // oldest first, so the next check reads on from there.
        let Some(newest) = raw
            .iter()
            .filter_map(|m| m.get("createTime").and_then(Value::as_str))
            .filter(|t| when(t).is_some())
            .max_by_key(|t| when(t))
        else {
            continue;
        };
        let newest = newest.to_string();
        summary.fetched += raw.len();
        // Who wrote, by name: as each message says, else as the space's
        // members say.
        let unnamed = raw.iter().filter(|m| kept(m)).any(|m| {
            m.get("sender")
                .and_then(|s| s.get("displayName"))
                .and_then(Value::as_str)
                .is_none_or(|n| n.trim().is_empty())
        });
        let members: HashMap<String, String> = if unnamed {
            match api.members(&space.name) {
                Ok(m) => m.into_iter().collect(),
                Err(GoogleChatError::RateLimited) => {
                    summary.partial = true;
                    break;
                }
                Err(e @ GoogleChatError::SignIn(_)) => {
                    importer.abandon();
                    return Err(e);
                }
                Err(_) => HashMap::new(),
            }
        } else {
            HashMap::new()
        };
        let mut others: Vec<String> = Vec::new();
        let mut messages: Vec<(String, RawMessage)> = Vec::new();
        for m in &raw {
            let sender = m.get("sender");
            let (Some(id), Some(created)) =
                (m.get("name").and_then(Value::as_str), m.get("createTime").and_then(Value::as_str))
            else {
                summary.left_out += 1;
                continue;
            };
            // An app's, one deleted, or one without words.
            let (true, Some(author)) = (kept(m), sender.and_then(|s| s.get("name")).and_then(Value::as_str)) else {
                summary.left_out += 1;
                continue;
            };
            let body = m.get("text").and_then(Value::as_str).unwrap_or("").trim().to_string();
            let name = sender
                .and_then(|s| s.get("displayName"))
                .and_then(Value::as_str)
                .filter(|n| !n.trim().is_empty())
                .map(str::to_string)
                .or_else(|| members.get(author).cloned())
                .unwrap_or_else(|| author.to_string());
            if author != config.user_id && !others.contains(&name) {
                others.push(name.clone());
            }
            let mut identifiers = vec![IdentifierInput::new(IdentifierKind::AccountId, account_id(author))];
            if let Some(email) = sender.and_then(|s| s.get("email")).and_then(Value::as_str).filter(|e| e.contains('@'))
            {
                identifiers.push(IdentifierInput::new(IdentifierKind::Email, email));
            }
            let attached = ["attachment", "attachedGifs"]
                .iter()
                .any(|k| m.get(*k).and_then(Value::as_array).is_some_and(|a| !a.is_empty()));
            messages.push((
                created.to_string(),
                RawMessage {
                    external_id: account_id(id),
                    author: AuthorRef { display_name: name, identifiers },
                    sent_at: sent_at(created),
                    body,
                    metadata: json!({
                        "googleChatSpace": space.name,
                        // To the nanosecond, for asking what came after it.
                        "googleChatCreated": created,
                        "googleChatDirect": space.kind == "DIRECT_MESSAGE",
                        "googleChatThread": m.get("thread").and_then(|t| t.get("name")).cloned().unwrap_or(Value::Null),
                        "attachments": attached,
                    }),
                },
            ));
        }
        messages.sort_by_key(|(t, _)| when(t));
        if !messages.is_empty() {
            let subject = space.display_name.clone().unwrap_or_else(|| {
                if others.is_empty() {
                    "Chat".to_string()
                } else {
                    others.join(", ")
                }
            });
            if let Err(e) = importer.take(DiscoveredConversation {
                external_id: account_id(&space.name),
                subject: Some(subject),
                channel: "chat".into(),
                messages: messages.into_iter().map(|(_, m)| m).collect(),
            }) {
                importer.abandon();
                return Err(e.into());
            }
            summary.chats += 1;
        }
        config.marks.insert(space.name.clone(), newest);
    }
    // Not one would Google Chat give, and none was passed over for having
    // nothing new: that is the account — the API turned off, say — not a
    // chat.
    if tried > 0 && summary.unreadable == tried && summary.unchanged == 0 {
        importer.abandon();
        return Err(last_refusal);
    }
    let imported = importer.finish()?;
    summary.inserted = imported.inserted;
    summary.duplicates = imported.duplicates;
    summary.from_self = imported.from_self;
    Ok(summary)
}

// -------------------------------------------------------------- schedule

/// Accounts due a check at `now`, on the mailboxes' interval.
pub fn due(db: &Db, now: chrono::DateTime<chrono::Utc>) -> Result<Vec<String>, DbError> {
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
        let Ok(config) = serde_json::from_value::<GoogleChatSourceConfig>(s.config.clone()) else { continue };
        let wait = if s.status == "failed" { (minutes * 4).min(360) } else { minutes };
        let last = config
            .last_checked_at
            .as_deref()
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok().map(|d| d.with_timezone(&chrono::Utc)));
        if last.is_none_or(|at| now.signed_duration_since(at) >= chrono::Duration::minutes(wait)) {
            out.push(s.id);
        }
    }
    Ok(out)
}

pub fn due_now(db: &Db) -> Result<Vec<String>, DbError> {
    due(db, chrono::Utc::now())
}

/// At start-up: an account left "importing" is ready again, unless a check
/// for it is queued.
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

pub struct CheckGoogleChatExecutor {
    access: Arc<Access>,
}

impl CheckGoogleChatExecutor {
    pub fn shared(access: Arc<Access>) -> Arc<dyn crate::jobs::JobExecutor> {
        Arc::new(CheckGoogleChatExecutor { access })
    }
}

impl crate::jobs::JobExecutor for CheckGoogleChatExecutor {
    fn kinds(&self) -> &'static [&'static str] {
        &[JOB_KIND]
    }
    fn resumable(&self, _kind: &str) -> bool {
        true
    }
    fn execute(&self, ctx: crate::jobs::JobContext) -> crate::jobs::JobFuture {
        let access = self.access.clone();
        Box::pin(async move {
            let source_id = ctx.job.payload["sourceId"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| crate::jobs::JobError::Failed("no Google Chat account was named".into()))?;
            let token = match tokio::task::block_in_place(|| access.token(&source_id)) {
                Ok(t) => t,
                Err(why) => {
                    record_failure(&ctx.db, &source_id, &why)?;
                    return Err(crate::jobs::JobError::Failed(why));
                }
            };
            let db = ctx.db.clone();
            let stop_ctx = ctx.clone();
            let result = tokio::task::block_in_place(|| {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let api = ChatApi::new(&token)?;
                    sync(&db, &source_id, &api, &|| stop_ctx.check_cancel().is_err())
                }))
            });
            let Ok(result) = result else {
                let why = "Something went wrong inside Mimic while reading Google Chat.";
                record_failure(&db, &source_id, why)?;
                return Err(crate::jobs::JobError::Failed(why.into()));
            };
            match result {
                Ok(summary) => Ok(serde_json::to_value(summary).unwrap_or(Value::Null)),
                Err(GoogleChatError::Canceled) => Err(crate::jobs::JobError::Canceled),
                Err(e) => {
                    if matches!(e, GoogleChatError::SignIn(_)) {
                        access.forget(&source_id);
                    }
                    Err(crate::jobs::JobError::Failed(e.to_string()))
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use super::*;
    use crate::sources::teams::fake::{graph as server, Answer, TOKEN};

    fn account(db: &Db) -> String {
        db.set_setting(crate::db::WITHIN_DAYS_SETTING, &0).unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::AccountId, &account_id("users/7")).unwrap();
        db.create_source(&new_source(&me()).unwrap()).unwrap().id
    }

    fn me() -> Me {
        Me { id: "users/7".into(), name: "C Haynes".into(), email: Some("c@formicaria.us".into()) }
    }

    fn chat_api(answer: Answer) -> (ChatApi, crate::sources::teams::fake::Seen) {
        let (base, seen) = server(answer);
        (ChatApi::at(&base, &format!("{base}/userinfo"), TOKEN).unwrap(), seen)
    }

    fn said(space: &str, n: u32, user: &str, name: &str, minute: u32, text: &str) -> Value {
        json!({
            "name": format!("{space}/messages/m{n}"),
            "sender": { "name": user, "displayName": name, "type": "HUMAN" },
            "createTime": format!("2026-10-26T09:{minute:02}:00.123456Z"),
            "text": text,
            "thread": { "name": format!("{space}/threads/t1") },
        })
    }

    const USERINFO_BODY: &str = r#"{"sub":"7","name":"C Haynes","email":"c@formicaria.us","email_verified":true}"#;

    #[test]
    fn a_check_reads_direct_messages_and_group_chats_both_sides_then_only_whats_new() {
        let db = Db::open_in_memory().unwrap();
        let source = account(&db);
        let second = Arc::new(StdMutex::new(false));
        let again = second.clone();
        let (api, seen) = chat_api(Box::new(move |path, query| {
            let later = *again.lock().unwrap();
            match path {
                "/userinfo" => (200, USERINFO_BODY.into()),
                "/spaces" => {
                    let dm_last = if later { "2026-10-26T09:30:00Z" } else { "2026-10-26T09:06:00Z" };
                    (
                        200,
                        json!({ "spaces": [
                            { "name": "spaces/DM1", "spaceType": "DIRECT_MESSAGE", "lastActiveTime": dm_last },
                            { "name": "spaces/G1", "spaceType": "GROUP_CHAT", "displayName": "", "lastActiveTime": "2026-10-26T09:10:00Z" },
                            { "name": "spaces/ROOM", "spaceType": "SPACE", "displayName": "Everyone", "lastActiveTime": "2026-10-26T09:20:00Z" },
                            { "name": "spaces/APP", "spaceType": "DIRECT_MESSAGE", "singleUserBotDm": true, "lastActiveTime": "2026-10-26T09:20:00Z" }
                        ]})
                        .to_string(),
                    )
                }
                "/spaces/DM1/messages" if !later => {
                    assert!(query.contains("orderBy=createTime%20desc"), "{query}");
                    (
                        200,
                        json!({ "messages": [
                            { "name": "spaces/DM1/messages/gone", "sender": { "name": "users/42", "type": "HUMAN" },
                              "createTime": "2026-10-26T09:06:00Z", "deletionMetadata": { "deletionType": "CREATOR" } },
                            said("spaces/DM1", 2, "users/7", "C Haynes", 5, "yes! where?"),
                            { "name": "spaces/DM1/messages/bot", "sender": { "name": "users/app", "type": "BOT" },
                              "createTime": "2026-10-26T09:02:00Z", "text": "Reminder" },
                            said("spaces/DM1", 1, "users/42", "Ada Lovelace", 1, "lunch on thursday?"),
                        ]})
                        .to_string(),
                    )
                }
                "/spaces/DM1/messages" => {
                    assert!(query.contains("filter=createTime"), "read after the mark: {query}");
                    (200, json!({ "messages": [said("spaces/DM1", 3, "users/42", "Ada Lovelace", 30, "the river place?")] }).to_string())
                }
                "/spaces/G1/messages" => (
                    200,
                    json!({ "messages": [
                        { "name": "spaces/G1/messages/g1", "sender": { "name": "users/43", "type": "HUMAN" },
                          "createTime": "2026-10-26T09:10:00Z", "text": "sunday?", "attachment": [{ "name": "x" }] }
                    ]})
                    .to_string(),
                ),
                "/spaces/G1/members" => (
                    200,
                    json!({ "memberships": [
                        { "member": { "name": "users/43", "displayName": "Grace Hopper", "type": "HUMAN" } },
                        { "member": { "name": "users/7", "displayName": "C Haynes", "type": "HUMAN" } }
                    ]})
                    .to_string(),
                ),
                other => panic!("not asked for: {other}"),
            }
        }));
        let summary = sync(&db, &source, &api, &|| false).unwrap();
        assert_eq!((summary.chats, summary.inserted, summary.from_self), (2, 3, 1), "{summary:?}");
        assert_eq!((summary.rooms, summary.left_out), (2, 2), "{summary:?}");
        let (subject, metadata): (String, String) = db
            .conn()
            .query_row(
                "SELECT c.subject, m.metadata_json FROM messages m JOIN conversations c ON c.id = m.conversation_id
                 WHERE m.external_id = 'google_chat:spaces/G1/messages/g1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(subject, "Grace Hopper", "named by who is in it");
        let metadata: Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(
            (metadata["googleChatDirect"].clone(), metadata["attachments"].clone()),
            (json!(false), json!(true))
        );
        let people = db.list_participants(10).unwrap();
        assert!(people.iter().any(|p| p.participant.display_name == "Ada Lovelace"));
        let config: GoogleChatSourceConfig =
            serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        assert_eq!(config.marks["spaces/DM1"], "2026-10-26T09:06:00Z", "read past what was left out too");
        assert!(config.last_read_at.is_some());

        // Again: the group has nothing new by the list, and isn't asked; the
        // direct message is read after its mark.
        *second.lock().unwrap() = true;
        let summary = sync(&db, &source, &api, &|| false).unwrap();
        assert_eq!((summary.inserted, summary.unchanged), (1, 1), "{summary:?}");
        assert_eq!(db.count_conversations().unwrap(), 2);
        assert!(seen.lock().unwrap().iter().all(|r| !r.starts_with("/spaces/ROOM") && !r.starts_with("/spaces/APP")));
    }

    #[test]
    fn a_sign_in_turned_down_or_a_personal_account_says_what_to_do() {
        let db = Db::open_in_memory().unwrap();
        let source = account(&db);
        let (api, _) = chat_api(Box::new(|path, _| {
            match path {
            "/userinfo" => (200, USERINFO_BODY.into()),
            _ => (403, r#"{"error":{"code":403,"status":"PERMISSION_DENIED","message":"Google Chat API has not been used in project 1 before or it is disabled."}}"#.into()),
        }
        }));
        let err = sync(&db, &source, &api, &|| false).unwrap_err();
        assert!(matches!(err, GoogleChatError::Forbidden(_)), "{err:?}");
        assert!(err.to_string().contains("Google Workspace"), "{err}");
        assert_eq!(db.get_source(&source).unwrap().unwrap().status, "failed");

        let (api, _) = chat_api(Box::new(|_, _| (401, r#"{"error":{"code":401,"status":"UNAUTHENTICATED"}}"#.into())));
        let err = sync(&db, &source, &api, &|| false).unwrap_err();
        assert!(err.to_string().contains("sign in to Google Chat again"), "{err}");

        // Someone else's sign-in reads nothing.
        let (api, _) = chat_api(Box::new(|path, _| match path {
            "/userinfo" => (200, r#"{"sub":"8","name":"Bob"}"#.into()),
            other => panic!("not asked for: {other}"),
        }));
        let err = sync(&db, &source, &api, &|| false).unwrap_err();
        assert!(err.to_string().contains("Bob's"), "{err}");
    }

    #[test]
    fn more_new_than_a_check_reads_is_read_on_oldest_first_from_where_it_stopped() {
        let (api, seen) = chat_api(Box::new(|path, query| match path {
            "/spaces/DM1/messages" if !query.contains("pageToken") => (
                200,
                json!({ "messages": [
                    said("spaces/DM1", 4, "users/42", "Ada", 4, "a"),
                    said("spaces/DM1", 5, "users/42", "Ada", 5, "b")
                ], "nextPageToken": "next" })
                .to_string(),
            ),
            "/spaces/DM1/messages" => {
                (200, json!({ "messages": [said("spaces/DM1", 6, "users/42", "Ada", 6, "c")] }).to_string())
            }
            other => panic!("not asked for: {other}"),
        }));
        let (got, complete) = api.messages("spaces/DM1", Some("2026-10-26T09:03:00Z"), 2).unwrap();
        assert!(!complete);
        let names: Vec<&str> = got.iter().filter_map(|m| m["name"].as_str()).collect();
        assert_eq!(names, vec!["spaces/DM1/messages/m4", "spaces/DM1/messages/m5"], "the oldest first");
        let (got, complete) = api.messages("spaces/DM1", Some("2026-10-26T09:03:00Z"), 5).unwrap();
        assert!(complete);
        assert_eq!(got.len(), 3);
        assert!(
            seen.lock().unwrap()[0].contains("createTime%20%3E%20%222026-10-26T09%3A03%3A00Z%22"),
            "{:?}",
            seen.lock().unwrap()
        );
    }

    #[test]
    fn signing_in_for_chat_asks_googles_way_with_the_chat_apis_scopes() {
        let google = Provider { client_secret: Some("GOCSPX-own".into()), ..Provider::google() };
        let chat = provider(&google);
        assert_eq!((chat.key, chat.token_url.as_str()), ("google_chat", "https://oauth2.googleapis.com/token"));
        assert_eq!(chat.client_secret.as_deref(), Some("GOCSPX-own"), "the user's own registration, as mailboxes use");
        assert!(chat.scope.contains("chat.messages") && chat.scope.contains("openid"));
        assert_eq!(path("spaces/AA A"), "spaces/AA%20A");
    }
}
