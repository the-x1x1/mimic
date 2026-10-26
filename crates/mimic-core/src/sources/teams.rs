//! The `teams` connector: Microsoft Teams chats — one-to-one and group —
//! read through Microsoft Graph as the user (0.10.0-alpha.67).
//!
//! Graph lets an app read someone's chats only with their sign-in: the user
//! signs in with Microsoft in their browser (`provider`: the
//! `organizations` tenant, since Graph's chats are only work and school
//! accounts'), granting `Chat.Read`, `ChatMessage.Send` (for a reply they
//! approve, later), `User.Read` and `offline_access`, through the same
//! registration Mimic signs mailboxes in with. What comes back that lasts —
//! the refresh token — is kept in the secret store (`secret_key`), never in
//! the source's settings; each check trades it for an access token
//! (`Access`), saving the one Microsoft hands back.
//!
//! A check (`sync`, the `check_teams` job) lists the user's chats
//! (`/me/chats`, with their members), reads each one's messages after the
//! last one read (`marks`; the newest `FIRST_READ` the first time), and
//! imports them, the user's and everyone else's. Meeting chats, system
//! messages and deleted ones are left out; HTML is read as text
//! (`mime::html_to_text`). Being asked to slow down ends the check early,
//! keeping what was read.

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

pub const CONNECTOR: &str = "teams";
pub const JOB_KIND: &str = "check_teams";
/// Microsoft Graph.
pub const GRAPH: &str = "https://graph.microsoft.com/v1.0";

/// What the sign-in asks for.
pub const SCOPE: &str = "https://graph.microsoft.com/Chat.Read https://graph.microsoft.com/ChatMessage.Send https://graph.microsoft.com/User.Read offline_access";

const FIRST_READ: usize = 300;
/// All that is new, in practice (40 pages of 50); more marks the check
/// partial.
const MOST_NEW: usize = 2000;
/// Graph's most per page of messages; chats come 50 at a time too.
const PAGE: &str = "50";
const MAX_PAGES: usize = 40;
const TIMEOUT: Duration = Duration::from_secs(30);
/// An access token is used until this long before it lapses.
const SPARE: Duration = Duration::from_secs(300);

/// Where a Teams sign-in is kept in the secret store.
pub fn secret_key(source_id: &str) -> String {
    format!("teams:{source_id}")
}

/// Someone in Teams, by their Microsoft account's id.
pub fn account_id(user_id: &str) -> String {
    format!("teams:{user_id}")
}

/// Signing in for Teams: Microsoft's, for work and school accounts only,
/// with Graph's scopes.
pub fn provider() -> Provider {
    Provider {
        key: "teams",
        name: "Microsoft",
        authorize_url: "https://login.microsoftonline.com/organizations/oauth2/v2.0/authorize".into(),
        token_url: "https://login.microsoftonline.com/organizations/oauth2/v2.0/token".into(),
        scope: SCOPE,
        imap_host: "",
        imap_port: 0,
        redirect_host: "localhost",
        authorize_extra: &[("response_mode", "query")],
        scope_in_token_requests: true,
        client_secret: None,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TeamsError {
    #[error("Microsoft wants you to sign in to Teams again: use Sign in again beside it under Your mail.")]
    SignIn(String),
    #[error("Teams said no ({0}). A personal Microsoft account's Teams can't be read this way; a work or school account's can, if its organisation allows it.")]
    Forbidden(String),
    #[error("Teams said no: {0}")]
    Refused(String),
    #[error("I couldn't reach Teams: {0}")]
    Network(String),
    #[error("Teams asked me to slow down; I'll read the rest on the next check.")]
    RateLimited,
    #[error("{0}")]
    Import(String),
    #[error("canceled")]
    Canceled,
}

impl From<DbError> for TeamsError {
    fn from(e: DbError) -> Self {
        TeamsError::Import(e.to_string())
    }
}

impl From<ImportError> for TeamsError {
    fn from(e: ImportError) -> Self {
        TeamsError::Import(e.to_string())
    }
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(300).collect()
}

/// Who signed in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
}

/// A chat, with who is in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chat {
    pub id: String,
    pub topic: Option<String>,
    /// `oneOnOne`, `group` or `meeting`.
    pub kind: String,
    /// Each member's account id, name and email, when the list gave them;
    /// otherwise asked for when the chat is read (`GraphApi::members`).
    pub members: Vec<(String, String, Option<String>)>,
    /// When its newest message was made, as the list says
    /// (`lastMessagePreview`, 0.10.0-alpha.70): a chat with nothing after
    /// its mark isn't read.
    pub last: Option<String>,
}

/// A chat's members, as Graph lists them.
fn members_in(list: Option<&Value>) -> Vec<(String, String, Option<String>)> {
    list.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let user = m.get("userId").and_then(Value::as_str)?.to_string();
            let name = m.get("displayName").and_then(Value::as_str).unwrap_or(&user).to_string();
            let email = m.get("email").and_then(Value::as_str).filter(|e| e.contains('@')).map(str::to_string);
            Some((user, name, email))
        })
        .collect()
}

/// Graph with one access token.
pub struct GraphApi {
    base: String,
    token: String,
    client: reqwest::blocking::Client,
}

impl GraphApi {
    pub fn new(token: &str) -> Result<Self, TeamsError> {
        Self::at(GRAPH, token)
    }

    /// At another address: a test's.
    pub fn at(base: &str, token: &str) -> Result<Self, TeamsError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| TeamsError::Network(one_line(&e.to_string())))?;
        Ok(GraphApi { base: base.trim_end_matches('/').to_string(), token: token.to_string(), client })
    }

    fn get(&self, url: &str) -> Result<Value, TeamsError> {
        let mut waits = 0;
        let response = loop {
            let response = self
                .client
                .get(url)
                .bearer_auth(&self.token)
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .map_err(|e| TeamsError::Network(one_line(&e.to_string())))?;
            if response.status().as_u16() != 429 {
                break response;
            }
            // Asked to wait a moment: as long as asked, then again (alpha.70).
            match super::slack::asked_to_wait(&response) {
                Some(wait) if waits < 4 => {
                    waits += 1;
                    std::thread::sleep(wait);
                }
                _ => return Err(TeamsError::RateLimited),
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
    pub fn me(&self) -> Result<Me, TeamsError> {
        let v = self.get(&format!("{}/me?$select=id,displayName,mail,userPrincipalName", self.base))?;
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        let id = s("id").ok_or_else(|| TeamsError::Refused("Microsoft didn't say who you are".into()))?;
        Ok(Me {
            name: s("displayName").unwrap_or_else(|| id.clone()),
            email: s("mail").or_else(|| s("userPrincipalName").filter(|u| u.contains('@'))),
            id,
        })
    }

    /// Whether this sign-in can do what a check does, asking for one of
    /// each: who it is, and the first of their chats (0.10.0-alpha.69).
    pub fn check(&self) -> Result<(Me, usize), TeamsError> {
        let me = self.me()?;
        let v = self.get(&format!("{}/me/chats?$top=1", self.base))?;
        let chats = v.get("value").and_then(Value::as_array).map_or(0, Vec::len);
        Ok((me, chats))
    }

    /// The user's chats, each with when its newest message was made.
    pub fn chats(&self) -> Result<Vec<Chat>, TeamsError> {
        let mut out = Vec::new();
        let mut next = Some(format!("{}/me/chats?$expand=lastMessagePreview&$top={PAGE}", self.base));
        for _ in 0..MAX_PAGES {
            let Some(url) = next.take() else { break };
            let v = self.get(&url)?;
            for c in v.get("value").and_then(Value::as_array).into_iter().flatten() {
                let Some(id) = c.get("id").and_then(Value::as_str) else { continue };
                out.push(Chat {
                    id: id.to_string(),
                    topic: c.get("topic").and_then(Value::as_str).filter(|t| !t.trim().is_empty()).map(str::to_string),
                    kind: c.get("chatType").and_then(Value::as_str).unwrap_or("").to_string(),
                    members: members_in(c.get("members")),
                    last: c
                        .get("lastMessagePreview")
                        .and_then(|p| p.get("createdDateTime"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                });
            }
            next = v.get("@odata.nextLink").and_then(Value::as_str).map(str::to_string);
        }
        Ok(out)
    }

    /// A chat's members (0.10.0-alpha.70).
    pub fn members(&self, chat: &str) -> Result<Vec<(String, String, Option<String>)>, TeamsError> {
        let v = self.get(&format!("{}/chats/{}/members", self.base, encode(chat)))?;
        Ok(members_in(v.get("value")))
    }

    /// A chat's messages created after `after` (all when none), newest
    /// first, at most `most`, and whether that was all of them.
    pub fn messages(&self, chat: &str, after: Option<&str>, most: usize) -> Result<(Vec<Value>, bool), TeamsError> {
        let mut out = Vec::new();
        let mut complete = false;
        let mut next = Some(format!(
            "{}/me/chats/{}/messages?$top={PAGE}&$orderby=createdDateTime%20desc",
            self.base,
            encode(chat)
        ));
        for _ in 0..MAX_PAGES {
            let Some(url) = next.take() else { break };
            let v = self.get(&url)?;
            let mut reached = false;
            for m in v.get("value").and_then(Value::as_array).into_iter().flatten() {
                let created = m.get("createdDateTime").and_then(Value::as_str).unwrap_or("");
                if after.is_some_and(|a| !later(created, a)) {
                    reached = true;
                    break;
                }
                out.push(m.clone());
            }
            next = v.get("@odata.nextLink").and_then(Value::as_str).map(str::to_string);
            if reached || next.is_none() {
                complete = out.len() <= most;
                break;
            }
            if out.len() >= most {
                break;
            }
        }
        out.truncate(most);
        Ok((out, complete))
    }
}

impl GraphApi {
    /// Post `text` into a chat as the user (0.10.0-alpha.68), as plain text:
    /// the new message's id and when Teams says it was made, or why not —
    /// and whether it may have gone anyway.
    pub fn post(&self, chat: &str, text: &str) -> Result<(String, Option<String>), PostError> {
        let response = self
            .client
            .post(format!("{}/chats/{}/messages", self.base, encode(chat)))
            .bearer_auth(&self.token)
            .json(&json!({ "body": { "contentType": "text", "content": text } }))
            .send();
        let response = match response {
            Ok(r) => r,
            Err(e) if e.is_connect() || e.is_builder() => {
                return Err(PostError::NotSent(TeamsError::Network(one_line(&e.to_string()))))
            }
            Err(e) => return Err(PostError::Unsure(one_line(&e.to_string()))),
        };
        let status = response.status().as_u16();
        if status == 429 {
            return Err(PostError::NotSent(TeamsError::RateLimited));
        }
        let value: Value = match response.json() {
            Ok(v) => v,
            Err(e) if (200..300).contains(&status) => {
                return Err(PostError::Unsure(format!("Teams answered {status}: {}", one_line(&e.to_string()))))
            }
            Err(_) => Value::Null,
        };
        if (200..300).contains(&status) {
            let created = value.get("createdDateTime").and_then(Value::as_str).map(str::to_string);
            return value
                .get("id")
                .and_then(Value::as_str)
                .map(|id| (id.to_string(), created))
                .ok_or_else(|| PostError::Unsure("Teams said it went, without saying which message it was".into()));
        }
        // A server's failure may have come after it took the message.
        if status >= 500 {
            return Err(PostError::Unsure(format!("Teams answered {status}")));
        }
        Err(PostError::NotSent(refusal(status, &value)))
    }
}

/// Why a post didn't go — or might have.
#[derive(Debug, thiserror::Error)]
pub enum PostError {
    #[error(transparent)]
    NotSent(TeamsError),
    /// Asked, and no clear answer came back: it may have gone.
    #[error("I lost Teams' answer as it was being sent ({0}), so it may or may not have gone.")]
    Unsure(String),
}

/// A chat id in a path: Teams' hold `:` and `@`.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b':' | b'@' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A Graph time (ISO 8601) as a moment, for order.
fn when(t: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(t).ok()
}

/// Whether Graph time `a` is after `b`.
fn later(a: &str, b: &str) -> bool {
    match (when(a), when(b)) {
        (Some(a), Some(b)) => a > b,
        _ => a > b,
    }
}

/// Why Graph said no, from its status and answer.
fn refusal(status: u16, value: &Value) -> TeamsError {
    let error = value.get("error");
    let code = error.and_then(|e| e.get("code")).and_then(Value::as_str).unwrap_or("").to_string();
    let message = error.and_then(|e| e.get("message")).and_then(Value::as_str).map(one_line).unwrap_or_default();
    let said = if message.is_empty() { format!("{status} {code}") } else { format!("{code}: {message}") };
    match status {
        401 => TeamsError::SignIn(said),
        403 => TeamsError::Forbidden(said),
        _ => TeamsError::Refused(said),
    }
}

// ---------------------------------------------------------------- tokens

/// Access tokens for each connected Teams account: kept in memory until
/// `SPARE` before they lapse, otherwise traded for the refresh token in the
/// secret store, saving the one Microsoft hands back.
pub struct Access {
    provider: Provider,
    /// The client id sign-ins use now (the user's registration, or this
    /// build's).
    client: ClientFor,
    read: PasswordFor,
    write: SaveSecret,
    cache: Mutex<HashMap<String, (String, Instant)>>,
}

/// The client id Microsoft sign-ins use now, when there is one.
pub type ClientFor = Arc<dyn Fn() -> Option<String> + Send + Sync>;

pub const NOT_SIGNED_IN: &str =
    "I don't have a Teams sign-in I can use for this account. Use Sign in again beside it under Your mail.";
pub const NO_CLIENT: &str = "This copy of Mimic isn't set up to sign in with Microsoft yet. Settings, under Signing in with Microsoft and Google, says how.";

impl Access {
    pub fn new(provider: Provider, client: ClientFor, read: PasswordFor, write: SaveSecret) -> Self {
        Access { provider, client, read, write, cache: Mutex::new(HashMap::new()) }
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
        let client = (self.client)().ok_or_else(|| NO_CLIENT.to_string())?;
        match oauth::refresh(&self.provider, &client, &refresh) {
            Ok(tokens) => {
                if let Some(new) = tokens.refresh_token.as_deref().filter(|t| *t != refresh) {
                    if let Err(e) = (self.write)(&key, new) {
                        tracing::warn!(target: "teams", error = %e, "a new Teams sign-in could not be saved");
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

    /// Forget a token Teams turned down.
    pub fn forget(&self, source_id: &str) {
        self.cache.lock().unwrap_or_else(|p| p.into_inner()).remove(source_id);
    }
}

// ---------------------------------------------------------------- reading

/// A Teams account connected for its chats.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamsSourceConfig {
    pub user_id: String,
    pub user: String,
    #[serde(default)]
    pub email: Option<String>,
    /// When the newest message read was created, per chat.
    #[serde(default)]
    pub marks: BTreeMap<String, String>,
    #[serde(default)]
    pub last_checked_at: Option<String>,
    /// When the last check that read everything through began.
    #[serde(default)]
    pub last_read_at: Option<String>,
}

impl TeamsSourceConfig {
    pub fn for_me(me: &Me) -> Self {
        TeamsSourceConfig {
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
        name: format!("{} (Teams)", me.email.as_deref().unwrap_or(&me.name)),
        channel: "chat".into(),
        location: None,
        config: serde_json::to_value(TeamsSourceConfig::for_me(me))?,
    })
}

/// The Teams source already connected as this account, if one is.
pub fn connected_as(db: &Db, me: &Me) -> Result<Option<String>, DbError> {
    Ok(db
        .list_sources()?
        .into_iter()
        .filter(|s| s.connector == CONNECTOR)
        .find(|s| serde_json::from_value::<TeamsSourceConfig>(s.config.clone()).is_ok_and(|c| c.is(me)))
        .map(|s| s.id))
}

/// What one check did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamsSummary {
    pub chats: usize,
    pub fetched: usize,
    pub inserted: usize,
    pub duplicates: usize,
    pub from_self: usize,
    /// System messages, deleted ones, and ones without words.
    pub left_out: usize,
    /// Meeting chats, which aren't read.
    pub meetings: usize,
    /// Asked to slow down, or a chat had more new than a check reads.
    pub partial: bool,
    /// Chats Teams wouldn't give: passed over, the rest read.
    pub unreadable: usize,
    /// Chats with nothing new since they were last read, by the list's
    /// account of their newest message: not asked for (0.10.0-alpha.70).
    #[serde(default)]
    pub unchanged: usize,
}

/// Check an account: read what is new in each chat, import it, and move
/// each chat's mark past what was read.
pub fn sync(
    db: &Db,
    source_id: &str,
    api: &GraphApi,
    should_stop: &dyn Fn() -> bool,
) -> Result<TeamsSummary, TeamsError> {
    let source = db.get_source(source_id)?.ok_or_else(|| TeamsError::Import(format!("no source {source_id}")))?;
    let mut config: TeamsSourceConfig = match serde_json::from_value(source.config.clone()) {
        Ok(c) => c,
        Err(e) => {
            let err = TeamsError::Import(format!(
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
        Err(TeamsError::Canceled) => db.set_source_status(source_id, "ready", None)?,
        Err(e) => db.set_source_status(source_id, "failed", Some(&json!({ "message": e.to_string() })))?,
    }
    result
}

/// A check that couldn't start (no sign-in): recorded as an attempt.
pub fn record_failure(db: &Db, source_id: &str, message: &str) -> Result<(), DbError> {
    if let Some(source) = db.get_source(source_id)? {
        if let Ok(mut config) = serde_json::from_value::<TeamsSourceConfig>(source.config) {
            config.last_checked_at = Some(crate::ids::now_rfc3339());
            db.set_source_config(source_id, &serde_json::to_value(&config).unwrap_or(Value::Null))?;
        }
        db.set_source_status(source_id, "failed", Some(&json!({ "message": message })))?;
    }
    Ok(())
}

/// A message's words: its HTML read as text, or its text.
fn words_of(m: &Value) -> String {
    let body = m.get("body");
    let content = body.and_then(|b| b.get("content")).and_then(Value::as_str).unwrap_or("");
    let html = body.and_then(|b| b.get("contentType")).and_then(Value::as_str) == Some("html");
    let text = if html { super::mime::html_to_text(content) } else { content.to_string() };
    text.trim().to_string()
}

fn sync_inner(
    db: &Db,
    source_id: &str,
    config: &mut TeamsSourceConfig,
    api: &GraphApi,
    should_stop: &dyn Fn() -> bool,
) -> Result<TeamsSummary, TeamsError> {
    let me = api.me()?;
    if !config.is(&me) {
        return Err(TeamsError::Refused(format!(
            "this sign-in is {}'s, not the account this was connected with",
            me.name
        )));
    }
    let chats = api.chats()?;
    let mut summary = TeamsSummary::default();
    let mut tried = 0usize;
    let mut last_refusal = TeamsError::Refused("no chat could be read".into());
    let mut importer = Importer::begin(db, source_id)?;
    for chat in chats {
        if should_stop() {
            importer.abandon();
            return Err(TeamsError::Canceled);
        }
        if chat.kind != "oneOnOne" && chat.kind != "group" {
            summary.meetings += 1;
            continue;
        }
        let mark = config.marks.get(&chat.id).cloned();
        // Nothing after the mark, by the list's account: not asked.
        if let (Some(last), Some(mark)) = (chat.last.as_deref(), mark.as_deref()) {
            if !later(last, mark) {
                summary.unchanged += 1;
                continue;
            }
        }
        let most = if mark.is_some() { MOST_NEW } else { FIRST_READ };
        tried += 1;
        let raw = match api.messages(&chat.id, mark.as_deref(), most) {
            Ok((raw, complete)) => {
                if mark.is_some() && !complete {
                    summary.partial = true;
                }
                raw
            }
            Err(TeamsError::RateLimited) => {
                summary.partial = true;
                break;
            }
            // The sign-in, or Teams itself: every chat would fail the same way.
            Err(e @ (TeamsError::SignIn(_) | TeamsError::Network(_))) => {
                importer.abandon();
                return Err(e);
            }
            // This chat alone (one left, or hidden from the account): passed
            // over, the rest read.
            Err(e) => {
                tracing::warn!(target: "teams", error = %e, "a chat couldn't be read");
                last_refusal = e;
                summary.unreadable += 1;
                continue;
            }
        };
        let Some(newest) = raw
            .iter()
            .filter_map(|m| m.get("createdDateTime").and_then(Value::as_str))
            .filter(|t| when(t).is_some())
            .max_by_key(|t| when(t))
        else {
            continue;
        };
        let newest = newest.to_string();
        summary.fetched += raw.len();
        // Who is in it, when the list didn't say: their addresses, and the
        // chat's name. Without them, by name alone.
        let members = if chat.members.is_empty() {
            match api.members(&chat.id) {
                Ok(m) => m,
                Err(TeamsError::RateLimited) => {
                    summary.partial = true;
                    break;
                }
                Err(e @ (TeamsError::SignIn(_) | TeamsError::Network(_))) => {
                    importer.abandon();
                    return Err(e);
                }
                Err(_) => Vec::new(),
            }
        } else {
            chat.members.clone()
        };
        let emails: HashMap<&str, &str> =
            members.iter().filter_map(|(id, _, e)| e.as_deref().map(|e| (id.as_str(), e))).collect();
        let others: Vec<&(String, String, Option<String>)> =
            members.iter().filter(|(id, _, _)| *id != config.user_id).collect();
        let mut messages: Vec<(String, RawMessage)> = Vec::new();
        for m in &raw {
            let user = m.get("from").and_then(|f| f.get("user"));
            let (Some(id), Some(created)) =
                (m.get("id").and_then(Value::as_str), m.get("createdDateTime").and_then(Value::as_str))
            else {
                summary.left_out += 1;
                continue;
            };
            let Some(author) = user.and_then(|u| u.get("id")).and_then(Value::as_str) else {
                // An app's, a bot's, or the system's.
                summary.left_out += 1;
                continue;
            };
            if m.get("messageType").and_then(Value::as_str) != Some("message")
                || m.get("deletedDateTime").is_some_and(|d| !d.is_null())
            {
                summary.left_out += 1;
                continue;
            }
            let body = words_of(m);
            if body.is_empty() {
                summary.left_out += 1;
                continue;
            }
            let name = user
                .and_then(|u| u.get("displayName"))
                .and_then(Value::as_str)
                .filter(|n| !n.trim().is_empty())
                .unwrap_or(author)
                .to_string();
            let mut identifiers = vec![IdentifierInput::new(IdentifierKind::AccountId, account_id(author))];
            if let Some(email) = emails.get(author) {
                identifiers.push(IdentifierInput::new(IdentifierKind::Email, *email));
            }
            let sent_at = chrono::DateTime::parse_from_rfc3339(created)
                .ok()
                .map(|t| t.with_timezone(&chrono::Utc).to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
            messages.push((
                created.to_string(),
                RawMessage {
                    external_id: format!("{}:{id}", account_id(&chat.id)),
                    author: AuthorRef { display_name: name, identifiers },
                    sent_at,
                    body,
                    metadata: json!({
                        "teamsChat": chat.id,
                        // To the millisecond, for asking what came after it.
                        "teamsCreated": created,
                        "teamsDirect": chat.kind == "oneOnOne",
                        // A file, a card or a quoted message; or a picture
                        // pasted into the words.
                        "attachments": m.get("attachments").and_then(Value::as_array).is_some_and(|a| !a.is_empty())
                            || m.get("body").and_then(|b| b.get("content")).and_then(Value::as_str).is_some_and(|c| c.contains("<img")),
                    }),
                },
            ));
        }
        messages.sort_by_key(|(t, _)| when(t));
        if !messages.is_empty() {
            let subject = chat.topic.clone().unwrap_or_else(|| {
                let names: Vec<&str> = others.iter().map(|(_, n, _)| n.as_str()).collect();
                if names.is_empty() {
                    "Chat".to_string()
                } else {
                    names.join(", ")
                }
            });
            if let Err(e) = importer.take(DiscoveredConversation {
                external_id: account_id(&chat.id),
                subject: Some(subject),
                channel: "chat".into(),
                messages: messages.into_iter().map(|(_, m)| m).collect(),
            }) {
                importer.abandon();
                return Err(e.into());
            }
            summary.chats += 1;
        }
        config.marks.insert(chat.id.clone(), newest);
    }
    // Not one would Teams give, and none was passed over for having nothing
    // new: that is the account — a permission, say — not a chat
    // (0.10.0-alpha.72). One chat always refused beside quiet ones is that
    // chat, and the account works.
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
        let Ok(config) = serde_json::from_value::<TeamsSourceConfig>(s.config.clone()) else { continue };
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

pub struct CheckTeamsExecutor {
    access: Arc<Access>,
}

impl CheckTeamsExecutor {
    pub fn shared(access: Arc<Access>) -> Arc<dyn crate::jobs::JobExecutor> {
        Arc::new(CheckTeamsExecutor { access })
    }
}

impl crate::jobs::JobExecutor for CheckTeamsExecutor {
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
                .ok_or_else(|| crate::jobs::JobError::Failed("no Teams account was named".into()))?;
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
                    let api = GraphApi::new(&token)?;
                    sync(&db, &source_id, &api, &|| stop_ctx.check_cancel().is_err())
                }))
            });
            let Ok(result) = result else {
                let why = "Something went wrong inside Mimic while reading Teams.";
                record_failure(&db, &source_id, why)?;
                return Err(crate::jobs::JobError::Failed(why.into()));
            };
            match result {
                Ok(summary) => Ok(serde_json::to_value(summary).unwrap_or(Value::Null)),
                Err(TeamsError::Canceled) => Err(crate::jobs::JobError::Canceled),
                Err(e) => {
                    if matches!(e, TeamsError::SignIn(_)) {
                        access.forget(&source_id);
                    }
                    Err(crate::jobs::JobError::Failed(e.to_string()))
                }
            }
        })
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! A Graph to talk to: each request answered by `answer` from its path
    //! and query, every request kept.
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    pub(crate) type Answer = Box<dyn Fn(&str, &str) -> (u16, String) + Send>;
    pub(crate) type Seen = Arc<Mutex<Vec<String>>>;
    pub(crate) const TOKEN: &str = "graph-access-token";

    pub(crate) fn graph(answer: Answer) -> (String, Seen) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let base = format!("http://127.0.0.1:{}/v1.0", listener.local_addr().unwrap().port());
        let seen: Seen = Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                if reader.read_line(&mut first).unwrap_or(0) == 0 {
                    continue;
                }
                let mut authorized = false;
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim_end().is_empty() {
                        break;
                    }
                    let lower = line.to_ascii_lowercase();
                    if lower.trim_end() == format!("authorization: bearer {TOKEN}") {
                        authorized = true;
                    }
                    if let Some(n) = lower.strip_prefix("content-length:") {
                        length = n.trim().parse().unwrap_or(0);
                    }
                }
                let mut sent = vec![0; length];
                if reader.read_exact(&mut sent).is_err() {
                    continue;
                }
                let target = first.split_whitespace().nth(1).unwrap_or("").to_string();
                let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
                let path = path.trim_start_matches("/v1.0").to_string();
                let query =
                    if sent.is_empty() { query.to_string() } else { String::from_utf8_lossy(&sent).into_owned() };
                kept.lock().unwrap().push(format!("{path}?{query}"));
                let (status, body) = if authorized {
                    answer(&path, &query)
                } else {
                    (
                        401,
                        r#"{"error":{"code":"InvalidAuthenticationToken","message":"Access token is empty."}}"#.into(),
                    )
                };
                if status == 0 {
                    continue;
                }
                // "wait:N|…" answers with `Retry-After: N`.
                let (extra, body) = match body.strip_prefix("wait:").and_then(|b| b.split_once('|')) {
                    Some((n, rest)) => (format!("Retry-After: {n}\r\n"), rest.to_string()),
                    None => (String::new(), body),
                };
                let mut w = stream;
                let reply = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = w.write_all(reply.as_bytes());
            }
        });
        (base, seen)
    }
}

#[cfg(test)]
pub(crate) mod live {
    //! A Graph that keeps what it is told: chats with their messages, newest
    //! last; history newest first; a post added as the user's, a second
    //! after the chat's last message.
    use std::sync::{Arc, Mutex};

    use serde_json::{json, Value};

    pub(crate) struct Chat {
        pub id: String,
        /// The other person, for a one-to-one chat.
        pub other: Option<(String, String)>,
        pub messages: Vec<Value>,
    }

    pub(crate) type Chats = Arc<Mutex<Vec<Chat>>>;

    pub(crate) fn said(user: &str, name: &str, id: &str, created: &str, text: &str) -> Value {
        json!({
            "id": id, "messageType": "message", "createdDateTime": created, "deletedDateTime": null,
            "from": { "user": { "id": user, "displayName": name } },
            "body": { "contentType": "text", "content": text }, "attachments": []
        })
    }

    pub(crate) fn graph(chats: Chats) -> super::fake::Answer {
        Box::new(move |path, body| {
            let mut chats = chats.lock().unwrap();
            let answer = if path == "/me" {
                json!({ "id": "u1", "displayName": "C Haynes", "mail": "c@formicaria.example" })
            } else if path == "/me/chats" {
                json!({ "value": chats.iter().map(|c| {
                    let mut members = vec![json!({ "userId": "u1", "displayName": "C Haynes" })];
                    match &c.other {
                        Some((id, name)) => members.push(json!({ "userId": id, "displayName": name })),
                        None => members.push(json!({ "userId": "u3", "displayName": "Bob" })),
                    }
                    let kind = if c.other.is_some() { "oneOnOne" } else { "group" };
                    json!({ "id": c.id, "chatType": kind, "members": members })
                }).collect::<Vec<_>>() })
            } else if let Some(id) = path.strip_prefix("/me/chats/").and_then(|p| p.strip_suffix("/messages")) {
                let mut found: Vec<Value> =
                    chats.iter().filter(|c| c.id == id).flat_map(|c| c.messages.iter().cloned()).collect();
                found.reverse();
                json!({ "value": found })
            } else if let Some(id) = path.strip_prefix("/chats/").and_then(|p| p.strip_suffix("/messages")) {
                let sent: Value = serde_json::from_str(body).unwrap();
                let Some(chat) = chats.iter_mut().find(|c| c.id == id) else {
                    return (404, r#"{"error":{"code":"NotFound","message":"no chat"}}"#.into());
                };
                let last =
                    chat.messages.last().and_then(|m| m["createdDateTime"].as_str()).unwrap_or("2026-01-01T00:00:00Z");
                let at = chrono::DateTime::parse_from_rfc3339(last).unwrap() + chrono::Duration::seconds(1);
                let id = format!("p{}", chat.messages.len());
                let created = at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                chat.messages.push(said("u1", "C Haynes", &id, &created, sent["body"]["content"].as_str().unwrap()));
                json!({ "id": id, "createdDateTime": created })
            } else {
                return (404, r#"{"error":{"code":"NotFound","message":"no"}}"#.into());
            };
            (200, answer.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::fake::TOKEN;
    use super::*;

    const ME: &str = r#"{"id":"u1","displayName":"C Haynes","mail":"c@formicaria.example","userPrincipalName":"c@formicaria.example"}"#;
    const CHATS: &str = r##"{"value":[
        {"id":"19:a1b2@unq.gbl.spaces","chatType":"oneOnOne","topic":null,"members":[
            {"@odata.type":"#microsoft.graph.aadUserConversationMember","userId":"u1","displayName":"C Haynes","email":"c@formicaria.example"},
            {"@odata.type":"#microsoft.graph.aadUserConversationMember","userId":"u2","displayName":"Ada Lovelace","email":"ada@formicaria.example"}]},
        {"id":"19:group@thread.v2","chatType":"group","topic":"Offsite","members":[
            {"userId":"u1","displayName":"C Haynes"},{"userId":"u3","displayName":"Bob"}]},
        {"id":"19:meeting_x@thread.v2","chatType":"meeting","topic":"Standup","members":[]}]}"##;

    fn answer(round: Arc<AtomicUsize>) -> fake::Answer {
        Box::new(move |path, _query| {
            let round = round.load(Ordering::SeqCst);
            let body = match path {
                "/me" => ME.to_string(),
                "/me/chats" => CHATS.to_string(),
                p if p.contains("a1b2") && round == 0 => r#"{"value":[
                    {"id":"m3","messageType":"message","createdDateTime":"2026-10-26T08:50:00.120Z","deletedDateTime":null,
                     "from":{"user":{"id":"u2","displayName":"Ada Lovelace"}},
                     "body":{"contentType":"html","content":"<p>can you <b>check</b> the deck &amp; send it?</p>"},"attachments":[]},
                    {"id":"m2","messageType":"systemEventMessage","createdDateTime":"2026-10-26T08:40:00Z","from":null,"body":{"contentType":"html","content":"<systemEventMessage/>"}},
                    {"id":"m1","messageType":"message","createdDateTime":"2026-10-26T08:30:00Z","deletedDateTime":null,
                     "from":{"user":{"id":"u1","displayName":"C Haynes"}},
                     "body":{"contentType":"text","content":"morning!"},"attachments":[]},
                    {"id":"m0","messageType":"message","createdDateTime":"2026-10-26T08:20:00Z","deletedDateTime":"2026-10-26T08:21:00Z",
                     "from":{"user":{"id":"u2","displayName":"Ada Lovelace"}},"body":{"contentType":"text","content":"oops"}}]}"#
                    .to_string(),
                p if p.contains("a1b2") => r#"{"value":[
                    {"id":"m4","messageType":"message","createdDateTime":"2026-10-26T09:05:00Z",
                     "from":{"user":{"id":"u2","displayName":"Ada Lovelace"}},"body":{"contentType":"text","content":"thanks!"},"attachments":[]},
                    {"id":"m3","messageType":"message","createdDateTime":"2026-10-26T08:50:00.120Z",
                     "from":{"user":{"id":"u2","displayName":"Ada Lovelace"}},"body":{"contentType":"text","content":"x"},"attachments":[]}]}"#
                    .to_string(),
                p if p.contains("group") && round == 0 => r#"{"value":[
                    {"id":"g1","messageType":"message","createdDateTime":"2026-10-26T08:55:00Z",
                     "from":{"user":{"id":"u3","displayName":"Bob"}},"body":{"contentType":"text","content":"who's driving?"},
                     "attachments":[{"contentType":"reference","name":"map.pdf"}]}]}"#
                    .to_string(),
                p if p.contains("group") => r#"{"value":[]}"#.to_string(),
                _ => return (404, r#"{"error":{"code":"NotFound","message":"no"}}"#.into()),
            };
            (200, body)
        })
    }

    fn account(db: &Db) -> String {
        db.set_user_identity("C Haynes").unwrap();
        db.add_user_identifier(IdentifierKind::AccountId, &account_id("u1")).unwrap();
        db.set_setting(crate::db::WITHIN_DAYS_SETTING, &0).unwrap();
        let me = Me { id: "u1".into(), name: "C Haynes".into(), email: Some("c@formicaria.example".into()) };
        let source = db.create_source(&new_source(&me).unwrap()).unwrap();
        assert_eq!(connected_as(db, &me).unwrap().as_deref(), Some(source.id.as_str()));
        source.id
    }

    #[test]
    fn a_check_reads_chats_both_sides_then_only_whats_new() {
        let round = Arc::new(AtomicUsize::new(0));
        let (base, seen) = fake::graph(answer(round.clone()));
        let db = Db::open_in_memory().unwrap();
        let source = account(&db);
        let api = GraphApi::at(&base, TOKEN).unwrap();

        let first = sync(&db, &source, &api, &|| false).unwrap();
        assert_eq!((first.chats, first.inserted, first.from_self, first.left_out, first.meetings), (2, 3, 1, 2, 1));
        let waiting: Vec<String> = db.threads_awaiting_reply(10).unwrap().into_iter().map(|t| t.last_message).collect();
        assert!(waiting.contains(&"can you check the deck & send it?".to_string()), "{waiting:?}");
        assert!(waiting.contains(&"who's driving?".to_string()), "{waiting:?}");
        let config: TeamsSourceConfig =
            serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        assert_eq!(config.marks.get("19:a1b2@unq.gbl.spaces").map(String::as_str), Some("2026-10-26T08:50:00.120Z"));
        assert!(seen.lock().unwrap().iter().any(|r| r.starts_with("/me/chats/19:a1b2@unq.gbl.spaces/messages?")));

        // Only what came after; the message at the mark isn't read again.
        round.store(1, Ordering::SeqCst);
        let next = sync(&db, &source, &api, &|| false).unwrap();
        assert_eq!((next.inserted, next.duplicates, next.fetched), (1, 0, 1));
        let people = db.list_participants(10).unwrap();
        let ada = people.iter().find(|p| p.participant.display_name == "Ada Lovelace").expect("Ada");
        let ada = db.get_participant(&ada.participant.id).unwrap().unwrap();
        assert!(ada.identifiers.iter().any(|i| i.value == "ada@formicaria.example"), "her email too");
    }

    #[test]
    fn a_sign_in_turned_down_or_a_personal_account_says_what_to_do() {
        let (base, _) = fake::graph(Box::new(|_, _| (200, "{}".into())));
        let err = GraphApi::at(&base, "someone-elses").unwrap().me().unwrap_err();
        assert!(matches!(err, TeamsError::SignIn(_)), "{err:?}");
        assert!(err.to_string().contains("sign in to Teams again"), "{err}");

        let (base, _) = fake::graph(Box::new(|path, _| match path {
            "/me" => (200, ME.into()),
            _ => {
                (403, r#"{"error":{"code":"Forbidden","message":"Missing scope permissions on the request."}}"#.into())
            }
        }));
        let db = Db::open_in_memory().unwrap();
        let source = account(&db);
        let err = sync(&db, &source, &GraphApi::at(&base, TOKEN).unwrap(), &|| false).unwrap_err();
        assert!(matches!(err, TeamsError::Forbidden(_)), "{err:?}");
        assert!(err.to_string().contains("personal Microsoft account"), "{err}");
        assert_eq!(db.get_source(&source).unwrap().unwrap().status, "failed");
    }

    #[test]
    fn an_account_whose_every_chat_is_refused_fails_the_check() {
        let (base, _) = fake::graph(Box::new(|path, _| match path {
            "/me" => (200, ME.into()),
            "/me/chats" => (200, r#"{"value":[{"id":"19:a@x","chatType":"oneOnOne","members":[]}]}"#.into()),
            _ => (403, r#"{"error":{"code":"Forbidden","message":"Missing role permissions on the request."}}"#.into()),
        }));
        let db = Db::open_in_memory().unwrap();
        let source = account(&db);
        let err = sync(&db, &source, &GraphApi::at(&base, TOKEN).unwrap(), &|| false).unwrap_err();
        assert!(matches!(err, TeamsError::Forbidden(_)), "{err:?}");
        assert_eq!(db.get_source(&source).unwrap().unwrap().status, "failed");

        // Beside a chat with nothing new, the one refused is only that chat.
        let (base, _) = fake::graph(Box::new(|path, _| {
            match path {
            "/me" => (200, ME.into()),
            "/me/chats" => (
                200,
                r#"{"value":[{"id":"19:a@x","chatType":"oneOnOne","members":[]},
                    {"id":"19:quiet@x","chatType":"oneOnOne","lastMessagePreview":{"createdDateTime":"2026-10-26T07:00:00Z"}}]}"#
                    .into(),
            ),
            _ => (403, r#"{"error":{"code":"Forbidden","message":"Missing role permissions on the request."}}"#.into()),
        }
        }));
        let db = Db::open_in_memory().unwrap();
        let source = account(&db);
        let mut config: TeamsSourceConfig =
            serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        config.marks.insert("19:quiet@x".into(), "2026-10-26T07:00:00Z".into());
        db.set_source_config(&source, &serde_json::to_value(&config).unwrap()).unwrap();
        let summary = sync(&db, &source, &GraphApi::at(&base, TOKEN).unwrap(), &|| false).unwrap();
        assert_eq!((summary.unreadable, summary.unchanged), (1, 1), "{summary:?}");
        assert_ne!(db.get_source(&source).unwrap().unwrap().status, "failed");
    }

    #[test]
    fn a_chat_teams_wont_give_is_passed_over_and_a_pasted_picture_is_something_attached() {
        let (base, _) = fake::graph(Box::new(|path, _| {
            match path {
            "/me" => (200, ME.into()),
            "/me/chats" => (
                200,
                r#"{"value":[{"id":"19:gone@thread.v2","chatType":"group","members":[]},
                    {"id":"19:a1b2@unq.gbl.spaces","chatType":"oneOnOne","members":[{"userId":"u2","displayName":"Ada Lovelace"}]}]}"#
                    .into(),
            ),
            p if p.contains("gone") => (404, r#"{"error":{"code":"NotFound","message":"Chat not found."}}"#.into()),
            _ => (
                200,
                r#"{"value":[{"id":"m1","messageType":"message","createdDateTime":"2026-10-26T08:50:00Z",
                    "from":{"user":{"id":"u2","displayName":"Ada Lovelace"}},
                    "body":{"contentType":"html","content":"<p>this one?</p><p><img src=\"https://graph.microsoft.com/v1.0/chats/x/messages/m1/hostedContents/a/$value\"></p>"},
                    "attachments":[]}]}"#
                    .into(),
            ),
        }
        }));
        let db = Db::open_in_memory().unwrap();
        let source = account(&db);
        let summary = sync(&db, &source, &GraphApi::at(&base, TOKEN).unwrap(), &|| false).unwrap();
        assert_eq!((summary.inserted, summary.unreadable, summary.partial), (1, 1, false));
        let metadata: String = db
            .conn()
            .query_row(
                "SELECT metadata_json FROM messages WHERE external_id = 'teams:19:a1b2@unq.gbl.spaces:m1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(metadata.contains(r#""attachments":true"#), "{metadata}");
    }

    #[test]
    fn only_a_chat_with_something_new_is_read_and_waiting_when_asked() {
        let asked = Arc::new(AtomicUsize::new(0));
        let count = asked.clone();
        let (base, seen) = fake::graph(Box::new(move |path, _| {
            match path {
            "/me" => (200, ME.into()),
            "/me/chats" => (
                200,
                r#"{"value":[
                    {"id":"19:a1b2@unq.gbl.spaces","chatType":"oneOnOne","lastMessagePreview":{"createdDateTime":"2026-10-26T09:05:00Z"}},
                    {"id":"19:quiet@unq.gbl.spaces","chatType":"oneOnOne","lastMessagePreview":{"createdDateTime":"2026-10-26T07:00:00Z"}}]}"#
                    .into(),
            ),
            p if p.ends_with("/members") => (
                200,
                r#"{"value":[{"userId":"u1","displayName":"C Haynes"},{"userId":"u2","displayName":"Ada Lovelace","email":"ada@formicaria.example"}]}"#.into(),
            ),
            p if p.contains("a1b2") && count.fetch_add(1, Ordering::SeqCst) == 0 => {
                (429, r#"wait:1|{"error":{"code":"TooManyRequests","message":"slow down"}}"#.into())
            }
            p if p.contains("a1b2") => (
                200,
                r#"{"value":[{"id":"m4","messageType":"message","createdDateTime":"2026-10-26T09:05:00Z",
                    "from":{"user":{"id":"u2","displayName":"Ada Lovelace"}},"body":{"contentType":"text","content":"thanks!"},"attachments":[]}]}"#
                    .into(),
            ),
            _ => (500, r#"{"error":{"code":"Unexpected","message":"read a quiet chat"}}"#.into()),
        }
        }));
        let db = Db::open_in_memory().unwrap();
        let source = account(&db);
        let mut config: TeamsSourceConfig =
            serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        config.marks.insert("19:a1b2@unq.gbl.spaces".into(), "2026-10-26T08:50:00Z".into());
        config.marks.insert("19:quiet@unq.gbl.spaces".into(), "2026-10-26T07:00:00Z".into());
        db.set_source_config(&source, &serde_json::to_value(&config).unwrap()).unwrap();
        let summary = sync(&db, &source, &GraphApi::at(&base, TOKEN).unwrap(), &|| false).unwrap();
        assert_eq!((summary.inserted, summary.unchanged, summary.partial), (1, 1, false), "{summary:?}");
        assert!(!seen.lock().unwrap().iter().any(|r| r.contains("quiet@unq.gbl.spaces/messages")), "not asked");
        let ada = db.list_participants(10).unwrap().into_iter().find(|p| p.participant.display_name == "Ada Lovelace");
        let ada = db.get_participant(&ada.expect("Ada").participant.id).unwrap().unwrap();
        assert!(ada.identifiers.iter().any(|i| i.value == "ada@formicaria.example"), "her address, from the members");
    }

    #[test]
    fn an_access_token_is_kept_until_it_lapses_and_a_new_sign_in_is_saved() {
        let (url, forms) = oauth::fake::token_endpoint(vec![(
            200,
            r#"{"access_token":"a1","refresh_token":"r2","expires_in":3600,"token_type":"Bearer"}"#.into(),
        )]);
        let saved: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let keep = saved.clone();
        let access = Access::new(
            Provider { token_url: url, ..provider() },
            Arc::new(|| Some("client-1".to_string())),
            Arc::new(|key: &str| (key == "teams:s1").then(|| "r1".to_string())),
            Arc::new(move |key: &str, value: &str| {
                keep.lock().unwrap().push((key.into(), value.into()));
                Ok(())
            }),
        );
        assert_eq!(access.token("s1").unwrap(), "a1");
        assert_eq!(access.token("s1").unwrap(), "a1", "kept, not asked again");
        let form = forms.recv().unwrap();
        assert!(form.contains("refresh_token=r1") && form.contains("client_id=client-1"), "{form}");
        assert!(form.contains("Chat.Read"), "{form}");
        assert_eq!(saved.lock().unwrap().as_slice(), [("teams:s1".to_string(), "r2".to_string())]);
        assert_eq!(access.token("s2").unwrap_err(), NOT_SIGNED_IN);
    }
}
