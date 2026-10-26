//! The `slack` connector: a Slack workspace's direct messages, read with the
//! user's own token (0.10.0-alpha.64).
//!
//! Slack lets an app read a person's direct messages, and write as them,
//! only with a token that person made: a *user* token (`xoxp-…`) from a Slack
//! app of their own, installed to their workspace with the user scopes
//! `im:history`, `im:read`, `mpim:history`, `mpim:read` and `users:read`
//! (`users:read.email` to match people with their email too; `chat:write`
//! to send a reply the user approves, `post`, 0.10.0-alpha.65). The token is kept in the secret store
//! (`secret_key`), never in the source's settings.
//!
//! A check (`sync`, the `check_slack` job) lists the user's direct messages
//! and group messages (`users.conversations`), reads each one's messages
//! after the last one read (`conversations.history` from `marks`; the newest
//! `FIRST_READ` the first time), and imports them as conversations of their
//! own — the user's and everyone else's — like a mailbox's. Bots, joins and
//! other notices (`subtype`) and messages without words are left out.
//! Slack asking it to slow down ends the check early, keeping what was read.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use chrono::{SecondsFormat, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{AuthorRef, DiscoveredConversation, RawMessage};
use crate::db::repo_people::IdentifierInput;
use crate::db::{Db, DbError, IdentifierKind};
use crate::import::{ImportError, Importer};

pub const CONNECTOR: &str = "slack";
pub const JOB_KIND: &str = "check_slack";
/// Slack's Web API.
pub const API: &str = "https://slack.com/api";

/// How many of a conversation's newest messages the first check reads.
const FIRST_READ: usize = 500;
/// How many new messages one check reads from a conversation at most: all
/// of them, in practice; more than this marks the check partial.
const MOST_NEW: usize = 10_000;
/// Results asked for per page, and pages per list at most.
const PAGE: &str = "200";
const MAX_PAGES: usize = 50;
const TIMEOUT: Duration = Duration::from_secs(30);
/// When Slack asks a check to slow down (429 with `Retry-After`), it waits
/// as long as asked, up to this, and asks again — at most `WAITS` times a
/// request (0.10.0-alpha.70) — rather than stopping: a workspace with more
/// conversations than a minute's requests is read through in one check.
const MOST_WAIT: Duration = Duration::from_secs(30);
const WAITS: usize = 4;

/// How long a 429 asks to wait, when it says, and the wait is one to keep.
pub(crate) fn asked_to_wait(response: &reqwest::blocking::Response) -> Option<Duration> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .filter(|wait| *wait <= MOST_WAIT)
}

/// Where a workspace's token is kept in the secret store.
pub fn secret_key(source_id: &str) -> String {
    format!("slack:{source_id}")
}

/// Whether `token` reads as a user token. A bot's (`xoxb-…`) can't read the
/// user's direct messages, or write as them.
pub fn is_user_token(token: &str) -> bool {
    token.starts_with("xoxp-") && token.len() > 20 && token.bytes().all(|b| b.is_ascii_graphic())
}

/// A person's address in a workspace.
pub fn account_id(team_id: &str, user_id: &str) -> String {
    format!("slack:{team_id}:{user_id}")
}

#[derive(Debug, thiserror::Error)]
pub enum SlackError {
    #[error("Slack turned the token down ({0}). Make a new one in your Slack app and give it to me under Your mail.")]
    Token(String),
    #[error("The token can't {what}: add the {scope} scope under your Slack app's User Token Scopes, reinstall the app to the workspace, and give me the new token.")]
    Scope { scope: String, what: &'static str },
    #[error("Slack said no: {0}")]
    Refused(String),
    #[error("I couldn't reach Slack: {0}")]
    Network(String),
    #[error("Slack asked me to slow down; I'll read the rest on the next check.")]
    RateLimited,
    #[error("{0}")]
    Import(String),
    #[error("canceled")]
    Canceled,
}

impl From<DbError> for SlackError {
    fn from(e: DbError) -> Self {
        SlackError::Import(e.to_string())
    }
}

impl From<ImportError> for SlackError {
    fn from(e: ImportError) -> Self {
        SlackError::Import(e.to_string())
    }
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(300).collect()
}

/// Whose token it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub team_id: String,
    pub team: String,
    pub user_id: String,
    pub user: String,
}

/// A direct message (`other`: who it's with) or a group message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub id: String,
    pub other: Option<String>,
}

/// Someone in the workspace, as `users.info` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub name: String,
    pub email: Option<String>,
}

/// Slack's Web API with one token.
pub struct SlackApi {
    base: String,
    token: String,
    client: reqwest::blocking::Client,
}

fn next_cursor(v: &Value) -> String {
    v.get("response_metadata").and_then(|m| m.get("next_cursor")).and_then(Value::as_str).unwrap_or("").to_string()
}

impl SlackApi {
    pub fn new(token: &str) -> Result<Self, SlackError> {
        Self::at(API, token)
    }

    /// At another address: a test's.
    pub fn at(base: &str, token: &str) -> Result<Self, SlackError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| SlackError::Network(one_line(&e.to_string())))?;
        Ok(SlackApi { base: base.trim_end_matches('/').to_string(), token: token.to_string(), client })
    }

    /// One Web API method: its answer when Slack says `ok`, or why not.
    fn call(&self, method: &str, params: &[(&str, &str)], what: &'static str) -> Result<Value, SlackError> {
        let mut waits = 0;
        let response = loop {
            let response = self
                .client
                .get(format!("{}/{method}", self.base))
                .bearer_auth(&self.token)
                .query(params)
                .send()
                .map_err(|e| SlackError::Network(one_line(&e.to_string())))?;
            if response.status().as_u16() != 429 {
                break response;
            }
            match asked_to_wait(&response) {
                Some(wait) if waits < WAITS => {
                    waits += 1;
                    std::thread::sleep(wait);
                }
                _ => return Err(SlackError::RateLimited),
            }
        };
        let value: Value = response.json().map_err(|e| SlackError::Network(one_line(&e.to_string())))?;
        if value.get("ok").and_then(Value::as_bool) == Some(true) {
            return Ok(value);
        }
        Err(refusal(&value, what))
    }

    pub fn auth_test(&self) -> Result<Identity, SlackError> {
        let v = self.call("auth.test", &[], "say whose it is")?;
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let identity = Identity { team_id: s("team_id"), team: s("team"), user_id: s("user_id"), user: s("user") };
        if identity.team_id.is_empty() || identity.user_id.is_empty() {
            return Err(SlackError::Refused("Slack didn't say whose token this is".into()));
        }
        Ok(identity)
    }

    /// The user's direct and group messages.
    pub fn conversations(&self) -> Result<Vec<Conversation>, SlackError> {
        let mut out = Vec::new();
        let mut cursor = String::new();
        for _ in 0..MAX_PAGES {
            let mut params = vec![("types", "im,mpim"), ("exclude_archived", "true"), ("limit", PAGE)];
            if !cursor.is_empty() {
                params.push(("cursor", cursor.as_str()));
            }
            let v = self.call("users.conversations", &params, "list your direct messages")?;
            for c in v.get("channels").and_then(Value::as_array).into_iter().flatten() {
                if let Some(id) = c.get("id").and_then(Value::as_str) {
                    out.push(Conversation {
                        id: id.to_string(),
                        other: c.get("user").and_then(Value::as_str).map(str::to_string),
                    });
                }
            }
            cursor = next_cursor(&v);
            if cursor.is_empty() {
                break;
            }
        }
        Ok(out)
    }

    /// A conversation's messages after `oldest` (every one when none), as
    /// Slack gives them — newest first — at most `most`, and whether that
    /// was all of them.
    pub fn history(&self, channel: &str, oldest: Option<&str>, most: usize) -> Result<(Vec<Value>, bool), SlackError> {
        let mut out = Vec::new();
        let mut cursor = String::new();
        let mut complete = false;
        for _ in 0..MAX_PAGES {
            let mut params = vec![("channel", channel), ("limit", PAGE)];
            if let Some(oldest) = oldest {
                params.push(("oldest", oldest));
            }
            if !cursor.is_empty() {
                params.push(("cursor", cursor.as_str()));
            }
            let v = self.call("conversations.history", &params, "read your direct messages")?;
            out.extend(v.get("messages").and_then(Value::as_array).cloned().unwrap_or_default());
            cursor = next_cursor(&v);
            let more = v.get("has_more").and_then(Value::as_bool) == Some(true);
            if !more || cursor.is_empty() {
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

    /// Whether this token can do what a check does, asking for one of each:
    /// whose it is, the first direct message, its newest message, and who
    /// the user is.
    pub fn check(&self) -> Result<Identity, SlackError> {
        let me = self.auth_test()?;
        let v =
            self.call("users.conversations", &[("types", "im,mpim"), ("limit", "1")], "list your direct messages")?;
        let first = v
            .get("channels")
            .and_then(Value::as_array)
            .and_then(|c| c.first())
            .and_then(|c| c.get("id"))
            .and_then(Value::as_str);
        if let Some(id) = first {
            self.call("conversations.history", &[("channel", id), ("limit", "1")], "read your direct messages")?;
        }
        self.user(&me.user_id)?;
        Ok(me)
    }

    /// Post `text` into a conversation as the user (`chat.postMessage`,
    /// 0.10.0-alpha.65): the new message's `ts`, or why not — and whether it
    /// may have gone anyway.
    pub fn post(&self, channel: &str, text: &str) -> Result<String, PostError> {
        let response = self
            .client
            .post(format!("{}/chat.postMessage", self.base))
            .bearer_auth(&self.token)
            .json(&json!({ "channel": channel, "text": escaped(text) }))
            .send();
        let response = match response {
            Ok(r) => r,
            // Never reached Slack: nothing went.
            Err(e) if e.is_connect() || e.is_builder() => {
                return Err(PostError::NotSent(SlackError::Network(one_line(&e.to_string()))))
            }
            Err(e) => return Err(PostError::Unsure(one_line(&e.to_string()))),
        };
        let status = response.status().as_u16();
        if status == 429 {
            return Err(PostError::NotSent(SlackError::RateLimited));
        }
        let value: Value = match response.json() {
            Ok(v) => v,
            Err(e) => return Err(PostError::Unsure(format!("Slack answered {status}: {}", one_line(&e.to_string())))),
        };
        if value.get("ok").and_then(Value::as_bool) == Some(true) {
            return value
                .get("ts")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| PostError::Unsure("Slack said it went, without saying which message it was".into()));
        }
        // Slack's own failures may come after it took the message.
        let error = value.get("error").and_then(Value::as_str).unwrap_or("");
        if status >= 500
            || matches!(error, "fatal_error" | "internal_error" | "service_unavailable" | "request_timeout")
        {
            return Err(PostError::Unsure(format!("Slack answered {status} {error}")));
        }
        if error == "rate_limited" {
            return Err(PostError::NotSent(SlackError::RateLimited));
        }
        Err(PostError::NotSent(refusal(&value, "send as you")))
    }

    /// One message of a conversation, by its `ts` (0.10.0-alpha.66): with
    /// `reply_count` and `reply_users` when its thread has replies.
    pub fn message(&self, channel: &str, ts: &str) -> Result<Option<Value>, SlackError> {
        let v = self.call(
            "conversations.history",
            &[("channel", channel), ("latest", ts), ("inclusive", "true"), ("limit", "1")],
            "read your direct messages",
        )?;
        Ok(v.get("messages")
            .and_then(Value::as_array)
            .and_then(|m| m.iter().find(|m| m.get("ts").and_then(Value::as_str) == Some(ts)))
            .cloned())
    }

    pub fn user(&self, id: &str) -> Result<Person, SlackError> {
        let v = self.call("users.info", &[("user", id)], "say who people are")?;
        let user = v.get("user").cloned().unwrap_or(Value::Null);
        let profile = user.get("profile");
        let pick =
            |s: Option<&Value>| s.and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        let name = pick(profile.and_then(|p| p.get("display_name")))
            .or_else(|| pick(profile.and_then(|p| p.get("real_name"))))
            .or_else(|| pick(user.get("real_name")))
            .or_else(|| pick(user.get("name")))
            .unwrap_or_else(|| id.to_string());
        Ok(Person { name, email: pick(profile.and_then(|p| p.get("email"))) })
    }
}

/// Why Slack said no, from its answer.
fn refusal(value: &Value, what: &'static str) -> SlackError {
    let error = value.get("error").and_then(Value::as_str).unwrap_or("unknown_error").to_string();
    match error.as_str() {
        "invalid_auth" | "not_authed" | "token_revoked" | "token_expired" | "account_inactive" => {
            SlackError::Token(error)
        }
        "missing_scope" => SlackError::Scope {
            scope: value.get("needed").and_then(Value::as_str).unwrap_or("needed").to_string(),
            what,
        },
        "ratelimited" => SlackError::RateLimited,
        _ => SlackError::Refused(error),
    }
}

/// Why a post didn't go — or might have.
#[derive(Debug, thiserror::Error)]
pub enum PostError {
    #[error(transparent)]
    NotSent(SlackError),
    /// Asked, and no clear answer came back: it may have gone.
    #[error("I lost Slack's answer as it was being sent ({0}), so it may or may not have gone.")]
    Unsure(String),
}

/// Text as Slack should show it, word for word: its markup characters
/// escaped, so `<@U1>` or `<!channel>` typed in a reply is shown, not acted
/// on. `readable` reads it back to what was written.
pub fn escaped(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// A message's text as people read it: `<@U1>` as `@name`, `<#C1|general>`
/// as `#general`, `<https://…|label>` as its label, `<!here>` as `@here`, and
/// Slack's `&amp;`, `&lt;` and `&gt;` as what they stand for.
pub fn readable(text: &str, name_of: &dyn Fn(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('>') else {
            out.push('<');
            rest = after;
            continue;
        };
        let inner = &after[..close];
        let (target, label) = match inner.split_once('|') {
            Some((t, l)) => (t, Some(l)),
            None => (inner, None),
        };
        let shown = if let Some(user) = target.strip_prefix('@') {
            format!("@{}", label.map_or_else(|| name_of(user), str::to_string))
        } else if let Some(channel) = target.strip_prefix('#') {
            format!("#{}", label.unwrap_or(channel))
        } else if let Some(special) = target.strip_prefix('!') {
            match special.split('^').next().unwrap_or("") {
                "here" => "@here".to_string(),
                "channel" => "@channel".to_string(),
                "everyone" => "@everyone".to_string(),
                _ => label.unwrap_or("").to_string(),
            }
        } else {
            label.unwrap_or(target).to_string()
        };
        out.push_str(&shown);
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// Slack's `ts` (`1700000000.000100`) as (seconds, microseconds), for order.
fn ts_key(ts: &str) -> (u64, u64) {
    let (secs, micros) = ts.split_once('.').unwrap_or((ts, "0"));
    (secs.parse().unwrap_or(0), micros.parse().unwrap_or(0))
}

pub(crate) fn ts_time(ts: &str) -> Option<String> {
    let (secs, _) = ts_key(ts);
    Utc.timestamp_opt(i64::try_from(secs).ok()?, 0).single().map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true))
}

/// Who wrote a message, or was named in it.
fn mentioned(text: &str) -> Vec<String> {
    text.match_indices("<@")
        .filter_map(|(at, _)| {
            let rest = &text[at + 2..];
            let end = rest.find(['>', '|'])?;
            Some(rest[..end].to_string())
        })
        .collect()
}

/// A workspace connected for its direct messages.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlackSourceConfig {
    pub team_id: String,
    pub team: String,
    pub user_id: String,
    pub user: String,
    /// The last message read, per conversation.
    #[serde(default)]
    pub marks: BTreeMap<String, String>,
    #[serde(default)]
    pub last_checked_at: Option<String>,
    /// When the last check that read everything through began.
    #[serde(default)]
    pub last_read_at: Option<String>,
}

impl SlackSourceConfig {
    pub fn for_identity(me: &Identity) -> Self {
        SlackSourceConfig {
            team_id: me.team_id.clone(),
            team: me.team.clone(),
            user_id: me.user_id.clone(),
            user: me.user.clone(),
            ..Default::default()
        }
    }

    pub fn is(&self, me: &Identity) -> bool {
        self.team_id == me.team_id && self.user_id == me.user_id
    }
}

/// A new workspace's source, read as chat.
pub fn new_source(me: &Identity) -> Result<crate::db::NewSource, serde_json::Error> {
    Ok(crate::db::NewSource {
        connector: CONNECTOR.into(),
        name: format!("{} (Slack)", me.team),
        channel: "chat".into(),
        location: None,
        config: serde_json::to_value(SlackSourceConfig::for_identity(me))?,
    })
}

/// The workspace already connected as this token's account, if one is.
pub fn connected_as(db: &Db, me: &Identity) -> Result<Option<String>, DbError> {
    Ok(db
        .list_sources()?
        .into_iter()
        .filter(|s| s.connector == CONNECTOR)
        .find(|s| serde_json::from_value::<SlackSourceConfig>(s.config.clone()).is_ok_and(|c| c.is(me)))
        .map(|s| s.id))
}

/// What one check did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlackSummary {
    pub conversations: usize,
    pub fetched: usize,
    pub inserted: usize,
    pub duplicates: usize,
    pub from_self: usize,
    /// Bots, notices and messages without words.
    pub left_out: usize,
    /// Slack asked to slow down before every conversation was read, or one
    /// had more new messages than a check reads.
    pub partial: bool,
    /// Conversations Slack wouldn't give (one closed or gone since it was
    /// listed): passed over, the rest read.
    pub unreadable: usize,
}

/// Check a workspace: read what is new in each direct and group message,
/// import it, and move each conversation's mark past what was read.
pub fn sync(
    db: &Db,
    source_id: &str,
    api: &SlackApi,
    should_stop: &dyn Fn() -> bool,
) -> Result<SlackSummary, SlackError> {
    let source = db.get_source(source_id)?.ok_or_else(|| SlackError::Import(format!("no source {source_id}")))?;
    let mut config: SlackSourceConfig = match serde_json::from_value(source.config.clone()) {
        Ok(c) => c,
        Err(e) => {
            let err = SlackError::Import(format!(
                "this workspace's settings are unreadable ({e}); remove it and connect it again"
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
        Err(SlackError::Canceled) => db.set_source_status(source_id, "ready", None)?,
        Err(e) => db.set_source_status(source_id, "failed", Some(&json!({ "message": e.to_string() })))?,
    }
    result
}

/// A check that couldn't start (no token): recorded as an attempt, so the
/// schedule waits before the next.
pub fn record_failure(db: &Db, source_id: &str, message: &str) -> Result<(), DbError> {
    if let Some(source) = db.get_source(source_id)? {
        if let Ok(mut config) = serde_json::from_value::<SlackSourceConfig>(source.config) {
            config.last_checked_at = Some(crate::ids::now_rfc3339());
            db.set_source_config(source_id, &serde_json::to_value(&config).unwrap_or(Value::Null))?;
        }
        db.set_source_status(source_id, "failed", Some(&json!({ "message": message })))?;
    }
    Ok(())
}

fn sync_inner(
    db: &Db,
    source_id: &str,
    config: &mut SlackSourceConfig,
    api: &SlackApi,
    should_stop: &dyn Fn() -> bool,
) -> Result<SlackSummary, SlackError> {
    let me = api.auth_test()?;
    if !config.is(&me) {
        return Err(SlackError::Refused(format!(
            "this token is {}'s in {}, not the account this workspace was connected with",
            me.user, me.team
        )));
    }
    let conversations = api.conversations()?;
    let mut people: HashMap<String, Person> = HashMap::new();
    let mut summary = SlackSummary::default();
    let (mut tried, mut last_refusal) = (0usize, String::new());
    let mut importer = Importer::begin(db, source_id)?;
    'conversations: for conversation in conversations {
        if should_stop() {
            importer.abandon();
            return Err(SlackError::Canceled);
        }
        let mark = config.marks.get(&conversation.id).cloned();
        let most = if mark.is_some() { MOST_NEW } else { FIRST_READ };
        tried += 1;
        let raw = match api.history(&conversation.id, mark.as_deref(), most) {
            Ok((raw, complete)) => {
                // More new than a check reads: what is between is passed over.
                if mark.is_some() && !complete {
                    summary.partial = true;
                }
                raw
            }
            Err(SlackError::RateLimited) => {
                summary.partial = true;
                break;
            }
            // The token, a scope, or Slack itself: every conversation would
            // fail the same way.
            Err(e @ (SlackError::Token(_) | SlackError::Scope { .. } | SlackError::Network(_))) => {
                importer.abandon();
                return Err(e);
            }
            // This conversation alone: passed over, the rest read.
            Err(e) => {
                tracing::warn!(target: "slack", error = %e, "a conversation couldn't be read");
                // Slack's own word for it, once: the error says "Slack said no".
                last_refusal = match e {
                    SlackError::Refused(why) => why,
                    other => other.to_string(),
                };
                summary.unreadable += 1;
                continue;
            }
        };
        let Some(newest) = raw.iter().filter_map(|m| m.get("ts").and_then(Value::as_str)).max_by_key(|ts| ts_key(ts))
        else {
            continue;
        };
        let newest = newest.to_string();
        summary.fetched += raw.len();
        // Everyone who wrote, or was named, by name.
        let mut wanted: Vec<String> = conversation.other.iter().cloned().collect();
        for m in &raw {
            wanted.extend(m.get("user").and_then(Value::as_str).map(str::to_string));
            wanted.extend(mentioned(m.get("text").and_then(Value::as_str).unwrap_or("")));
        }
        for id in wanted {
            if people.contains_key(&id) {
                continue;
            }
            match api.user(&id) {
                Ok(person) => {
                    people.insert(id, person);
                }
                Err(SlackError::RateLimited) => {
                    summary.partial = true;
                    break 'conversations;
                }
                Err(e @ (SlackError::Token(_) | SlackError::Scope { .. } | SlackError::Network(_))) => {
                    importer.abandon();
                    return Err(e);
                }
                // Someone Slack won't say (gone from the workspace): known
                // by their id.
                Err(_) => {
                    people.insert(id.clone(), Person { name: id, email: None });
                }
            }
        }
        let name_of = |id: &str| people.get(id).map_or_else(|| id.to_string(), |p| p.name.clone());
        let mut messages: Vec<(String, RawMessage)> = Vec::new();
        for m in &raw {
            let (Some(ts), Some(user)) = (m.get("ts").and_then(Value::as_str), m.get("user").and_then(Value::as_str))
            else {
                summary.left_out += 1;
                continue;
            };
            // Joins, edits shown as their own messages, and bots.
            let subtype = m.get("subtype").and_then(Value::as_str);
            if subtype.is_some_and(|s| s != "thread_broadcast" && s != "file_share") || m.get("bot_id").is_some() {
                summary.left_out += 1;
                continue;
            }
            let body = readable(m.get("text").and_then(Value::as_str).unwrap_or(""), &name_of).trim().to_string();
            if body.is_empty() {
                summary.left_out += 1;
                continue;
            }
            let mut identifiers =
                vec![IdentifierInput::new(IdentifierKind::AccountId, account_id(&config.team_id, user))];
            if let Some(email) = people.get(user).and_then(|p| p.email.as_deref()) {
                identifiers.push(IdentifierInput::new(IdentifierKind::Email, email));
            }
            messages.push((
                ts.to_string(),
                RawMessage {
                    external_id: format!("{}:{}", account_id(&config.team_id, &conversation.id), ts),
                    author: AuthorRef { display_name: name_of(user), identifiers },
                    sent_at: ts_time(ts),
                    body,
                    // Whether it is a direct message, which alone is
                    // answered by itself, and whether it came with a file
                    // a reply by itself would answer without reading
                    // (0.10.0-alpha.66).
                    metadata: json!({
                        "slackChannel": conversation.id,
                        "slackDirect": conversation.other.is_some(),
                        // Files, and what Slack attaches: a shared or
                        // forwarded message, whose words aren't in the text.
                        "attachments": m.get("files").and_then(Value::as_array).is_some_and(|f| !f.is_empty())
                            || m.get("attachments").and_then(Value::as_array).is_some_and(|a| !a.is_empty())
                            || subtype == Some("file_share"),
                    }),
                },
            ));
        }
        messages.sort_by_key(|(ts, _)| ts_key(ts));
        if !messages.is_empty() {
            let subject = match &conversation.other {
                Some(other) => name_of(other),
                None => {
                    let mut names: Vec<String> = messages
                        .iter()
                        .map(|(_, m)| m.author.display_name.clone())
                        .filter(|n| *n != name_of(&config.user_id))
                        .collect();
                    names.sort();
                    names.dedup();
                    if names.is_empty() {
                        "Group message".to_string()
                    } else {
                        names.join(", ")
                    }
                }
            };
            if let Err(e) = importer.take(DiscoveredConversation {
                external_id: account_id(&config.team_id, &conversation.id),
                subject: Some(subject),
                channel: "chat".into(),
                messages: messages.into_iter().map(|(_, m)| m).collect(),
            }) {
                importer.abandon();
                return Err(e.into());
            }
            summary.conversations += 1;
        }
        config.marks.insert(conversation.id.clone(), newest);
    }
    // Not one would Slack give: that is the workspace, not a conversation
    // (0.10.0-alpha.72).
    if tried > 0 && summary.unreadable == tried {
        importer.abandon();
        return Err(SlackError::Refused(format!("it gave none of your conversations ({last_refusal})")));
    }
    let imported = importer.finish()?;
    summary.inserted = imported.inserted;
    summary.duplicates = imported.duplicates;
    summary.from_self = imported.from_self;
    Ok(summary)
}

// -------------------------------------------------------------- schedule

/// Workspaces due a check at `now`, on the mailboxes' interval: connected,
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
        let Ok(config) = serde_json::from_value::<SlackSourceConfig>(s.config.clone()) else { continue };
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

/// At start-up: a workspace left "importing" by a check that never finished
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

/// Reads a workspace's token from wherever the app keeps secrets.
pub type TokenFor = std::sync::Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

pub struct CheckSlackExecutor {
    token_for: TokenFor,
}

impl CheckSlackExecutor {
    pub fn shared(token_for: TokenFor) -> std::sync::Arc<dyn crate::jobs::JobExecutor> {
        std::sync::Arc::new(CheckSlackExecutor { token_for })
    }
}

impl crate::jobs::JobExecutor for CheckSlackExecutor {
    fn kinds(&self) -> &'static [&'static str] {
        &[JOB_KIND]
    }
    fn resumable(&self, _kind: &str) -> bool {
        true
    }
    fn execute(&self, ctx: crate::jobs::JobContext) -> crate::jobs::JobFuture {
        let token_for = self.token_for.clone();
        Box::pin(async move {
            let source_id = ctx.job.payload["sourceId"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| crate::jobs::JobError::Failed("no workspace was named".into()))?;
            let Some(token) = token_for(&secret_key(&source_id)) else {
                let why =
                    "I don't have a Slack token I can use for this workspace. Give it to me again under Your mail.";
                record_failure(&ctx.db, &source_id, why)?;
                return Err(crate::jobs::JobError::Failed(why.into()));
            };
            let db = ctx.db.clone();
            let stop_ctx = ctx.clone();
            // Slack is asked with a blocking client, which may not be made or
            // dropped on the runtime's own threads.
            let result = tokio::task::block_in_place(|| {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let api = SlackApi::new(&token)?;
                    sync(&db, &source_id, &api, &|| stop_ctx.check_cancel().is_err())
                }))
            });
            // A panic would leave the workspace marked as being read.
            let Ok(result) = result else {
                let why = "Something went wrong inside Mimic while reading this workspace.";
                record_failure(&db, &source_id, why)?;
                return Err(crate::jobs::JobError::Failed(why.into()));
            };
            match result {
                Ok(summary) => Ok(serde_json::to_value(summary).unwrap_or(Value::Null)),
                Err(SlackError::Canceled) => Err(crate::jobs::JobError::Canceled),
                Err(e) => Err(crate::jobs::JobError::Failed(e.to_string())),
            }
        })
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! A Slack to talk to: each request answered by `answer` from its method
    //! and query, and every request's method and query kept.
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    use super::{account_id, connected_as, new_source, Identity};
    use crate::db::{Db, IdentifierKind};

    pub(crate) const TOKEN: &str = "xoxp-1111111111-2222222222-abcdefabcdef";

    pub(crate) type Answer = Box<dyn Fn(&str, &str) -> (u16, String) + Send>;

    /// Every request, as `method?query` (or `method?body` for a post).
    pub(crate) type Seen = Arc<Mutex<Vec<String>>>;

    pub(crate) fn slack(answer: Answer) -> (String, Seen) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let base = format!("http://127.0.0.1:{}/api", listener.local_addr().unwrap().port());
        let seen = Arc::new(Mutex::new(Vec::new()));
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
                    if lower.starts_with("authorization: bearer xoxp-") {
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
                let method = path.rsplit('/').next().unwrap_or("").to_string();
                // What was posted stands for the query.
                let query =
                    if sent.is_empty() { query.to_string() } else { String::from_utf8_lossy(&sent).into_owned() };
                kept.lock().unwrap().push(format!("{method}?{query}"));
                let (status, body) = if authorized {
                    answer(&method, &query)
                } else {
                    (200, r#"{"ok":false,"error":"not_authed"}"#.into())
                };
                // Status 0: the connection is dropped without an answer.
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

    /// A workspace with one direct message with Ada, and one group message.
    pub(crate) fn acme(round: Arc<std::sync::atomic::AtomicUsize>) -> Answer {
        Box::new(move |method, query| {
            let round = round.load(std::sync::atomic::Ordering::SeqCst);
            let body = match method {
                "auth.test" => r#"{"ok":true,"team_id":"T1","team":"Acme","user_id":"U1","user":"c"}"#.to_string(),
                "users.conversations" => r#"{"ok":true,"channels":[{"id":"D1","is_im":true,"user":"U2"},{"id":"G1","is_mpim":true}],"response_metadata":{"next_cursor":""}}"#.to_string(),
                "users.info" if query.contains("user=U1") => r#"{"ok":true,"user":{"id":"U1","name":"c","profile":{"display_name":"C","email":"c@example.com"}}}"#.to_string(),
                "users.info" if query.contains("user=U2") => r#"{"ok":true,"user":{"id":"U2","name":"ada","profile":{"display_name":"","real_name":"Ada Lovelace","email":"ada@example.com"}}}"#.to_string(),
                "users.info" => r#"{"ok":true,"user":{"id":"U3","name":"bob","profile":{"display_name":"Bob"}}}"#.to_string(),
                "conversations.history" if query.contains("channel=D1") && round == 0 => r#"{"ok":true,"has_more":false,"messages":[
                    {"type":"message","user":"U2","text":"lunch thursday, <@U1>?","ts":"1770110400.000200"},
                    {"type":"message","user":"U1","text":"yes!","ts":"1770110040.000100"},
                    {"type":"message","subtype":"channel_join","user":"U2","text":"joined","ts":"1770110000.000100"},
                    {"type":"message","bot_id":"B1","text":"reminder","ts":"1770110300.000100"}]}"#.to_string(),
                "conversations.history" if query.contains("channel=D1") && round == 2 => r#"{"ok":true,"has_more":false,"messages":[
                    {"type":"message","user":"U1","text":"Yes &lt;noon&gt; &amp; thanks","ts":"1770112000.000100"}]}"#.to_string(),
                "chat.postMessage" => r#"{"ok":true,"channel":"D1","ts":"1770112000.000100"}"#.to_string(),
                "conversations.history" if query.contains("channel=D1") => {
                    assert!(query.contains("oldest=1770110400.000200"), "{query}");
                    r#"{"ok":true,"has_more":false,"messages":[{"type":"message","user":"U2","text":"noon?","ts":"1770111000.000100"}]}"#.to_string()
                }
                "conversations.history" if round == 0 => r#"{"ok":true,"has_more":false,"messages":[
                    {"type":"message","user":"U3","text":"dinner sunday","ts":"1770111100.000100"}]}"#.to_string(),
                "conversations.history" => r#"{"ok":true,"has_more":false,"messages":[]}"#.to_string(),
                _ => r#"{"ok":false,"error":"unknown_method"}"#.to_string(),
            };
            (200, body)
        })
    }

    /// A conversation in `live`: a direct message (with `other`) or a group.
    pub(crate) struct Room {
        pub id: String,
        pub other: Option<String>,
        /// Oldest first, as `{ "user", "text", "ts" }`.
        pub messages: Vec<serde_json::Value>,
    }

    pub(crate) type Rooms = Arc<Mutex<Vec<Room>>>;

    fn param(query: &str, key: &str) -> Option<String> {
        query.split('&').find_map(|kv| kv.strip_prefix(&format!("{key}="))).map(str::to_string)
    }

    fn ts_of(m: &serde_json::Value) -> f64 {
        m["ts"].as_str().and_then(|t| t.parse().ok()).unwrap_or(0.0)
    }

    /// A Slack that keeps what it is told: Acme's people, `rooms` as they
    /// stand, history after `oldest`, newest first, and a post added to its
    /// room as the user's, a second after its last message.
    pub(crate) fn live(rooms: Rooms) -> Answer {
        Box::new(move |method, query| {
            let mut rooms = rooms.lock().unwrap();
            let body = match method {
                "auth.test" => {
                    serde_json::json!({ "ok": true, "team_id": "T1", "team": "Acme", "user_id": "U1", "user": "c" })
                }
                "users.conversations" => serde_json::json!({
                    "ok": true,
                    "channels": rooms.iter().map(|r| match &r.other {
                        Some(u) => serde_json::json!({ "id": r.id, "is_im": true, "user": u }),
                        None => serde_json::json!({ "id": r.id, "is_mpim": true }),
                    }).collect::<Vec<_>>(),
                }),
                "users.info" => {
                    let id = param(query, "user").unwrap_or_default();
                    let name = match id.as_str() {
                        "U1" => "C",
                        "U2" => "Ada Lovelace",
                        _ => "Bob",
                    };
                    serde_json::json!({ "ok": true, "user": { "id": id, "profile": { "display_name": name } } })
                }
                "conversations.history" => {
                    let channel = param(query, "channel").unwrap_or_default();
                    let oldest: f64 = param(query, "oldest").and_then(|t| t.parse().ok()).unwrap_or(0.0);
                    let latest: f64 = param(query, "latest").and_then(|t| t.parse().ok()).unwrap_or(f64::MAX);
                    let inclusive = param(query, "inclusive").as_deref() == Some("true");
                    let mut found: Vec<serde_json::Value> = rooms
                        .iter()
                        .filter(|r| r.id == channel)
                        .flat_map(|r| {
                            r.messages
                                .iter()
                                .filter(|m| {
                                    ts_of(m) > oldest && (ts_of(m) < latest || (inclusive && ts_of(m) == latest))
                                })
                                .cloned()
                        })
                        .collect();
                    found.reverse();
                    serde_json::json!({ "ok": true, "has_more": false, "messages": found })
                }
                "chat.postMessage" => {
                    let sent: serde_json::Value = serde_json::from_str(query).unwrap();
                    let channel = sent["channel"].as_str().unwrap_or_default().to_string();
                    let Some(room) = rooms.iter_mut().find(|r| r.id == channel) else {
                        return (200, r#"{"ok":false,"error":"channel_not_found"}"#.into());
                    };
                    let last = room.messages.last().map(ts_of).unwrap_or(0.0);
                    let ts = format!("{:.0}.000100", last.floor() + 1.0);
                    room.messages
                        .push(serde_json::json!({ "type": "message", "user": "U1", "text": sent["text"], "ts": ts }));
                    serde_json::json!({ "ok": true, "channel": channel, "ts": ts })
                }
                _ => serde_json::json!({ "ok": false, "error": "unknown_method" }),
            };
            (200, body.to_string())
        })
    }

    pub(crate) fn workspace(db: &Db) -> String {
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::AccountId, &account_id("T1", "U1")).unwrap();
        db.set_setting(crate::db::WITHIN_DAYS_SETTING, &0).unwrap();
        let me = Identity { team_id: "T1".into(), team: "Acme".into(), user_id: "U1".into(), user: "c".into() };
        let source = db.create_source(&new_source(&me).unwrap()).unwrap();
        assert_eq!(connected_as(db, &me).unwrap().as_deref(), Some(source.id.as_str()));
        assert_eq!(connected_as(db, &Identity { user_id: "U2".into(), ..me }).unwrap(), None);
        source.id
    }
}

#[cfg(test)]
mod tests {
    use super::fake::TOKEN;
    use super::*;

    #[test]
    fn a_user_token_is_told_from_a_bots() {
        assert!(is_user_token(TOKEN));
        assert!(!is_user_token("xoxb-1111111111-2222222222-abcdefabcdef"), "a bot's");
        assert!(!is_user_token("xoxp-short"));
        assert!(!is_user_token("xoxp-1111111111 2222222222-abcdef"));
    }

    #[test]
    fn slacks_markup_is_read_as_people_read_it() {
        let names = |id: &str| if id == "U2" { "Ada".to_string() } else { id.to_string() };
        assert_eq!(
            readable("hey <@U2>, see <#C1|general> and <https://x.example/a?b=1&amp;c=2|the doc> <!here>", &names),
            "hey @Ada, see #general and the doc @here"
        );
        assert_eq!(
            readable("1 &lt; 2 &amp;&amp; 3 &gt; 2 <https://x.example>", &names),
            "1 < 2 && 3 > 2 https://x.example"
        );
        assert_eq!(readable("a < b", &names), "a < b", "a bare < is itself");
        assert_eq!(mentioned("hi <@U2> and <@U3|bob>"), ["U2", "U3"]);
    }

    #[test]
    fn connecting_asks_for_one_of_each_thing_a_check_does() {
        let round = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (base, seen) = fake::slack(fake::acme(round));
        let me = SlackApi::at(&base, TOKEN).unwrap().check().unwrap();
        assert_eq!((me.team.as_str(), me.user_id.as_str()), ("Acme", "U1"));
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 4, "{seen:?}");
        assert!(seen[1].starts_with("users.conversations?") && seen[1].contains("limit=1"), "{seen:?}");
        assert!(seen[2].starts_with("conversations.history?channel=D1&limit=1"), "{seen:?}");
        assert!(seen[3].starts_with("users.info?user=U1"), "{seen:?}");
    }

    #[test]
    fn a_check_reads_both_sides_then_only_whats_new() {
        let round = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (base, seen) = fake::slack(fake::acme(round.clone()));
        let db = Db::open_in_memory().unwrap();
        let source = fake::workspace(&db);
        let api = SlackApi::at(&base, TOKEN).unwrap();

        let first = sync(&db, &source, &api, &|| false).unwrap();
        assert_eq!((first.conversations, first.inserted, first.from_self, first.left_out), (2, 3, 1, 2));
        assert!(!first.partial);
        let waiting: Vec<String> = db.threads_awaiting_reply(10).unwrap().into_iter().map(|t| t.last_message).collect();
        assert!(waiting.contains(&"lunch thursday, @C?".to_string()), "{waiting:?}");
        assert!(waiting.contains(&"dinner sunday".to_string()), "{waiting:?}");
        let config: SlackSourceConfig =
            serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        assert_eq!(config.marks.get("D1").map(String::as_str), Some("1770110400.000200"));
        assert!(config.last_read_at.is_some());
        assert!(seen.lock().unwrap().iter().all(|r| !r.contains("xoxp")), "the token never in a query");

        // The next check asks only for what came after.
        round.store(1, std::sync::atomic::Ordering::SeqCst);
        let next = sync(&db, &source, &api, &|| false).unwrap();
        assert_eq!((next.inserted, next.duplicates), (1, 0));
        // The people: Ada once, with her email, and the user not a person.
        let people = db.list_participants(10).unwrap();
        let names: Vec<&str> = people.iter().map(|p| p.participant.display_name.as_str()).collect();
        assert!(names.contains(&"Ada Lovelace") && names.contains(&"Bob") && !names.contains(&"C"), "{names:?}");
    }

    #[test]
    fn a_turned_down_token_and_a_missing_scope_say_what_to_do() {
        let (base, _) = fake::slack(Box::new(|method, _| match method {
            "auth.test" => (200, r#"{"ok":true,"team_id":"T1","team":"Acme","user_id":"U1","user":"c"}"#.into()),
            _ => (200, r#"{"ok":false,"error":"missing_scope","needed":"im:history","provided":"users:read"}"#.into()),
        }));
        let db = Db::open_in_memory().unwrap();
        let source = fake::workspace(&db);
        let err = sync(&db, &source, &SlackApi::at(&base, TOKEN).unwrap(), &|| false).unwrap_err();
        assert!(matches!(&err, SlackError::Scope { scope, .. } if scope == "im:history"), "{err:?}");
        assert!(err.to_string().contains("add the im:history scope"), "{err}");
        assert_eq!(db.get_source(&source).unwrap().unwrap().status, "failed");

        let (base, _) = fake::slack(Box::new(|_, _| (200, r#"{"ok":false,"error":"token_revoked"}"#.into())));
        let err = SlackApi::at(&base, TOKEN).unwrap().auth_test().unwrap_err();
        assert!(matches!(err, SlackError::Token(_)), "{err:?}");
    }

    #[test]
    fn a_workspace_that_gives_no_conversation_fails_the_check() {
        let (base, _) = fake::slack(Box::new(|method, _| match method {
            "auth.test" => (200, r#"{"ok":true,"team_id":"T1","team":"Acme","user_id":"U1","user":"c"}"#.into()),
            "users.conversations" => {
                (200, r#"{"ok":true,"channels":[{"id":"D1","user":"U2"},{"id":"D2","user":"U3"}]}"#.into())
            }
            _ => (200, r#"{"ok":false,"error":"access_denied"}"#.into()),
        }));
        let db = Db::open_in_memory().unwrap();
        let source = fake::workspace(&db);
        let err = sync(&db, &source, &SlackApi::at(&base, TOKEN).unwrap(), &|| false).unwrap_err();
        assert!(err.to_string().contains("gave none of your conversations (access_denied)"), "{err}");
        assert_eq!(err.to_string().matches("Slack said no").count(), 1, "{err}");
        assert_eq!(db.get_source(&source).unwrap().unwrap().status, "failed");
    }

    #[test]
    fn a_conversation_or_a_person_slack_wont_give_is_passed_over() {
        let (base, _) = fake::slack(Box::new(|method, query| {
            match method {
            "auth.test" => (200, r#"{"ok":true,"team_id":"T1","team":"Acme","user_id":"U1","user":"c"}"#.into()),
            "users.conversations" => (200, r#"{"ok":true,"channels":[{"id":"D1","user":"U2"},{"id":"D2","user":"U3"}]}"#.into()),
            "conversations.history" if query.contains("channel=D1") => (200, r#"{"ok":false,"error":"channel_not_found"}"#.into()),
            "conversations.history" => (
                200,
                r#"{"ok":true,"messages":[{"type":"message","user":"U3","text":"still there?","ts":"1770110400.000200"}]}"#.into(),
            ),
            _ => (200, r#"{"ok":false,"error":"user_not_found"}"#.into()),
        }
        }));
        let db = Db::open_in_memory().unwrap();
        let source = fake::workspace(&db);
        let summary = sync(&db, &source, &SlackApi::at(&base, TOKEN).unwrap(), &|| false).unwrap();
        assert_eq!((summary.inserted, summary.unreadable, summary.partial), (1, 1, false));
        let people = db.list_participants(10).unwrap();
        assert!(people.iter().any(|p| p.participant.display_name == "U3"), "known by their id");
        assert_eq!(db.get_source(&source).unwrap().unwrap().status, "imported");
    }

    #[test]
    fn asked_to_wait_a_moment_it_waits_and_reads_on() {
        let asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = asked.clone();
        let round = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let acme = fake::acme(round);
        let (base, _) = fake::slack(Box::new(move |method, query| {
            if method == "conversations.history" && count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                return (429, r#"wait:1|{"ok":false,"error":"ratelimited"}"#.into());
            }
            acme(method, query)
        }));
        let db = Db::open_in_memory().unwrap();
        let source = fake::workspace(&db);
        let summary = sync(&db, &source, &SlackApi::at(&base, TOKEN).unwrap(), &|| false).unwrap();
        assert!(!summary.partial, "{summary:?}");
        assert_eq!(summary.inserted, 3);
    }

    #[test]
    fn slowing_down_keeps_what_was_read() {
        let (base, _) = fake::slack(Box::new(|method, query| match method {
            "auth.test" => (200, r#"{"ok":true,"team_id":"T1","team":"Acme","user_id":"U1","user":"c"}"#.into()),
            "users.conversations" => {
                (200, r#"{"ok":true,"channels":[{"id":"D1","user":"U2"},{"id":"D2","user":"U3"}]}"#.into())
            }
            "users.info" => (200, r#"{"ok":true,"user":{"id":"U2","name":"ada"}}"#.into()),
            "conversations.history" if query.contains("channel=D1") => (
                200,
                r#"{"ok":true,"messages":[{"type":"message","user":"U2","text":"hi","ts":"1770110400.000200"}]}"#
                    .into(),
            ),
            _ => (429, r#"{"ok":false,"error":"ratelimited"}"#.into()),
        }));
        let db = Db::open_in_memory().unwrap();
        let source = fake::workspace(&db);
        let summary = sync(&db, &source, &SlackApi::at(&base, TOKEN).unwrap(), &|| false).unwrap();
        assert!(summary.partial);
        assert_eq!(summary.inserted, 1);
        let config: SlackSourceConfig =
            serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        assert!(config.marks.contains_key("D1") && !config.marks.contains_key("D2"));
        assert!(config.last_read_at.is_none(), "not read through");
    }
}
