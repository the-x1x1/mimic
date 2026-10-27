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

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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

/// What the sign-in asks for: who it is, the spaces, reading messages,
/// writing one (never editing or deleting, 0.10.0-alpha.81), and who is in
/// a space.
pub const SCOPE: &str = "openid email profile https://www.googleapis.com/auth/chat.spaces.readonly https://www.googleapis.com/auth/chat.messages.readonly https://www.googleapis.com/auth/chat.messages.create https://www.googleapis.com/auth/chat.memberships.readonly";

/// The Chat API's scopes a sign-in must have been granted: Google lets the
/// user untick some (0.10.0-alpha.81).
pub const NEEDED: [&str; 4] = [
    "https://www.googleapis.com/auth/chat.spaces.readonly",
    "https://www.googleapis.com/auth/chat.messages.readonly",
    "https://www.googleapis.com/auth/chat.messages.create",
    "https://www.googleapis.com/auth/chat.memberships.readonly",
];

/// What a sign-in was granted that it needs and lacks, by the token
/// response's `scope`; nothing missing when Google didn't say.
pub fn missing(granted: Option<&str>) -> Vec<&'static str> {
    let Some(granted) = granted else { return Vec::new() };
    let have: Vec<&str> = granted.split_whitespace().collect();
    NEEDED.iter().copied().filter(|s| !have.contains(s)).collect()
}

pub const TICK_EVERY_BOX: &str =
    "Google Chat needs every permission it asked for: sign in again, and leave every box on Google's page ticked.";

const FIRST_READ: usize = 300;
/// All that is new, in practice; more marks the check partial, and the next
/// reads on from where it stopped.
const MOST_NEW: usize = 2000;
/// Messages asked for a page at a time (the API's most is 1000).
const PAGE: usize = 500;
/// The most of a thread's earlier messages read when the user is first
/// mentioned in it, or first writes there (0.10.0-alpha.95).
const THREAD_BEFORE: usize = 100;
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
    /// The Chat API answers, but the Google Cloud project's Chat app isn't
    /// configured (0.10.0-alpha.81).
    #[error("Google Chat said the app isn't set up ({0}). In the Google Cloud project Mimic signs in through, open the Google Chat API's Configuration page and give it a name, an avatar address and a description, with interactive features off.")]
    NotConfigured(String),
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
            // Asked to wait a moment: as long as asked — Google seldom says,
            // so a second, then two, four and eight (0.10.0-alpha.81) — then
            // again.
            let wait = super::slack::asked_to_wait(&response).unwrap_or(Duration::from_secs(1 << waits));
            if waits >= 4 {
                return Err(GoogleChatError::RateLimited);
            }
            waits += 1;
            std::thread::sleep(wait);
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

    /// The user's direct messages, group chats and named spaces, each with
    /// when its newest message was posted.
    pub fn spaces(&self) -> Result<Vec<Space>, GoogleChatError> {
        let filter = encode(r#"spaceType = "DIRECT_MESSAGE" OR spaceType = "GROUP_CHAT" OR spaceType = "SPACE""#);
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

    /// A thread's messages posted before `before`, its newest `most` of them,
    /// oldest first (0.10.0-alpha.95): what was said in a thread before the
    /// user was mentioned in it.
    pub fn earlier_in_thread(
        &self,
        space: &str,
        thread: &str,
        before: &str,
        most: usize,
    ) -> Result<Vec<Value>, GoogleChatError> {
        let filter = encode(&format!(r#"createTime < "{before}" AND thread.name = {thread}"#));
        let url = format!(
            "{}/{}/messages?pageSize={}&orderBy={}&filter={filter}",
            self.base,
            path(space),
            most.clamp(1, 1000),
            encode("createTime DESC")
        );
        let v = self.get(&url)?;
        let mut out: Vec<Value> = v.get("messages").and_then(Value::as_array).cloned().unwrap_or_default();
        out.truncate(most);
        out.reverse();
        Ok(out)
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
                encode("createTime DESC")
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

impl ChatApi {
    /// Post `text` into a space as the user (0.10.0-alpha.80), as plain
    /// text, under `request`: the same request asked again makes no second
    /// message. The new message's name and when Google Chat says it was
    /// made, or why not — and whether it may have gone anyway.
    ///
    /// With `thread` (0.10.0-alpha.91: a reply to a mention in a named
    /// space), it goes into that thread, or starts one if the thread is gone.
    pub fn post(
        &self,
        space: &str,
        text: &str,
        request: &str,
        thread: Option<&str>,
    ) -> Result<(String, Option<String>), PostError> {
        let url = self.post_target(space, request, thread.is_some());
        let mut body = json!({ "text": defused(text) });
        if let Some(thread) = thread {
            body["thread"] = json!({ "name": thread });
        }
        let response = self.client.post(url).bearer_auth(&self.token).json(&body).send();
        let response = match response {
            Ok(r) => r,
            Err(e) if e.is_connect() || e.is_builder() => {
                return Err(PostError::NotSent(GoogleChatError::Network(one_line(&e.to_string()))))
            }
            Err(e) => return Err(PostError::Unsure(one_line(&e.to_string()))),
        };
        let status = response.status().as_u16();
        if status == 429 {
            return Err(PostError::NotSent(GoogleChatError::RateLimited));
        }
        let value: Value = match response.json() {
            Ok(v) => v,
            Err(e) if (200..300).contains(&status) => {
                return Err(PostError::Unsure(format!("Google Chat answered {status}: {}", one_line(&e.to_string()))))
            }
            Err(_) => Value::Null,
        };
        if (200..300).contains(&status) {
            let created = value.get("createTime").and_then(Value::as_str).map(str::to_string);
            return value.get("name").and_then(Value::as_str).map(|name| (name.to_string(), created)).ok_or_else(
                || PostError::Unsure("Google Chat said it went, without saying which message it was".into()),
            );
        }
        // A server's failure may have come after it took the message.
        if status >= 500 {
            return Err(PostError::Unsure(format!("Google Chat answered {status}")));
        }
        Err(PostError::NotSent(refusal(status, &value)))
    }
}

impl ChatApi {
    /// Where a post goes: the space's messages, under `request`, and — into
    /// a thread — replying in it, or in a new one if it is gone.
    fn post_target(&self, space: &str, request: &str, threaded: bool) -> String {
        let mut url = format!("{}/{}/messages?requestId={}", self.base, path(space), encode(request));
        if threaded {
            url.push_str("&messageReplyOption=REPLY_MESSAGE_FALLBACK_TO_NEW_THREAD");
        }
        url
    }
}

/// `text` as the user wrote it, with nothing in it read as Google Chat's
/// own markup for a mention (`<users/…>`, `<users/all>`) or a link
/// (`<url|text>`): a zero-width space after each `<` that would begin one
/// (0.10.0-alpha.81).
pub fn defused(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..=i]);
        rest = &rest[i + 1..];
        let inside = rest.split('>').next().unwrap_or("");
        if rest.starts_with("users/")
            || inside.contains('|')
            || inside.starts_with("http")
            || inside.starts_with("mailto:")
        {
            out.push('\u{200B}');
        }
    }
    out.push_str(rest);
    out
}

/// Why a post didn't go — or might have.
#[derive(Debug, thiserror::Error)]
pub enum PostError {
    #[error(transparent)]
    NotSent(GoogleChatError),
    /// Asked, and no clear answer came back: it may have gone.
    #[error("I lost Google Chat's answer as it was being sent ({0}), so it may or may not have gone.")]
    Unsure(String),
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
    let insufficient = message.to_ascii_lowercase().contains("insufficient authentication scopes")
        || value.to_string().contains("ACCESS_TOKEN_SCOPE_INSUFFICIENT");
    match status {
        401 => GoogleChatError::SignIn(said),
        // A permission the user unticked when signing in: signing in again,
        // ticking it, mends it.
        403 if insufficient => GoogleChatError::SignIn(format!("{said}. {TICK_EVERY_BOX}")),
        403 => GoogleChatError::Forbidden(said),
        404 if message.to_ascii_lowercase().contains("chat app not found") => GoogleChatError::NotConfigured(said),
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
    /// Whether named spaces are read — the user's own messages there, those
    /// that mention them, and the threads they are in — as well as direct
    /// messages and group chats. Off unless the user turned it on, connecting
    /// or since (0.10.0-alpha.92), so an account connected before spaces were
    /// read isn't read further than it was given to be.
    #[serde(default)]
    pub spaces: bool,
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

/// A new account's source; `spaces`, whether its named spaces are read.
pub fn new_source(me: &Me, spaces: bool) -> Result<crate::db::NewSource, serde_json::Error> {
    Ok(crate::db::NewSource {
        connector: CONNECTOR.into(),
        name: format!("{} (Google Chat)", me.email.as_deref().unwrap_or(&me.name)),
        channel: "chat".into(),
        location: None,
        config: serde_json::to_value(GoogleChatSourceConfig { spaces, ..GoogleChatSourceConfig::for_me(me) })?,
    })
}

/// Read an account's named spaces, or stop (0.10.0-alpha.92). What was read
/// from them stays; a check running meanwhile keeps the choice.
pub fn set_spaces(db: &Db, source_id: &str, on: bool) -> Result<(), DbError> {
    db.set_source_config_key(source_id, "spaces", &Value::Bool(on))
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
    /// Direct messages with apps, which aren't read.
    pub rooms: usize,
    /// Named spaces, while reading them is off: not read (0.10.0-alpha.92).
    #[serde(default)]
    pub spaces_off: usize,
    /// In named spaces: messages in threads the user isn't in — none of
    /// theirs, none mentioning them, none read before — left out
    /// (0.10.0-alpha.91, .92).
    #[serde(default)]
    pub not_for_you: usize,
    /// In named spaces: messages mentioning the user, read.
    #[serde(default)]
    pub mentions: usize,
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
    // Spaces turned on or off while this ran: the user's choice is kept, in
    // the same statement (0.10.0-alpha.95).
    db.set_source_config_keeping_flag(source_id, &serde_json::to_value(&config).unwrap_or(Value::Null), "spaces")?;
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

/// Whether a message is a person's, and not deleted: with words or without
/// (a photo, a file).
pub(crate) fn by_a_person(m: &Value) -> bool {
    let sender = m.get("sender");
    sender.and_then(|s| s.get("name")).and_then(Value::as_str).is_some()
        && sender.and_then(|s| s.get("type")).and_then(Value::as_str).is_none_or(|t| t == "HUMAN")
        && m.get("deletionMetadata").is_none_or(Value::is_null)
        && m.get("deleteTime").is_none_or(Value::is_null)
}

/// Whether a message is one a check keeps: a person's, not deleted, with
/// words.
pub(crate) fn kept(m: &Value) -> bool {
    by_a_person(m) && m.get("text").and_then(Value::as_str).is_some_and(|t| !t.trim().is_empty())
}

/// Whether a message mentions `user` by name (`users/…`): an `@` of them,
/// not of everyone (0.10.0-alpha.91).
pub(crate) fn mentions(m: &Value, user: &str) -> bool {
    m.get("annotations").and_then(Value::as_array).into_iter().flatten().any(|a| {
        a.get("type").and_then(Value::as_str) == Some("USER_MENTION")
            && a.get("userMention").and_then(|u| u.get("user")).and_then(|u| u.get("name")).and_then(Value::as_str)
                == Some(user)
    })
}

/// The thread a message is in, by Google Chat's name for it.
fn thread_name(m: &Value) -> Option<&str> {
    m.get("thread").and_then(|t| t.get("name")).and_then(Value::as_str)
}

/// The threads among a named space's messages that the user is in, as
/// Google Chat itself follows a thread for them: one with a message of
/// theirs or one mentioning them among `raw`, or one the source already
/// holds — read before, so they were in it then (0.10.0-alpha.92). And of
/// those, the ones followed only now (0.10.0-alpha.95), whose earlier
/// messages haven't been read.
fn followed_threads(
    db: &Db,
    source_id: &str,
    raw: &[Value],
    user: &str,
) -> Result<(HashSet<String>, BTreeSet<String>), DbError> {
    let theirs: HashSet<&str> = raw
        .iter()
        .filter(|m| {
            by_a_person(m)
                && (m.get("sender").and_then(|s| s.get("name")).and_then(Value::as_str) == Some(user)
                    || mentions(m, user))
        })
        .filter_map(thread_name)
        .collect();
    let all: BTreeSet<&str> = raw.iter().filter_map(thread_name).collect();
    let ids: Vec<String> = all.iter().map(|t| account_id(t)).collect();
    let known = db.conversations_in_source(source_id, &ids)?;
    let held = |t: &str| known.contains(&account_id(t));
    let followed = all.iter().filter(|t| theirs.contains(**t) || held(t)).map(|t| t.to_string()).collect();
    let newly = theirs.into_iter().filter(|t| !held(t)).map(str::to_string).collect();
    Ok((followed, newly))
}

/// Whether the user reads named spaces now: they may have turned it off
/// while a check ran (0.10.0-alpha.95).
fn reading_spaces(db: &Db, source_id: &str) -> bool {
    db.get_source(source_id)
        .ok()
        .flatten()
        .and_then(|s| s.config.get("spaces").and_then(Value::as_bool))
        .unwrap_or(false)
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
        let named = space.kind == "SPACE";
        if (space.kind != "DIRECT_MESSAGE" && space.kind != "GROUP_CHAT" && !named) || space.with_app {
            summary.rooms += 1;
            continue;
        }
        if named && !reading_spaces(db, source_id) {
            summary.spaces_off += 1;
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
        let mut raw = match api.messages(&space.name, mark.as_deref(), most) {
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
        // oldest first, so the next check reads on from there. A whole read
        // that found nothing (history off, every message deleted) is read
        // up to what the list said, so it isn't asked again until something
        // new comes (0.10.0-alpha.81).
        let Some(newest) = raw
            .iter()
            .filter_map(|m| m.get("createTime").and_then(Value::as_str))
            .filter(|t| when(t).is_some())
            .max_by_key(|t| when(t))
        else {
            if let Some(last) = space.last.as_deref().filter(|l| when(l).is_some()) {
                if mark.as_deref().is_none_or(|m| later(last, m)) {
                    config.marks.insert(space.name.clone(), last.to_string());
                }
            }
            continue;
        };
        let newest = newest.to_string();
        summary.fetched += raw.len();
        // In a named space, only the threads the user is in are read: the
        // rest of a busy space is not addressed to them (0.10.0-alpha.91,
        // .92).
        let (followed, newly) = if named {
            match followed_threads(db, source_id, &raw, &config.user_id) {
                Ok(f) => f,
                Err(e) => {
                    importer.abandon();
                    return Err(e.into());
                }
            }
        } else {
            (HashSet::new(), BTreeSet::new())
        };
        // A thread followed only now — someone just mentioned them in it, or
        // they just wrote there — is read from its start too, as far as
        // `THREAD_BEFORE`, so a draft has what was asked (0.10.0-alpha.95).
        for thread in &newly {
            let Some(first) = raw
                .iter()
                .filter(|m| thread_name(m) == Some(thread.as_str()))
                .filter_map(|m| m.get("createTime").and_then(Value::as_str))
                .filter(|t| when(t).is_some())
                .min_by_key(|t| when(t))
                .map(str::to_string)
            else {
                continue;
            };
            match api.earlier_in_thread(&space.name, thread, &first, THREAD_BEFORE) {
                Ok(earlier) => {
                    let have: HashSet<String> =
                        raw.iter().filter_map(|m| m.get("name").and_then(Value::as_str)).map(str::to_string).collect();
                    let earlier: Vec<Value> = earlier
                        .into_iter()
                        .filter(|m| thread_name(m) == Some(thread.as_str()))
                        .filter(|m| m.get("name").and_then(Value::as_str).is_some_and(|n| !have.contains(n)))
                        .collect();
                    summary.fetched += earlier.len();
                    raw.extend(earlier);
                }
                Err(e @ GoogleChatError::SignIn(_)) => {
                    importer.abandon();
                    return Err(e);
                }
                Err(GoogleChatError::RateLimited) => {
                    summary.partial = true;
                    break;
                }
                Err(e) => {
                    tracing::warn!(target: "google_chat", error = %e, "a thread's earlier messages couldn't be read")
                }
            }
        }
        let read = |m: &Value| kept(m) && (!named || thread_name(m).is_some_and(|t| followed.contains(t)));
        // Who wrote, by name: as each message read says, else as the space's
        // members say.
        let unnamed = raw.iter().filter(|m| read(m)).any(|m| {
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
        // In a named space, a conversation is a thread.
        let mut threads: Vec<(String, Vec<(String, RawMessage)>)> = Vec::new();
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
            let thread = thread_name(m).map(str::to_string);
            if named {
                match thread.as_deref() {
                    None => {
                        summary.left_out += 1;
                        continue;
                    }
                    Some(t) if !followed.contains(t) => {
                        summary.not_for_you += 1;
                        continue;
                    }
                    Some(_) => {}
                }
                if author != config.user_id && mentions(m, &config.user_id) {
                    summary.mentions += 1;
                }
            }
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
            let said = (
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
                        "googleChatKind": space.kind,
                        "googleChatThread": thread.clone().map(Value::String).unwrap_or(Value::Null),
                        "attachments": attached,
                    }),
                },
            );
            match (named, thread) {
                (true, Some(thread)) => match threads.iter_mut().find(|(t, _)| *t == thread) {
                    Some((_, said_there)) => said_there.push(said),
                    None => threads.push((thread, vec![said])),
                },
                _ => messages.push(said),
            }
        }
        if named {
            let subject = space.display_name.clone().unwrap_or_else(|| "A space".to_string());
            for (thread, mut said) in threads {
                said.sort_by_key(|(t, _)| when(t));
                if let Err(e) = importer.take(DiscoveredConversation {
                    external_id: account_id(&thread),
                    subject: Some(subject.clone()),
                    channel: "chat".into(),
                    messages: said.into_iter().map(|(_, m)| m).collect(),
                }) {
                    importer.abandon();
                    return Err(e.into());
                }
                summary.chats += 1;
            }
            config.marks.insert(space.name.clone(), newest);
            continue;
        }
        messages.sort_by_key(|(t, _)| when(t));
        // A direct message only the user has written in yet is named by who
        // else is in it, not "Chat" (0.10.0-alpha.81).
        if others.is_empty() && space.kind == "DIRECT_MESSAGE" && !messages.is_empty() {
            let named = if members.is_empty() {
                api.members(&space.name).unwrap_or_default()
            } else {
                members.into_iter().collect()
            };
            others.extend(named.into_iter().filter(|(id, _)| *id != config.user_id).map(|(_, name)| name));
        }
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
pub(crate) mod live {
    //! A Chat API that keeps what it is told: spaces with their messages,
    //! oldest first; who signed in; a post added as the user's, a second
    //! after the space's last message, once per request id.
    use std::sync::{Arc, Mutex};

    use serde_json::{json, Value};

    pub(crate) struct Space {
        pub name: String,
        /// The other person, for a direct message.
        pub other: Option<(String, String)>,
        pub messages: Vec<Value>,
    }

    pub(crate) type Spaces = Arc<Mutex<Vec<Space>>>;

    pub(crate) fn said(space: &str, n: &str, user: &str, name: &str, created: &str, text: &str) -> Value {
        json!({
            "name": format!("{space}/messages/{n}"),
            "sender": { "name": user, "displayName": name, "type": "HUMAN" },
            "createTime": created,
            "text": text,
        })
    }

    /// The query's `createTime > "…"`, if it has one.
    fn after(query: &str) -> Option<String> {
        let filter = query.split('&').find_map(|p| p.strip_prefix("filter="))?;
        let decoded = percent(filter);
        let start = decoded.find('"')? + 1;
        let end = decoded[start..].find('"')? + start;
        Some(decoded[start..end].to_string())
    }

    fn percent(s: &str) -> String {
        let bytes = s.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(b);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    pub(crate) fn chat(spaces: Spaces, requests: Arc<Mutex<Vec<String>>>) -> crate::sources::teams::fake::Answer {
        Box::new(move |path, body| {
            let mut spaces = spaces.lock().unwrap();
            let answer = if path == "/userinfo" {
                json!({ "sub": "7", "name": "C Haynes", "email": "c@formicaria.example" })
            } else if path == "/spaces" {
                json!({ "spaces": spaces.iter().map(|s| {
                    let kind = if s.other.is_some() { "DIRECT_MESSAGE" } else { "GROUP_CHAT" };
                    let last = s.messages.last().and_then(|m| m["createTime"].as_str()).unwrap_or("2026-01-01T00:00:00Z");
                    json!({ "name": s.name, "spaceType": kind, "lastActiveTime": last })
                }).collect::<Vec<_>>() })
            } else if let Some(name) = path.strip_prefix('/').and_then(|p| p.strip_suffix("/messages")) {
                let Some(space) = spaces.iter_mut().find(|s| s.name == name) else {
                    return (404, r#"{"error":{"code":404,"status":"NOT_FOUND","message":"no space"}}"#.into());
                };
                if body.starts_with('{') {
                    // A post: `body` is what was sent, the request id kept.
                    let sent: Value = serde_json::from_str(body).unwrap();
                    let last =
                        space.messages.last().and_then(|m| m["createTime"].as_str()).unwrap_or("2026-01-01T00:00:00Z");
                    let at = chrono::DateTime::parse_from_rfc3339(last).unwrap() + chrono::Duration::seconds(1);
                    let created = at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
                    let n = format!("p{}", space.messages.len());
                    let message =
                        said(&space.name, &n, "users/7", "C Haynes", &created, sent["text"].as_str().unwrap());
                    space.messages.push(message.clone());
                    requests.lock().unwrap().push(n);
                    message
                } else {
                    let since = after(body);
                    let found: Vec<Value> = space
                        .messages
                        .iter()
                        .filter(|m| since.as_deref().is_none_or(|s| m["createTime"].as_str().unwrap_or("") > s))
                        .cloned()
                        .collect();
                    json!({ "messages": found })
                }
            } else {
                return (404, r#"{"error":{"code":404,"status":"NOT_FOUND","message":"no"}}"#.into());
            };
            (200, answer.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use super::*;
    use crate::sources::teams::fake::{graph as server, Answer, TOKEN};

    fn account(db: &Db) -> String {
        account_reading(db, true)
    }

    /// The account, its named spaces read or not.
    fn account_reading(db: &Db, spaces: bool) -> String {
        db.set_setting(crate::db::WITHIN_DAYS_SETTING, &0).unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::AccountId, &account_id("users/7")).unwrap();
        db.create_source(&new_source(&me(), spaces).unwrap()).unwrap().id
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
                    assert!(query.contains("orderBy=createTime%20DESC"), "{query}");
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
                // A named space: a mention of C; in another thread, one of
                // everyone and talk between others; and C's own message in a
                // third.
                "/spaces/ROOM/messages" if !later => {
                    let at = |user: &str| json!([{ "type": "USER_MENTION", "userMention": { "user": { "name": user }, "type": "MENTION" } }]);
                    let mut asked =
                        said("spaces/ROOM", 1, "users/44", "Alan Turing", 15, "@C Haynes can you look at the budget?");
                    asked["annotations"] = at("users/7");
                    let mut everyone =
                        said("spaces/ROOM", 2, "users/44", "Alan Turing", 16, "@all standup moved to 10");
                    everyone["annotations"] = at("users/all");
                    everyone["thread"] = json!({ "name": "spaces/ROOM/threads/t3" });
                    let mut chatter = said("spaces/ROOM", 3, "users/45", "Ada Lovelace", 17, "lunch?");
                    chatter["thread"] = json!({ "name": "spaces/ROOM/threads/t3" });
                    let mut own = said("spaces/ROOM", 4, "users/7", "C Haynes", 20, "shipped it");
                    own["thread"] = json!({ "name": "spaces/ROOM/threads/t2" });
                    (200, json!({ "messages": [asked, everyone, chatter, own] }).to_string())
                }
                other => panic!("not asked for: {other}"),
            }
        }));
        let summary = sync(&db, &source, &api, &|| false).unwrap();
        // The direct message, the group, and in the named space the thread C
        // was asked in and the thread C wrote in (0.10.0-alpha.91).
        assert_eq!((summary.chats, summary.inserted, summary.from_self), (4, 5, 2), "{summary:?}");
        assert_eq!((summary.rooms, summary.left_out), (1, 2), "{summary:?}");
        assert_eq!((summary.mentions, summary.not_for_you), (1, 2), "{summary:?}");
        let (subject, metadata): (String, String) = db
            .conn()
            .query_row(
                "SELECT c.subject, m.metadata_json FROM messages m JOIN conversations c ON c.id = m.conversation_id
                 WHERE m.external_id = 'google_chat:spaces/ROOM/messages/m1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(subject, "Everyone", "named by its space");
        let metadata: Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(metadata["googleChatKind"], "SPACE");
        assert_eq!(metadata["googleChatThread"], "spaces/ROOM/threads/t1");
        let in_space: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM messages WHERE external_id LIKE 'google_chat:spaces/ROOM/%'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(in_space, 2, "talk not addressed to C, and @all, aren't read");
        let waiting = db.threads_awaiting_reply(10).unwrap();
        assert!(
            waiting.iter().any(|t| t.conversation.subject.as_deref() == Some("Everyone")),
            "the mention waits for a reply"
        );
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
        assert_eq!((summary.inserted, summary.unchanged), (1, 2), "{summary:?}");
        assert_eq!(db.count_conversations().unwrap(), 4);
        assert!(seen.lock().unwrap().iter().all(|r| !r.starts_with("/spaces/APP")));
    }

    #[test]
    fn a_named_space_is_read_only_once_turned_on_and_then_each_thread_the_user_is_in_whole() {
        let db = Db::open_in_memory().unwrap();
        let source = account_reading(&db, false);
        let round = Arc::new(StdMutex::new(0u32));
        let (on, held) = (round.clone(), db.clone());
        let held_source = source.clone();
        let (api, seen) = chat_api(Box::new(move |path, _| {
            let round = *on.lock().unwrap();
            let at_c = json!([{ "type": "USER_MENTION", "userMention": { "user": { "name": "users/7" } } }]);
            let in_thread = |mut m: Value, t: &str| {
                m["thread"] = json!({ "name": format!("spaces/ROOM/threads/{t}") });
                m
            };
            match path {
                "/userinfo" => (200, USERINFO_BODY.into()),
                "/spaces" => {
                    let last = if round < 2 { "2026-10-26T09:14:00Z" } else { "2026-10-26T09:31:00Z" };
                    (200, json!({ "spaces": [{ "name": "spaces/ROOM", "spaceType": "SPACE", "displayName": "Budget", "lastActiveTime": last }] }).to_string())
                }
                "/spaces/ROOM/messages" if round == 1 => {
                    // A question to the room, C asked by name in its thread,
                    // and an answer after that names no one; in another
                    // thread, talk not to C, from someone the message doesn't
                    // name — so nobody need ask who they are.
                    let question = in_thread(
                        said("spaces/ROOM", 1, "users/44", "Alan Turing", 10, "who owns the Q3 budget?"),
                        "t1",
                    );
                    let mut asked =
                        in_thread(said("spaces/ROOM", 2, "users/45", "Grace Hopper", 11, "@C Haynes do you?"), "t1");
                    asked["annotations"] = at_c;
                    let after = in_thread(
                        said("spaces/ROOM", 3, "users/44", "Alan Turing", 12, "it's the one from March"),
                        "t1",
                    );
                    let mut other = in_thread(said("spaces/ROOM", 4, "users/46", "", 14, "lunch?"), "t2");
                    other["sender"]["displayName"] = Value::Null;
                    (200, json!({ "messages": [question, asked, after, other] }).to_string())
                }
                "/spaces/ROOM/messages" if round == 2 => {
                    // Reading spaces turned off while this check runs.
                    set_spaces(&held, &held_source, false).unwrap();
                    let more = in_thread(said("spaces/ROOM", 5, "users/44", "Alan Turing", 30, "any news?"), "t1");
                    let elsewhere =
                        in_thread(said("spaces/ROOM", 6, "users/47", "Ada Lovelace", 31, "sunny out"), "t2");
                    (200, json!({ "messages": [more, elsewhere] }).to_string())
                }
                other => panic!("not asked for: {other}"),
            }
        }));
        // Off, as an account connected before spaces were read: not asked.
        let summary = sync(&db, &source, &api, &|| false).unwrap();
        assert_eq!((summary.spaces_off, summary.chats, summary.fetched), (1, 0, 0), "{summary:?}");

        set_spaces(&db, &source, true).unwrap();
        *round.lock().unwrap() = 1;
        let summary = sync(&db, &source, &api, &|| false).unwrap();
        assert_eq!(
            (summary.chats, summary.inserted, summary.mentions, summary.not_for_you),
            (1, 3, 1, 1),
            "{summary:?}"
        );
        assert!(seen.lock().unwrap().iter().all(|r| !r.contains("/members")), "no one read needs naming");

        // The thread C was asked in is followed: what's said there after
        // reads without naming them; other threads still don't.
        *round.lock().unwrap() = 2;
        let summary = sync(&db, &source, &api, &|| false).unwrap();
        assert_eq!((summary.inserted, summary.not_for_you), (1, 1), "{summary:?}");
        let thread: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM messages m JOIN conversations c ON c.id = m.conversation_id
                 WHERE c.external_id = 'google_chat:spaces/ROOM/threads/t1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(thread, 4, "the whole thread, in order of its own");
        let config: GoogleChatSourceConfig =
            serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        assert!(!config.spaces, "turned off while the check ran, and kept so");
        assert_eq!(config.marks["spaces/ROOM"], "2026-10-26T09:31:00.123456Z");
    }

    #[test]
    fn a_thread_the_user_is_first_mentioned_in_is_read_from_its_start() {
        let db = Db::open_in_memory().unwrap();
        let source = account(&db);
        let round = Arc::new(StdMutex::new(0u32));
        let on = round.clone();
        let (api, seen) = chat_api(Box::new(move |path, query| {
            let round = *on.lock().unwrap();
            match path {
                "/userinfo" => (200, USERINFO_BODY.into()),
                "/spaces" => {
                    let last = if round == 0 { "2026-10-26T09:10:00Z" } else { "2026-10-26T09:20:00Z" };
                    (200, json!({ "spaces": [{ "name": "spaces/ROOM", "spaceType": "SPACE", "displayName": "Budget", "lastActiveTime": last }] }).to_string())
                }
                // What was asked before C was mentioned, in the thread alone.
                "/spaces/ROOM/messages" if query.contains("thread.name") => {
                    assert!(query.contains("createTime%20%3C%20"), "before the mention: {query}");
                    assert!(query.contains("spaces%2FROOM%2Fthreads%2Ft1"), "{query}");
                    (200, json!({ "messages": [said("spaces/ROOM", 1, "users/44", "Alan Turing", 10, "who owns the Q3 budget?")] }).to_string())
                }
                "/spaces/ROOM/messages" if round == 0 => {
                    (200, json!({ "messages": [said("spaces/ROOM", 1, "users/44", "Alan Turing", 10, "who owns the Q3 budget?")] }).to_string())
                }
                "/spaces/ROOM/messages" => {
                    let mut asked = said("spaces/ROOM", 2, "users/45", "Grace Hopper", 20, "@C Haynes can you answer the above?");
                    asked["annotations"] = json!([{ "type": "USER_MENTION", "userMention": { "user": { "name": "users/7" } } }]);
                    (200, json!({ "messages": [asked] }).to_string())
                }
                other => panic!("not asked for: {other}"),
            }
        }));
        let summary = sync(&db, &source, &api, &|| false).unwrap();
        assert_eq!((summary.inserted, summary.not_for_you), (0, 1), "no one asked C yet: {summary:?}");
        *round.lock().unwrap() = 1;
        let summary = sync(&db, &source, &api, &|| false).unwrap();
        assert_eq!((summary.inserted, summary.mentions), (2, 1), "{summary:?}");
        let thread: Vec<String> = {
            let conn = db.conn();
            let mut stmt = conn
                .prepare(
                    "SELECT m.body FROM messages m JOIN conversations c ON c.id = m.conversation_id
                     WHERE c.external_id = 'google_chat:spaces/ROOM/threads/t1' ORDER BY m.sequence_index",
                )
                .unwrap();
            stmt.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
        };
        assert_eq!(thread, vec!["who owns the Q3 budget?".to_string(), "@C Haynes can you answer the above?".into()]);
        assert_eq!(seen.lock().unwrap().iter().filter(|r| r.contains("thread.name")).count(), 1, "once");
    }

    #[test]
    fn a_reply_into_a_thread_says_so_and_one_into_a_chat_doesnt() {
        let (api, _) = chat_api(Box::new(|_, _| (404, "{}".into())));
        let threaded = api.post_target("spaces/ROOM", "r1", true);
        assert!(
            threaded.ends_with(
                "/spaces/ROOM/messages?requestId=r1&messageReplyOption=REPLY_MESSAGE_FALLBACK_TO_NEW_THREAD"
            ),
            "{threaded}"
        );
        assert!(api.post_target("spaces/DM1", "r1", false).ends_with("/spaces/DM1/messages?requestId=r1"));
        let at = |user: &str| json!({ "annotations": [{ "type": "USER_MENTION", "userMention": { "user": { "name": user } } }] });
        assert!(mentions(&at("users/7"), "users/7"));
        assert!(!mentions(&at("users/all"), "users/7"), "everyone is not them");
        assert!(!mentions(&json!({ "text": "users/7" }), "users/7"));
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
    fn a_direct_message_only_the_user_wrote_in_is_named_by_who_else_is_in_it_and_an_empty_one_isnt_asked_again() {
        let db = Db::open_in_memory().unwrap();
        let source = account(&db);
        let (api, seen) = chat_api(Box::new(|path, _| {
            match path {
            "/userinfo" => (200, USERINFO_BODY.into()),
            "/spaces" => (
                200,
                json!({ "spaces": [
                    { "name": "spaces/DM9", "spaceType": "DIRECT_MESSAGE", "lastActiveTime": "2026-10-26T09:01:00.123456Z" },
                    { "name": "spaces/OLD", "spaceType": "DIRECT_MESSAGE", "lastActiveTime": "2026-10-20T08:00:00Z" }
                ]})
                .to_string(),
            ),
            "/spaces/DM9/messages" => {
                (200, json!({ "messages": [said("spaces/DM9", 1, "users/7", "C Haynes", 1, "are you around?")] }).to_string())
            }
            // History off: nothing kept to read.
            "/spaces/OLD/messages" => (200, "{}".into()),
            "/spaces/DM9/members" => (
                200,
                json!({ "memberships": [
                    { "member": { "name": "users/7", "displayName": "C Haynes" } },
                    { "member": { "name": "users/42", "displayName": "Ada Lovelace" } }
                ]})
                .to_string(),
            ),
            other => panic!("not asked for: {other}"),
        }
        }));
        sync(&db, &source, &api, &|| false).unwrap();
        let subject: String = db
            .conn()
            .query_row("SELECT subject FROM conversations WHERE external_id = 'google_chat:spaces/DM9'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(subject, "Ada Lovelace");
        let config: GoogleChatSourceConfig =
            serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        assert_eq!(config.marks["spaces/OLD"], "2026-10-20T08:00:00Z");
        let asked = |p: &str| seen.lock().unwrap().iter().filter(|r| r.starts_with(p)).count();
        assert_eq!(asked("/spaces/OLD/messages"), 1);
        sync(&db, &source, &api, &|| false).unwrap();
        assert_eq!(asked("/spaces/OLD/messages"), 1, "nothing new by the list: not asked again");
    }

    #[test]
    fn nothing_sent_is_read_as_a_mention_or_a_link_and_ungranted_permissions_are_named() {
        assert_eq!(defused("thanks <users/all> and <users/42>!"), "thanks <\u{200B}users/all> and <\u{200B}users/42>!");
        assert_eq!(defused("see <https://x.example|this>"), "see <\u{200B}https://x.example|this>");
        assert_eq!(defused("2 < 3 and a <b> tag"), "2 < 3 and a <b> tag");
        let all = NEEDED.join(" ");
        assert!(missing(Some(format!("openid {all}").as_str())).is_empty());
        assert_eq!(missing(Some("openid https://www.googleapis.com/auth/chat.spaces.readonly")), NEEDED[1..].to_vec());
        assert!(missing(None).is_empty(), "Google not saying isn't a refusal");
        let insufficient = json!({ "error": { "code": 403, "status": "PERMISSION_DENIED",
            "message": "Request had insufficient authentication scopes.",
            "details": [{ "reason": "ACCESS_TOKEN_SCOPE_INSUFFICIENT" }] } });
        assert!(matches!(refusal(403, &insufficient), GoogleChatError::SignIn(why) if why.contains("leave every box")));
        let no_app = json!({ "error": { "code": 404, "status": "NOT_FOUND", "message": "Google Chat app not found. To create a Chat app, you must turn on the Chat API and configure the app in the Google Cloud console." } });
        assert!(matches!(refusal(404, &no_app), GoogleChatError::NotConfigured(_)));
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
