//! A connected mailbox: IMAP, so mail arrives without an export.
//!
//! What it does:
//!
//! * Logs in with a username and an app password over implicit TLS (port
//!   993), verified against the bundled Mozilla root store. Plain text is
//!   refused for anything but this computer (`127.0.0.1`, `::1`,
//!   `localhost`), which is where local bridges such as Proton Mail Bridge
//!   listen and where the tests run their server.
//! * Reads the inbox **and the sent folder**. Sent mail is the point: it is
//!   the only place the user's own writing is, and without it a connected
//!   mailbox teaches nothing. The sent folder is found by its RFC 6154
//!   `\Sent` attribute, falling back to the names the common servers use.
//! * Remembers, per folder, the server's `UIDVALIDITY` and the highest UID it
//!   has imported, and asks only for what is newer. A changed `UIDVALIDITY`
//!   means the server renumbered the folder, and the folder is re-read;
//!   nothing is duplicated, because a message's id is its `Message-ID`.
//! * On the first check of a folder, takes at most the newest
//!   `FIRST_SYNC_LIMIT` messages, so connecting a twenty-year-old mailbox is
//!   minutes rather than hours. Older mail can still be imported from an
//!   export.
//! * Fetches with `BODY.PEEK[]`, which does not mark anything as read.
//!   Nothing is ever written to the mailbox, and there is no command in this
//!   module that could.
//!
//! What it does not: OAuth. Gmail, iCloud, Fastmail and Yahoo accept an app
//! password for IMAP. Outlook.com and Hotmail stopped accepting passwords of
//! any kind for IMAP in September 2024, and a work or school account whose
//! admin allows only single sign-on is the same; none of those can be
//! connected from here, and the connect dialog says so rather than letting a
//! login fail mysteriously.
//!
//! The client is a small IMAP4rev1 subset written against `Read + Write`, so
//! the tests drive the real protocol code against a scripted server on a
//! loopback socket.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::mbox::{parse_raw, thread, Mail};
use crate::db::{Db, DbError};
use crate::import::{ImportError, Importer};

pub const CONNECTOR: &str = "imap";
/// Messages read from a folder the first time it is checked.
pub const FIRST_SYNC_LIMIT: usize = 2000;
/// Messages fetched per round trip.
const CHUNK: usize = 50;
/// Bytes fetched per round trip, whatever the count: fifty newsletters with
/// images are not fifty lines of text.
const CHUNK_BYTES: u64 = 20 * 1024 * 1024;
/// Everything one command's reply may hold. A server that sends more is not
/// answering the question that was asked.
const MAX_COMMAND_BYTES: usize = 64 * 1024 * 1024;
/// Longest a single read from the server may take.
const TIMEOUT: Duration = Duration::from_secs(60);
/// Largest single message Mimic will read. Bigger ones are almost always
/// attachments; they are skipped by size before being fetched, and the
/// position moves past them, so one of them cannot stall every check.
const MAX_MESSAGE_BYTES: usize = 25 * 1024 * 1024;

const SENT_NAMES: [&str; 6] = ["sent", "sent items", "sent messages", "sent mail", "[gmail]/sent mail", "inbox.sent"];

/// How to reach the server. Stored in `sources.config`; the password is not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImapAccount {
    pub host: String,
    pub port: u16,
    pub username: String,
    /// `tls` (implicit, port 993) or `plain` (this computer only).
    pub security: Security,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Security {
    Tls,
    Plain,
}

/// Where a folder was left.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderMark {
    pub uid_validity: u32,
    pub last_uid: u32,
}

/// `sources.config` for an IMAP source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImapSourceConfig {
    pub account: ImapAccount,
    /// The folders read, found on the first connection.
    #[serde(default)]
    pub folders: Vec<String>,
    #[serde(default)]
    pub sent_folder: Option<String>,
    #[serde(default)]
    pub marks: BTreeMap<String, FolderMark>,
    #[serde(default)]
    pub last_checked_at: Option<String>,
}

/// The secret-store key a source's password lives under.
pub fn secret_key(source_id: &str) -> String {
    format!("imap:{source_id}")
}

#[derive(Debug, thiserror::Error)]
pub enum ImapError {
    #[error("Couldn't reach {0}. Check the server name and port, and that you're online.")]
    Connect(String),
    #[error("The secure connection to {0} failed: {1}")]
    Tls(String, String),
    #[error("The server turned down that username and password. Most providers want an app password here, not your usual one. ({0})")]
    Login(String),
    #[error("A plain, unencrypted connection is only allowed to this computer. Use TLS for {0}.")]
    PlainRefused(String),
    #[error("The server said no: {0}")]
    Refused(String),
    #[error("The server's reply didn't make sense: {0}")]
    Protocol(String),
    #[error("The connection dropped: {0}")]
    Io(String),
    #[error("{0}")]
    Import(String),
    #[error("canceled")]
    Canceled,
}

impl From<std::io::Error> for ImapError {
    fn from(e: std::io::Error) -> Self {
        ImapError::Io(e.to_string())
    }
}

impl From<DbError> for ImapError {
    fn from(e: DbError) -> Self {
        ImapError::Import(e.to_string())
    }
}

impl From<ImportError> for ImapError {
    fn from(e: ImportError) -> Self {
        ImapError::Import(e.to_string())
    }
}

/// A server's reply text, cut to one short line. Server text goes into error
/// messages, and must not be able to carry anything long or multi-line.
fn one_line(s: &str) -> String {
    let line = s.lines().next().unwrap_or_default().trim();
    let mut out: String = line.chars().take(160).collect();
    if line.chars().count() > 160 {
        out.push('…');
    }
    out
}

/// `localhost`, or a loopback IP address. Decided on the address itself, not
/// on how a name starts: "127.0.0.1.example.com" is a name, and can point
/// anywhere.
pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim().trim_start_matches('[').trim_end_matches(']');
    h.eq_ignore_ascii_case("localhost") || h.parse::<std::net::IpAddr>().map(|ip| ip.is_loopback()).unwrap_or(false)
}

// ------------------------------------------------------------------ protocol

/// One untagged or tagged response line, with any literals it carried.
#[derive(Debug, Clone, Default)]
struct Line {
    /// The line with each literal replaced by `{#n}`.
    text: String,
    literals: Vec<Vec<u8>>,
}

pub struct Session<S: Read + Write> {
    stream: S,
    buf: Vec<u8>,
    tag: u32,
}

impl<S: Read + Write> Session<S> {
    /// Wrap a connected stream and read the server's greeting.
    pub fn start(stream: S) -> Result<Self, ImapError> {
        let mut s = Session { stream, buf: Vec::new(), tag: 0 };
        let greeting = s.read_line()?;
        let t = greeting.text.to_ascii_uppercase();
        if t.starts_with("* BYE") {
            return Err(ImapError::Refused(one_line(&greeting.text)));
        }
        if !t.starts_with("* OK") && !t.starts_with("* PREAUTH") {
            return Err(ImapError::Protocol(one_line(&greeting.text)));
        }
        Ok(s)
    }

    fn fill(&mut self) -> Result<(), ImapError> {
        let mut chunk = [0u8; 16 * 1024];
        let n = self.stream.read(&mut chunk)?;
        if n == 0 {
            return Err(ImapError::Io("the server closed the connection".into()));
        }
        self.buf.extend_from_slice(&chunk[..n]);
        Ok(())
    }

    /// One logical response line: CRLF-terminated text, with `{n}` literals
    /// read in place.
    fn read_line(&mut self) -> Result<Line, ImapError> {
        let mut line = Line::default();
        loop {
            let pos = loop {
                if let Some(p) = self.buf.windows(2).position(|w| w == b"\r\n") {
                    break p;
                }
                if self.buf.len() > MAX_MESSAGE_BYTES {
                    return Err(ImapError::Protocol("a response line with no end".into()));
                }
                self.fill()?;
            };
            let raw: Vec<u8> = self.buf.drain(..pos + 2).collect();
            let text = String::from_utf8_lossy(&raw[..pos]).into_owned();
            // A literal: "... {1234}" at the end of the line, then 1234 bytes.
            let literal_len = text
                .strip_suffix('}')
                .and_then(|t| t.rfind('{').map(|i| &t[i + 1..]))
                .and_then(|n| n.trim_end_matches('+').parse::<usize>().ok());
            match literal_len {
                Some(n) => {
                    let brace = text.rfind('{').unwrap_or(text.len());
                    line.text.push_str(&text[..brace]);
                    line.text.push_str(&format!("{{#{}}}", line.literals.len()));
                    if n > MAX_MESSAGE_BYTES {
                        // Bigger than was promised (a server's size can be an
                        // estimate). Read past it without keeping it, so this
                        // one message is skipped rather than failing the check
                        // — and every check after it.
                        let mut left = n;
                        while left > 0 {
                            if self.buf.is_empty() {
                                self.fill()?;
                            }
                            let take = left.min(self.buf.len());
                            self.buf.drain(..take);
                            left -= take;
                        }
                        line.literals.push(Vec::new());
                        continue;
                    }
                    while self.buf.len() < n {
                        self.fill()?;
                    }
                    line.literals.push(self.buf.drain(..n).collect());
                }
                None => {
                    line.text.push_str(&text);
                    return Ok(line);
                }
            }
        }
    }

    /// Send a command and collect every untagged line until its tagged reply.
    fn command(&mut self, cmd: &str) -> Result<Vec<Line>, ImapError> {
        self.tag += 1;
        let tag = format!("m{}", self.tag);
        self.stream.write_all(format!("{tag} {cmd}\r\n").as_bytes())?;
        self.stream.flush()?;
        let mut untagged = Vec::new();
        let mut held = 0usize;
        loop {
            let line = self.read_line()?;
            held += line.text.len() + line.literals.iter().map(Vec::len).sum::<usize>();
            if held > MAX_COMMAND_BYTES {
                return Err(ImapError::Protocol("the server sent far more than was asked for".into()));
            }
            if let Some(rest) = line.text.strip_prefix(&format!("{tag} ")) {
                let upper = rest.to_ascii_uppercase();
                if upper.starts_with("OK") {
                    return Ok(untagged);
                }
                // "NO [CODE] human text": keep the human text.
                let mut reason = rest.split_once(' ').map(|(_, r)| r.trim()).unwrap_or_default();
                if reason.starts_with('[') {
                    reason = reason.split_once(']').map(|(_, r)| r.trim()).unwrap_or(reason);
                }
                return Err(ImapError::Refused(one_line(if reason.is_empty() { rest } else { reason })));
            }
            untagged.push(line);
        }
    }

    pub fn login(&mut self, username: &str, password: &str) -> Result<(), ImapError> {
        let (u, p) = (quote(username)?, quote(password)?);
        self.command(&format!("LOGIN {u} {p}")).map(|_| ()).map_err(|e| match e {
            ImapError::Refused(why) => ImapError::Login(why),
            other => other,
        })
    }

    /// Every folder with its attributes: `(attributes, name)`.
    pub fn list(&mut self) -> Result<Vec<(Vec<String>, String)>, ImapError> {
        let lines = self.command("LIST \"\" \"*\"")?;
        let mut out = Vec::new();
        for l in lines {
            let Some(rest) = l.text.strip_prefix("* LIST ") else { continue };
            let Some(close) = rest.find(')') else { continue };
            let attrs: Vec<String> = rest[1..close].split_whitespace().map(|a| a.to_ascii_lowercase()).collect();
            // After the attributes: the delimiter (quoted or NIL), then the name.
            let after = rest[close + 1..].trim_start();
            let after = if let Some(stripped) = after.strip_prefix("NIL") { stripped } else { skip_quoted(after) };
            let name = match after.trim() {
                n if n.starts_with("{#") => {
                    let idx: usize = n.trim_start_matches("{#").trim_end_matches('}').parse().unwrap_or(0);
                    String::from_utf8_lossy(l.literals.get(idx).map(|v| v.as_slice()).unwrap_or_default()).into_owned()
                }
                n => unquote(n),
            };
            out.push((attrs, name));
        }
        Ok(out)
    }

    /// Open a folder read-only: `EXAMINE`, so not even the `\Recent` flags
    /// change. Returns its `UIDVALIDITY` and message count.
    pub fn examine(&mut self, folder: &str) -> Result<(u32, u32), ImapError> {
        let lines = self.command(&format!("EXAMINE {}", quote(folder)?))?;
        let mut validity = 0;
        let mut exists = 0;
        for l in &lines {
            let t = l.text.to_ascii_uppercase();
            if let Some(i) = t.find("UIDVALIDITY ") {
                validity = t[i + 12..].split(|c: char| !c.is_ascii_digit()).next().unwrap_or("0").parse().unwrap_or(0);
            }
            if t.ends_with(" EXISTS") {
                exists = t.trim_start_matches("* ").split_whitespace().next().unwrap_or("0").parse().unwrap_or(0);
            }
        }
        Ok((validity, exists))
    }

    /// UIDs strictly greater than `after`, ascending.
    pub fn uids_after(&mut self, after: u32) -> Result<Vec<u32>, ImapError> {
        let lines = self.command(&format!("UID SEARCH UID {}:*", after.saturating_add(1)))?;
        let mut uids: Vec<u32> = lines
            .iter()
            .filter_map(|l| l.text.strip_prefix("* SEARCH"))
            .flat_map(|r| r.split_whitespace().filter_map(|n| n.parse::<u32>().ok()).collect::<Vec<_>>())
            // "n:*" always matches the last message, even when its UID is
            // below n. Only what is actually newer counts.
            .filter(|&u| u > after)
            .collect();
        uids.sort_unstable();
        uids.dedup();
        Ok(uids)
    }

    /// Each message's size, so the oversized can be skipped before fetching.
    pub fn sizes(&mut self, uids: &[u32]) -> Result<Vec<(u32, u64)>, ImapError> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let set = uids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        let lines = self.command(&format!("UID FETCH {set} (UID RFC822.SIZE)"))?;
        let mut out = Vec::new();
        for l in lines {
            let upper = l.text.to_ascii_uppercase();
            let number_after = |key: &str| {
                upper.find(key).and_then(|i| {
                    upper[i + key.len()..].trim_start().split(|c: char| !c.is_ascii_digit()).next()?.parse::<u64>().ok()
                })
            };
            if let (Some(uid), Some(size)) = (number_after("UID "), number_after("RFC822.SIZE ")) {
                out.push((uid as u32, size));
            }
        }
        Ok(out)
    }

    /// Full messages by UID, without setting `\Seen`.
    pub fn fetch(&mut self, uids: &[u32]) -> Result<Vec<(u32, Vec<u8>)>, ImapError> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let set = uids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        let lines = self.command(&format!("UID FETCH {set} (UID BODY.PEEK[])"))?;
        let mut out = Vec::new();
        for l in lines {
            if !l.text.starts_with("* ") || !l.text.to_ascii_uppercase().contains(" FETCH ") {
                continue;
            }
            let upper = l.text.to_ascii_uppercase();
            let uid = upper
                .find("UID ")
                .and_then(|i| upper[i + 4..].split(|c: char| !c.is_ascii_digit()).next().and_then(|n| n.parse().ok()));
            let body = upper.find("BODY[]").and_then(|_| l.literals.first().cloned());
            if let (Some(uid), Some(body)) = (uid, body) {
                out.push((uid, body));
            }
        }
        Ok(out)
    }

    pub fn logout(&mut self) {
        let _ = self.command("LOGOUT");
    }
}

/// An IMAP quoted string. CR and LF cannot appear in one, and a password
/// containing them is refused rather than allowed to end the command early.
fn quote(s: &str) -> Result<String, ImapError> {
    if s.contains(['\r', '\n']) {
        return Err(ImapError::Protocol(
            "a line break cannot be sent as part of a username, password or folder".into(),
        ));
    }
    Ok(format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
}

fn unquote(s: &str) -> String {
    let t = s.trim();
    match t.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        Some(inner) => inner.replace("\\\"", "\"").replace("\\\\", "\\"),
        None => t.to_string(),
    }
}

/// Skip one quoted string at the start of `s`.
fn skip_quoted(s: &str) -> &str {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'"') {
        return s.split_once(' ').map(|(_, r)| r).unwrap_or("");
    }
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return &s[i + 1..],
            _ => i += 1,
        }
    }
    ""
}

/// Pick the folders to read: the inbox, and the sent folder by attribute
/// first, by name second.
pub fn choose_folders(listed: &[(Vec<String>, String)]) -> (Vec<String>, Option<String>) {
    let inbox = listed
        .iter()
        .find(|(_, n)| n.eq_ignore_ascii_case("INBOX"))
        .map(|(_, n)| n.clone())
        .unwrap_or_else(|| "INBOX".to_string());
    let selectable = |attrs: &Vec<String>| !attrs.iter().any(|a| a == "\\noselect" || a == "\\nonexistent");
    let sent = listed
        .iter()
        .find(|(a, _)| a.iter().any(|x| x == "\\sent") && selectable(a))
        .or_else(|| {
            SENT_NAMES.iter().find_map(|want| listed.iter().find(|(a, n)| n.to_lowercase() == *want && selectable(a)))
        })
        .map(|(_, n)| n.clone());
    let mut folders = vec![inbox];
    if let Some(s) = &sent {
        folders.push(s.clone());
    }
    (folders, sent)
}

// --------------------------------------------------------------- connecting

/// A connected, logged-in session over TLS or (to this computer) plain TCP.
pub enum Connection {
    Tls(Box<Session<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>>),
    Plain(Box<Session<TcpStream>>),
}

macro_rules! with_session {
    ($conn:expr, $s:ident => $body:expr) => {
        match $conn {
            Connection::Tls($s) => $body,
            Connection::Plain($s) => $body,
        }
    };
}

impl Connection {
    pub fn open(account: &ImapAccount, password: &str) -> Result<Connection, ImapError> {
        // Refuse before connecting: a password must never be offered in the
        // clear to anything but this computer.
        if account.security == Security::Plain && !is_loopback_host(&account.host) {
            return Err(ImapError::PlainRefused(account.host.clone()));
        }
        let addr = format!("{}:{}", account.host.trim(), account.port);
        let tcp = TcpStream::connect(&addr).map_err(|_| ImapError::Connect(addr.clone()))?;
        tcp.set_read_timeout(Some(TIMEOUT))?;
        tcp.set_write_timeout(Some(TIMEOUT))?;
        let mut conn = match account.security {
            Security::Plain => Connection::Plain(Box::new(Session::start(tcp)?)),
            Security::Tls => {
                let mut roots = rustls::RootCertStore::empty();
                roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
                let config =
                    rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                        .with_safe_default_protocol_versions()
                        .map_err(|e| ImapError::Tls(account.host.clone(), e.to_string()))?
                        .with_root_certificates(roots)
                        .with_no_client_auth();
                let name = rustls::pki_types::ServerName::try_from(account.host.trim().to_string())
                    .map_err(|e| ImapError::Tls(account.host.clone(), e.to_string()))?;
                let client = rustls::ClientConnection::new(Arc::new(config), name)
                    .map_err(|e| ImapError::Tls(account.host.clone(), e.to_string()))?;
                let stream = rustls::StreamOwned::new(client, tcp);
                Connection::Tls(Box::new(Session::start(stream).map_err(|e| match e {
                    ImapError::Io(why) => ImapError::Tls(account.host.clone(), why),
                    other => other,
                })?))
            }
        };
        with_session!(&mut conn, s => s.login(&account.username, password))?;
        Ok(conn)
    }

    pub fn list(&mut self) -> Result<Vec<(Vec<String>, String)>, ImapError> {
        with_session!(self, s => s.list())
    }
    pub fn examine(&mut self, folder: &str) -> Result<(u32, u32), ImapError> {
        with_session!(self, s => s.examine(folder))
    }
    pub fn uids_after(&mut self, after: u32) -> Result<Vec<u32>, ImapError> {
        with_session!(self, s => s.uids_after(after))
    }
    pub fn fetch(&mut self, uids: &[u32]) -> Result<Vec<(u32, Vec<u8>)>, ImapError> {
        with_session!(self, s => s.fetch(uids))
    }
    pub fn sizes(&mut self, uids: &[u32]) -> Result<Vec<(u32, u64)>, ImapError> {
        with_session!(self, s => s.sizes(uids))
    }
    pub fn logout(&mut self) {
        with_session!(self, s => s.logout())
    }
}

/// What connecting found, before anything is imported: shown on the screen
/// that adds the mailbox, so a missing sent folder is known up front.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImapProbe {
    pub folders: Vec<String>,
    pub sent_folder: Option<String>,
    /// Messages in each folder that will be read.
    pub counts: Vec<(String, u32)>,
    pub warnings: Vec<String>,
}

pub fn probe(account: &ImapAccount, password: &str) -> Result<ImapProbe, ImapError> {
    let mut conn = Connection::open(account, password)?;
    let listed = conn.list()?;
    let (folders, sent_folder) = choose_folders(&listed);
    let mut counts = Vec::new();
    for f in &folders {
        let (_, exists) = conn.examine(f)?;
        counts.push((f.clone(), exists));
    }
    conn.logout();
    let mut warnings = Vec::new();
    if sent_folder.is_none() {
        warnings.push(
            "I couldn't find a sent folder. Your inbox will come in, but what you wrote is what I learn from, so I'll have little to go on.".into(),
        );
    }
    if counts.iter().any(|(_, n)| *n as usize > FIRST_SYNC_LIMIT) {
        warnings.push(format!(
            "I'll read the newest {FIRST_SYNC_LIMIT} messages in each folder the first time. For everything older, add an export of your mail as well — a message in both is only counted once."
        ));
    }
    Ok(ImapProbe { folders, sent_folder, counts, warnings })
}

// --------------------------------------------------------------------- sync

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSummary {
    pub folders: Vec<(String, usize)>,
    pub fetched: usize,
    /// Fetched messages with no text in them (a calendar invite, a picture).
    pub without_text: usize,
    /// Messages over the size limit, skipped without being kept.
    pub too_large: usize,
    /// Messages that could not be parsed at all, skipped.
    pub unreadable: usize,
    pub inserted: usize,
    pub duplicates: usize,
    pub from_self: usize,
    /// UIDVALIDITY changed and the folder was read again from the start.
    pub renumbered: Vec<String>,
}

/// Check a mailbox for new mail and import it.
///
/// Every folder's new mail is fetched first and threaded together before
/// anything is imported, so the order folders are read in cannot split a
/// thread (the user's message in Sent and the reply to it in the inbox, new
/// in the same check, are one conversation). The position only moves once
/// the import is done: a check that is canceled or drops its connection
/// part-way imports nothing and loses nothing, and the next one reads the
/// same mail again.
pub fn sync(
    db: &Db,
    source_id: &str,
    password: &str,
    on_progress: &mut dyn FnMut(usize, &str),
    should_stop: &dyn Fn() -> bool,
) -> Result<SyncSummary, ImapError> {
    let source = db.get_source(source_id)?.ok_or_else(|| ImapError::Import(format!("no source {source_id}")))?;
    let mut config: ImapSourceConfig = match serde_json::from_value(source.config.clone()) {
        Ok(c) => c,
        Err(e) => {
            let err = ImapError::Import(format!(
                "this mailbox's settings are unreadable ({e}); remove it and connect it again"
            ));
            db.set_source_status(source_id, "failed", Some(&json!({ "message": err.to_string() })))?;
            return Err(err);
        }
    };
    db.set_source_status(source_id, "importing", None)?;
    let result = sync_inner(db, source_id, &mut config, password, on_progress, should_stop);
    // Every attempt is recorded, successful or not, so the schedule waits a
    // whole interval before the next one rather than retrying every minute.
    config.last_checked_at = Some(crate::ids::now_rfc3339());
    db.set_source_config(source_id, &serde_json::to_value(&config).unwrap_or(Value::Null))?;
    match &result {
        Ok(_) => db.set_source_status(source_id, "imported", None)?,
        Err(ImapError::Canceled) => db.set_source_status(source_id, "ready", None)?,
        Err(e) => db.set_source_status(source_id, "failed", Some(&json!({ "message": e.to_string() })))?,
    }
    result
}

/// A check that could not start (no password): recorded like any other
/// attempt, so the schedule backs off instead of failing every minute.
pub fn record_failure(db: &Db, source_id: &str, message: &str) -> Result<(), DbError> {
    if let Some(source) = db.get_source(source_id)? {
        if let Ok(mut config) = serde_json::from_value::<ImapSourceConfig>(source.config) {
            config.last_checked_at = Some(crate::ids::now_rfc3339());
            db.set_source_config(source_id, &serde_json::to_value(&config).unwrap_or(Value::Null))?;
        }
        db.set_source_status(source_id, "failed", Some(&json!({ "message": message })))?;
    }
    Ok(())
}

/// Why a mailbox's password could not be given again.
#[derive(Debug, thiserror::Error)]
pub enum PasswordError {
    #[error("That mailbox isn't connected any more.")]
    NotAMailbox,
    #[error("I can't read this mailbox's settings; remove it and connect it again.")]
    UnreadableSettings,
    #[error(transparent)]
    Login(#[from] ImapError),
    #[error("I couldn't save the password: {0}")]
    Save(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

/// Give a connected mailbox its password again — the provider's app password
/// was replaced, or the saved one can no longer be unlocked — without
/// removing it and everything imported through it. The login is tried first,
/// and `save` is called only when it worked; a mailbox whose last check
/// failed is then ready to be checked again. Nothing imported is touched.
pub fn replace_password(
    db: &Db,
    source_id: &str,
    password: &str,
    save: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), PasswordError> {
    let source = db.get_source(source_id)?.filter(|s| s.connector == CONNECTOR).ok_or(PasswordError::NotAMailbox)?;
    let config: ImapSourceConfig =
        serde_json::from_value(source.config).map_err(|_| PasswordError::UnreadableSettings)?;
    probe(&config.account, password)?;
    save(password).map_err(PasswordError::Save)?;
    // Only a failure is cleared: a mailbox being read, or read fine, keeps
    // saying so.
    if source.status == "failed" {
        db.set_source_status(source_id, "ready", None)?;
    }
    Ok(())
}

fn sync_inner(
    db: &Db,
    source_id: &str,
    config: &mut ImapSourceConfig,
    password: &str,
    on_progress: &mut dyn FnMut(usize, &str),
    should_stop: &dyn Fn() -> bool,
) -> Result<SyncSummary, ImapError> {
    let mut conn = Connection::open(&config.account, password)?;
    // Folders are looked up every time: a sent folder renamed, or shown in
    // another language after a settings change, should not break every check
    // that follows.
    let (folders, sent) = choose_folders(&conn.list()?);
    config.folders = folders.clone();
    config.sent_folder = sent.clone();

    let mut summary = SyncSummary::default();
    // What is new, per folder: (folder, uidvalidity, uids to read, highest uid seen).
    let mut plan: Vec<(String, u32, Vec<u32>, u32)> = Vec::new();
    for folder in &folders {
        let (validity, exists) = conn.examine(folder)?;
        let mark = config.marks.entry(folder.clone()).or_default();
        if mark.uid_validity != validity {
            if mark.uid_validity != 0 {
                summary.renumbered.push(folder.clone());
            }
            *mark = FolderMark { uid_validity: validity, last_uid: 0 };
        }
        if exists == 0 {
            // Some servers answer a search of an empty folder with an error.
            plan.push((folder.clone(), validity, Vec::new(), 0));
            continue;
        }
        let mut uids = conn.uids_after(mark.last_uid)?;
        if mark.last_uid == 0 && uids.len() > FIRST_SYNC_LIMIT {
            uids = uids.split_off(uids.len() - FIRST_SYNC_LIMIT);
        }
        let highest = uids.iter().copied().max().unwrap_or(0);
        plan.push((folder.clone(), validity, uids, highest));
    }

    let mut mails: Vec<Mail> = Vec::new();
    for (folder, _, uids, _) in &plan {
        if uids.is_empty() {
            summary.folders.push((folder.clone(), 0));
            continue;
        }
        conn.examine(folder)?;
        let mut sizes: HashMap<u32, u64> = HashMap::new();
        for chunk in uids.chunks(500) {
            sizes.extend(conn.sizes(chunk)?);
        }
        let mut batch: Vec<u32> = Vec::new();
        let mut batch_bytes = 0u64;
        let mut batches: Vec<Vec<u32>> = Vec::new();
        for uid in uids {
            let size = sizes.get(uid).copied().unwrap_or(0);
            if size > MAX_MESSAGE_BYTES as u64 {
                summary.too_large += 1;
                continue;
            }
            if !batch.is_empty() && (batch.len() >= CHUNK || batch_bytes + size > CHUNK_BYTES) {
                batches.push(std::mem::take(&mut batch));
                batch_bytes = 0;
            }
            batch.push(*uid);
            batch_bytes += size;
        }
        if !batch.is_empty() {
            batches.push(batch);
        }
        let mut read = 0;
        for b in batches {
            if should_stop() {
                conn.logout();
                return Err(ImapError::Canceled);
            }
            let fetched = conn.fetch(&b)?;
            summary.fetched += fetched.len();
            read += fetched.len();
            for (_, bytes) in &fetched {
                if bytes.is_empty() {
                    summary.too_large += 1;
                    continue;
                }
                // One message that trips a bug in parsing is skipped, not
                // allowed to fail this check and every one after it.
                match std::panic::catch_unwind(|| parse_raw(bytes)) {
                    Ok(Some(mut m)) => {
                        m.sent |= sent.as_deref() == Some(folder.as_str());
                        mails.push(m);
                    }
                    Ok(None) => summary.without_text += 1,
                    Err(_) => summary.unreadable += 1,
                }
            }
            on_progress(summary.fetched, folder);
        }
        summary.folders.push((folder.clone(), read));
    }
    conn.logout();

    let mut importer = Importer::begin(db, source_id)?;
    for convo in thread(mails) {
        importer.take(convo)?;
    }
    let imported = importer.finish()?;
    summary.inserted = imported.inserted;
    summary.duplicates = imported.duplicates;
    summary.from_self = imported.from_self;

    // Everything is in; only now move the position past it.
    for (folder, validity, _, highest) in plan {
        let mark = config.marks.entry(folder).or_default();
        mark.uid_validity = validity;
        mark.last_uid = mark.last_uid.max(highest);
    }
    Ok(summary)
}

// -------------------------------------------------------------- schedule

/// How often connected mailboxes are checked, in minutes. `0` is off.
pub const INTERVAL_SETTING: &str = "mail.checkEveryMinutes";
pub const DEFAULT_INTERVAL_MINUTES: i64 = 15;
/// Checking more often than this is not polite to a mail server, and not
/// useful to a person.
pub const MIN_INTERVAL_MINUTES: i64 = 5;
/// After a failed check, wait this many intervals (at most six hours) before
/// trying again: a mailbox whose password was changed should not produce an
/// error every few minutes.
const FAILED_BACKOFF: i64 = 4;

pub fn interval_minutes(db: &Db) -> Result<i64, DbError> {
    let v = db.get_setting::<i64>(INTERVAL_SETTING)?.unwrap_or(DEFAULT_INTERVAL_MINUTES);
    Ok(if v <= 0 { 0 } else { v.max(MIN_INTERVAL_MINUTES) })
}

/// Mailboxes due a check at `now`: connected, readable, not already queued or
/// being checked, and last attempted longer ago than the interval (or never).
/// "Being checked" is decided by the job queue, not by the source's status,
/// so a status left behind by a crash cannot stop checking for good.
pub fn due(db: &Db, now: chrono::DateTime<chrono::Utc>) -> Result<Vec<String>, DbError> {
    let minutes = interval_minutes(db)?;
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
        let Ok(config) = serde_json::from_value::<ImapSourceConfig>(s.config.clone()) else {
            // Unreadable settings cannot be checked; it says so on its row.
            continue;
        };
        let wait = if s.status == "failed" { (minutes * FAILED_BACKOFF).min(360) } else { minutes };
        let last = config
            .last_checked_at
            .as_deref()
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok().map(|d| d.with_timezone(&chrono::Utc)));
        let is_due = match last {
            None => true,
            Some(at) => now.signed_duration_since(at) >= chrono::Duration::minutes(wait),
        };
        if is_due {
            out.push(s.id);
        }
    }
    Ok(out)
}

/// `due` at the current time.
pub fn due_now(db: &Db) -> Result<Vec<String>, DbError> {
    due(db, chrono::Utc::now())
}

/// At startup: a mailbox left "importing" by a check that never finished is
/// ready again, unless a check for it is actually queued.
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

/// For the home screen: whether mail arrives by itself, how often, when it
/// was last looked at, and whether the last look failed. `None` when no
/// mailbox is connected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailChecking {
    pub mailboxes: usize,
    /// `0` when checking is off.
    pub every_minutes: i64,
    pub last_checked_at: Option<String>,
    /// Why the last check of a mailbox failed, when one did.
    pub failing: Option<String>,
}

pub fn checking(db: &Db) -> Result<Option<MailChecking>, DbError> {
    let boxes: Vec<crate::db::Source> = db.list_sources()?.into_iter().filter(|s| s.connector == CONNECTOR).collect();
    if boxes.is_empty() {
        return Ok(None);
    }
    let last =
        boxes.iter().filter_map(|s| s.config.get("lastCheckedAt").and_then(|v| v.as_str())).max().map(str::to_string);
    let failing = boxes.iter().find(|s| s.status == "failed").map(|s| {
        s.last_error
            .as_ref()
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("The last check failed.")
            .to_string()
    });
    Ok(Some(MailChecking {
        mailboxes: boxes.len(),
        every_minutes: interval_minutes(db)?,
        last_checked_at: last,
        failing,
    }))
}

// ------------------------------------------------------------------- job

pub const JOB_KIND: &str = "check_mailbox";

/// Reads a mailbox's password from wherever the app keeps secrets.
pub type PasswordFor = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

pub struct CheckMailboxExecutor {
    password_for: PasswordFor,
}

impl CheckMailboxExecutor {
    pub fn shared(password_for: PasswordFor) -> Arc<dyn crate::jobs::JobExecutor> {
        Arc::new(CheckMailboxExecutor { password_for })
    }
}

impl crate::jobs::JobExecutor for CheckMailboxExecutor {
    fn kinds(&self) -> &'static [&'static str] {
        &[JOB_KIND]
    }
    fn resumable(&self, _kind: &str) -> bool {
        // The mailbox keeps its own position; running again continues.
        true
    }
    fn execute(&self, ctx: crate::jobs::JobContext) -> crate::jobs::JobFuture {
        let password_for = self.password_for.clone();
        Box::pin(async move {
            let source_id = ctx.job.payload["sourceId"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| crate::jobs::JobError::Failed("no mailbox was named".into()))?;
            let Some(password) = password_for(&secret_key(&source_id)) else {
                let why = "I don't have a password I can use for this mailbox. Give it to me again with New password, under Your mail.";
                record_failure(&ctx.db, &source_id, why)?;
                return Err(crate::jobs::JobError::Failed(why.into()));
            };
            let db = ctx.db.clone();
            let progress_ctx = ctx.clone();
            let stop_ctx = ctx.clone();
            let summary = tokio::task::block_in_place(|| {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    sync(
                        &db,
                        &source_id,
                        &password,
                        &mut |n, folder| progress_ctx.progress(n as i64, 0, &format!("reading {folder}")),
                        &|| stop_ctx.check_cancel().is_err(),
                    )
                }))
            });
            // A panic would leave the mailbox marked as being read, with no
            // back-off; record it as the failed attempt it was.
            let summary = match summary {
                Ok(s) => s,
                Err(_) => {
                    let why = "Something went wrong inside Mimic while reading this mailbox.";
                    record_failure(&db, &source_id, why)?;
                    return Err(crate::jobs::JobError::Failed(why.into()));
                }
            };
            match summary {
                Ok(s) => Ok(serde_json::to_value(s).unwrap_or(Value::Null)),
                Err(ImapError::Canceled) => Err(crate::jobs::JobError::Canceled),
                Err(e) => Err(crate::jobs::JobError::Failed(e.to_string())),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    #[test]
    fn a_folder_list_yields_the_inbox_and_the_sent_folder() {
        let by_attribute = vec![
            (vec![], "INBOX".to_string()),
            (vec!["\\hasnochildren".into(), "\\sent".into()], "Outbox Archive".to_string()),
            (vec![], "Sent".to_string()),
        ];
        assert_eq!(
            choose_folders(&by_attribute),
            (vec!["INBOX".into(), "Outbox Archive".into()], Some("Outbox Archive".into()))
        );

        let by_name = vec![(vec![], "INBOX".to_string()), (vec![], "[Gmail]/Sent Mail".to_string())];
        assert_eq!(choose_folders(&by_name).1.as_deref(), Some("[Gmail]/Sent Mail"));

        let none = vec![(vec![], "INBOX".to_string()), (vec!["\\noselect".into()], "Sent".to_string())];
        assert_eq!(
            choose_folders(&none),
            (vec!["INBOX".into()], None),
            "a folder that cannot be opened is not a sent folder"
        );
    }

    #[test]
    fn quoting_escapes_and_line_breaks_are_refused() {
        assert_eq!(quote(r#"pa"ss\word"#).unwrap(), r#""pa\"ss\\word""#);
        assert!(quote("pass\r\nm2 DELETE INBOX").is_err(), "a line break cannot smuggle a second command");
        assert_eq!(unquote(r#""Sent \"Items\"""#), r#"Sent "Items""#);
    }

    #[test]
    fn loopback_is_recognised_and_nothing_else_is() {
        for h in ["localhost", "127.0.0.1", "127.0.0.2", "::1", "[::1]"] {
            assert!(is_loopback_host(h), "{h}");
        }
        for h in [
            "imap.gmail.com",
            "10.0.0.5",
            "localhost.evil.com",
            "192.168.1.2",
            "127.0.0.1.example.com",
            "127.x.example.com",
        ] {
            assert!(!is_loopback_host(h), "{h}");
        }
    }

    #[test]
    fn plain_text_to_a_remote_server_is_refused_before_connecting() {
        let account =
            ImapAccount { host: "imap.example.com".into(), port: 143, username: "c".into(), security: Security::Plain };
        assert!(matches!(Connection::open(&account, "secret"), Err(ImapError::PlainRefused(_))));
    }

    // ------------------------------------------------------ a scripted server

    struct Mailbox {
        name: &'static str,
        validity: u32,
        /// (uid, raw message)
        messages: Vec<(u32, String)>,
    }

    /// A tiny IMAP server: enough of LOGIN, LIST, EXAMINE, UID SEARCH and
    /// UID FETCH to drive the real client over a real socket. It records
    /// every command it received so tests can check what was (and was not)
    /// sent.
    fn serve(boxes: Vec<Mailbox>, password: &'static str) -> (u16, std::sync::mpsc::Receiver<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let mut seen = Vec::new();
                let mut w = stream.try_clone().unwrap();
                let mut r = BufReader::new(stream);
                w.write_all(b"* OK test server ready\r\n").unwrap();
                let mut selected: Option<usize> = None;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    let line = line.trim_end().to_string();
                    seen.push(line.clone());
                    let (tag, cmd) = line.split_once(' ').unwrap();
                    let upper = cmd.to_ascii_uppercase();
                    if upper.starts_with("LOGIN") {
                        if cmd.ends_with(&format!("\"{password}\"")) {
                            w.write_all(format!("{tag} OK logged in\r\n").as_bytes()).unwrap();
                        } else {
                            w.write_all(format!("{tag} NO [AUTHENTICATIONFAILED] Invalid credentials\r\n").as_bytes())
                                .unwrap();
                        }
                    } else if upper.starts_with("LIST") {
                        for b in &boxes {
                            let attrs = if b.name == "Sent" { "\\HasNoChildren \\Sent" } else { "\\HasNoChildren" };
                            w.write_all(format!("* LIST ({attrs}) \"/\" \"{}\"\r\n", b.name).as_bytes()).unwrap();
                        }
                        w.write_all(format!("{tag} OK list done\r\n").as_bytes()).unwrap();
                    } else if let Some(arg) = cmd.strip_prefix("EXAMINE ") {
                        let name = arg.trim().trim_matches('"');
                        selected = boxes.iter().position(|b| b.name == name);
                        let b = &boxes[selected.unwrap()];
                        w.write_all(
                            format!(
                                "* {} EXISTS\r\n* OK [UIDVALIDITY {}] ok\r\n{tag} OK [READ-ONLY] done\r\n",
                                b.messages.len(),
                                b.validity
                            )
                            .as_bytes(),
                        )
                        .unwrap();
                    } else if let Some(range) = upper.strip_prefix("UID SEARCH UID ") {
                        let from: u32 = range.split(':').next().unwrap().parse().unwrap();
                        let b = &boxes[selected.unwrap()];
                        let mut uids: Vec<u32> = b.messages.iter().map(|(u, _)| *u).filter(|u| *u >= from).collect();
                        // Real servers answer "n:*" with the last message even when it is older.
                        if uids.is_empty() {
                            if let Some((u, _)) = b.messages.last() {
                                uids.push(*u);
                            }
                        }
                        let list = uids.iter().map(u32::to_string).collect::<Vec<_>>().join(" ");
                        w.write_all(format!("* SEARCH {list}\r\n{tag} OK search done\r\n").as_bytes()).unwrap();
                    } else if let Some(args) = cmd.strip_prefix("UID FETCH ").filter(|a| a.contains("RFC822.SIZE")) {
                        let set: Vec<u32> =
                            args.split_whitespace().next().unwrap().split(',').map(|n| n.parse().unwrap()).collect();
                        let b = &boxes[selected.unwrap()];
                        for (seq, (uid, raw)) in b.messages.iter().enumerate() {
                            if set.contains(uid) {
                                // A message marked huge reports a size over the
                                // limit; the client must skip it unfetched.
                                let size = if raw.contains("X-Test-Huge: yes") { 30 * 1024 * 1024 } else { raw.len() };
                                w.write_all(
                                    format!("* {} FETCH (UID {uid} RFC822.SIZE {size})\r\n", seq + 1).as_bytes(),
                                )
                                .unwrap();
                            }
                        }
                        w.write_all(format!("{tag} OK sizes\r\n").as_bytes()).unwrap();
                    } else if let Some(args) = cmd.strip_prefix("UID FETCH ") {
                        let set: Vec<u32> =
                            args.split_whitespace().next().unwrap().split(',').map(|n| n.parse().unwrap()).collect();
                        let b = &boxes[selected.unwrap()];
                        for (seq, (uid, raw)) in b.messages.iter().enumerate() {
                            if set.contains(uid) {
                                let crlf = raw.replace('\n', "\r\n");
                                w.write_all(
                                    format!("* {} FETCH (UID {uid} BODY[] {{{}}}\r\n", seq + 1, crlf.len()).as_bytes(),
                                )
                                .unwrap();
                                w.write_all(crlf.as_bytes()).unwrap();
                                w.write_all(b")\r\n").unwrap();
                            }
                        }
                        w.write_all(format!("{tag} OK fetch done\r\n").as_bytes()).unwrap();
                    } else if upper.starts_with("LOGOUT") {
                        w.write_all(format!("* BYE\r\n{tag} OK bye\r\n").as_bytes()).unwrap();
                        break;
                    } else {
                        w.write_all(format!("{tag} BAD unknown\r\n").as_bytes()).unwrap();
                    }
                }
                let _ = tx.send(seen);
            }
        });
        (port, rx)
    }

    fn mail(id: &str, from: &str, reply_to: Option<&str>, date: &str, body: &str) -> String {
        let irt = reply_to.map(|r| format!("In-Reply-To: <{r}>\nReferences: <{r}>\n")).unwrap_or_default();
        format!(
            "From: {from}\nTo: someone\nSubject: Offsite\nDate: {date}\nMessage-ID: <{id}>\n{irt}MIME-Version: 1.0\nContent-Type: text/plain; charset=utf-8\nContent-Transfer-Encoding: quoted-printable\n\n{body}\n"
        )
    }

    fn db_for(port: u16) -> (Db, String) {
        let db = Db::open_in_memory().unwrap();
        // Fixed dates: the waiting window is measured against the clock, so
        // it is opened to any age here (`repo_waiting` tests the window).
        db.set_setting(crate::db::WITHIN_DAYS_SETTING, &0).unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(crate::db::IdentifierKind::Email, "c@example.com").unwrap();
        let config = ImapSourceConfig {
            account: ImapAccount {
                host: "127.0.0.1".into(),
                port,
                username: "c@example.com".into(),
                security: Security::Plain,
            },
            folders: vec![],
            sent_folder: None,
            marks: BTreeMap::new(),
            last_checked_at: None,
        };
        let source = db
            .create_source(&crate::db::NewSource {
                connector: CONNECTOR.into(),
                name: "c@example.com".into(),
                channel: "email".into(),
                location: None,
                config: serde_json::to_value(&config).unwrap(),
            })
            .unwrap();
        (db, source.id)
    }

    #[test]
    fn a_mailbox_is_read_threaded_and_then_only_new_mail_is_fetched() {
        let inbox = Mailbox {
            name: "INBOX",
            validity: 7,
            messages: vec![(
                1,
                mail(
                    "q1@x",
                    "Ada <ada@example.com>",
                    None,
                    "Mon, 2 Mar 2026 09:00:00 +0000",
                    "can you make the offsite?",
                ),
            )],
        };
        let sent = Mailbox {
            name: "Sent",
            validity: 3,
            messages: vec![(
                11,
                mail(
                    "a1@x",
                    "C <c@example.com>",
                    Some("q1@x"),
                    "Mon, 2 Mar 2026 09:30:00 +0000",
                    "yes =E2=80=94 see you there",
                ),
            )],
        };
        let (port, seen) = serve(vec![inbox, sent], "app-pass");
        let (db, source) = db_for(port);

        let first = sync(&db, &source, "app-pass", &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(first.fetched, 2);
        assert_eq!(first.inserted, 2);
        assert_eq!(first.from_self, 1, "the sent folder is where the user's own writing is");
        let commands = seen.recv().unwrap();
        assert!(
            commands.iter().all(|c| !c.to_ascii_uppercase().contains(" SELECT ")),
            "read-only: EXAMINE, never SELECT"
        );
        assert!(
            commands.iter().filter(|c| c.contains("BODY")).all(|c| c.contains("BODY.PEEK[]")),
            "nothing is marked read"
        );
        assert!(!commands.iter().any(|c| ["STORE", "EXPUNGE", "DELETE", "APPEND", "COPY", "MOVE"]
            .iter()
            .any(|w| c.to_ascii_uppercase().contains(w))));

        // One conversation: the reply joined the question.
        assert_eq!(db.count_conversations().unwrap(), 1);
        let cfg: ImapSourceConfig = serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        assert_eq!(cfg.sent_folder.as_deref(), Some("Sent"));
        assert_eq!(cfg.marks["INBOX"], FolderMark { uid_validity: 7, last_uid: 1 });
        assert_eq!(cfg.marks["Sent"], FolderMark { uid_validity: 3, last_uid: 11 });
        assert!(cfg.last_checked_at.is_some());
        assert_eq!(db.get_source(&source).unwrap().unwrap().status, "imported");
        let body: String =
            db.conn().query_row("SELECT body FROM messages WHERE direction = 'self'", [], |r| r.get(0)).unwrap();
        assert_eq!(body, "yes — see you there", "quoted-printable decoded, not stored raw");

        // Second check: the server answers "12:*" with message 11 anyway; nothing is fetched.
        let second = sync(&db, &source, "app-pass", &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(second.fetched, 0);
        assert_eq!(second.inserted, 0);
    }

    #[test]
    fn a_reply_that_arrives_later_joins_the_thread_it_answers() {
        let (port, _) = serve(
            vec![
                Mailbox {
                    name: "INBOX",
                    validity: 1,
                    messages: vec![(
                        1,
                        mail(
                            "q1@x",
                            "Ada <ada@example.com>",
                            None,
                            "Mon, 2 Mar 2026 09:00:00 +0000",
                            "can you make the offsite?",
                        ),
                    )],
                },
                Mailbox { name: "Sent", validity: 1, messages: vec![] },
            ],
            "pw",
        );
        let (db, source) = db_for(port);
        sync(&db, &source, "pw", &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(db.count_conversations().unwrap(), 1);

        // Ada follows up in a later check, replying to her own message.
        let (port2, _) = serve(
            vec![
                Mailbox {
                    name: "INBOX",
                    validity: 1,
                    messages: vec![
                        (
                            1,
                            mail(
                                "q1@x",
                                "Ada <ada@example.com>",
                                None,
                                "Mon, 2 Mar 2026 09:00:00 +0000",
                                "can you make the offsite?",
                            ),
                        ),
                        (
                            2,
                            mail(
                                "q2@x",
                                "Ada <ada@example.com>",
                                Some("q1@x"),
                                "Tue, 3 Mar 2026 09:00:00 +0000",
                                "any news?",
                            ),
                        ),
                    ],
                },
                Mailbox { name: "Sent", validity: 1, messages: vec![] },
            ],
            "pw",
        );
        let mut cfg: ImapSourceConfig =
            serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        cfg.account.port = port2;
        db.set_source_config(&source, &serde_json::to_value(&cfg).unwrap()).unwrap();
        let s = sync(&db, &source, "pw", &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(s.fetched, 1, "only the new message");
        assert_eq!(db.count_conversations().unwrap(), 1, "it joined the existing thread");
    }

    #[test]
    fn a_renumbered_folder_is_read_again_without_duplicating_anything() {
        let make = |validity| {
            vec![
                Mailbox {
                    name: "INBOX",
                    validity,
                    messages: vec![(
                        5,
                        mail("q1@x", "Ada <ada@example.com>", None, "Mon, 2 Mar 2026 09:00:00 +0000", "hello"),
                    )],
                },
                Mailbox { name: "Sent", validity: 1, messages: vec![] },
            ]
        };
        let (port, _) = serve(make(1), "pw");
        let (db, source) = db_for(port);
        sync(&db, &source, "pw", &mut |_, _| {}, &|| false).unwrap();
        let (port2, _) = serve(make(2), "pw");
        let mut cfg: ImapSourceConfig =
            serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        cfg.account.port = port2;
        db.set_source_config(&source, &serde_json::to_value(&cfg).unwrap()).unwrap();
        let s = sync(&db, &source, "pw", &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(s.renumbered, vec!["INBOX".to_string()]);
        assert_eq!(s.fetched, 1);
        assert_eq!(s.inserted, 0);
        assert_eq!(s.duplicates, 1);
    }

    #[test]
    fn a_wrong_password_says_so_and_marks_the_source_failed() {
        let (port, _) = serve(vec![Mailbox { name: "INBOX", validity: 1, messages: vec![] }], "right");
        let (db, source) = db_for(port);
        let err = sync(&db, &source, "wrong", &mut |_, _| {}, &|| false).unwrap_err();
        assert!(matches!(err, ImapError::Login(_)), "{err}");
        assert!(err.to_string().contains("app password"));
        let s = db.get_source(&source).unwrap().unwrap();
        assert_eq!(s.status, "failed");
        assert!(!s.last_error.unwrap().to_string().contains("wrong"), "the password never reaches an error");
    }

    #[test]
    fn a_new_password_is_kept_only_when_it_logs_in_and_nothing_imported_is_touched() {
        let inbox = vec![(1, mail("a@x", "Ada <ada@x>", None, "Mon, 1 Sep 2026 09:00:00 +0000", "Lunch?"))];
        let (port, _) = serve(vec![Mailbox { name: "INBOX", validity: 1, messages: inbox }], "right");
        let (db, source) = db_for(port);
        sync(&db, &source, "right", &mut |_, _| {}, &|| false).unwrap();
        let before = db.refresh_source_counts(&source).unwrap();
        assert_eq!(before, 1);
        record_failure(&db, &source, "I don't have a password I can use for this mailbox.").unwrap();

        let mut saved: Option<String> = None;
        let err = replace_password(&db, &source, "wrong", |pw| {
            saved = Some(pw.to_string());
            Ok(())
        })
        .unwrap_err();
        assert!(matches!(err, PasswordError::Login(_)), "{err}");
        assert!(!err.to_string().contains("wrong"), "the password never reaches an error");
        assert_eq!(saved, None, "a password that does not log in is not kept");
        assert_eq!(db.get_source(&source).unwrap().unwrap().status, "failed");

        replace_password(&db, &source, "right", |pw| {
            saved = Some(pw.to_string());
            Ok(())
        })
        .unwrap();
        assert_eq!(saved.as_deref(), Some("right"));
        let s = db.get_source(&source).unwrap().unwrap();
        assert_eq!(s.status, "ready");
        assert!(s.last_error.is_none());
        assert_eq!(db.refresh_source_counts(&source).unwrap(), before, "nothing imported was touched");

        // A mailbox that was fine stays as it was.
        db.set_source_status(&source, "imported", None).unwrap();
        replace_password(&db, &source, "right", |_| Ok(())).unwrap();
        assert_eq!(db.get_source(&source).unwrap().unwrap().status, "imported");

        let err = replace_password(&db, &source, "right", |_| Err("disk full".into())).unwrap_err();
        assert_eq!(err.to_string(), "I couldn't save the password: disk full");
        assert!(matches!(
            replace_password(&db, "no-such-source", "right", |_| Ok(())),
            Err(PasswordError::NotAMailbox)
        ));
    }

    #[test]
    fn a_canceled_check_imports_nothing_and_loses_nothing() {
        let many: Vec<(u32, String)> = (1..=(CHUNK as u32 + 5))
            .map(|u| {
                (
                    u,
                    mail(
                        &format!("m{u}@x"),
                        "Ada <ada@example.com>",
                        None,
                        "Mon, 2 Mar 2026 09:00:00 +0000",
                        &format!("message {u}"),
                    ),
                )
            })
            .collect();
        let boxes = || {
            vec![
                Mailbox { name: "INBOX", validity: 1, messages: many.clone() },
                Mailbox { name: "Sent", validity: 1, messages: vec![] },
            ]
        };
        let (port, _) = serve(boxes(), "pw");
        let (db, source) = db_for(port);
        let calls = std::cell::Cell::new(0);
        let err = sync(&db, &source, "pw", &mut |_, _| {}, &|| {
            calls.set(calls.get() + 1);
            calls.get() > 1
        })
        .unwrap_err();
        assert!(matches!(err, ImapError::Canceled));
        assert_eq!(db.count_messages().unwrap(), 0, "nothing half-imported");
        let cfg: ImapSourceConfig = serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        assert_eq!(cfg.marks["INBOX"].last_uid, 0, "and the position did not move, so nothing is skipped next time");
        assert_eq!(db.get_source(&source).unwrap().unwrap().status, "ready");

        let (port2, _) = serve(boxes(), "pw");
        let mut cfg = cfg;
        cfg.account.port = port2;
        db.set_source_config(&source, &serde_json::to_value(&cfg).unwrap()).unwrap();
        let s = sync(&db, &source, "pw", &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(s.inserted, CHUNK + 5, "the next check reads all of it");
    }

    /// The user's message in Sent and the reply to it in the inbox, both new
    /// in one check, are one conversation — whichever folder is read first —
    /// and the thread is waiting on the user because the reply is last.
    #[test]
    fn folder_order_cannot_split_a_thread() {
        let (port, _) = serve(
            vec![
                Mailbox {
                    name: "INBOX",
                    validity: 1,
                    messages: vec![(
                        1,
                        mail(
                            "b@x",
                            "Ada <ada@example.com>",
                            Some("a@x"),
                            "Mon, 2 Mar 2026 10:00:00 +0000",
                            "sounds good, what time?",
                        ),
                    )],
                },
                Mailbox {
                    name: "Sent",
                    validity: 1,
                    messages: vec![(
                        1,
                        mail("a@x", "C <c@example.com>", None, "Mon, 2 Mar 2026 09:00:00 +0000", "lunch friday?"),
                    )],
                },
            ],
            "pw",
        );
        let (db, source) = db_for(port);
        sync(&db, &source, "pw", &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(db.count_conversations().unwrap(), 1);
        let waiting = db.threads_awaiting_reply(10).unwrap();
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].last_message, "sounds good, what time?");
    }

    #[test]
    fn an_answered_thread_is_not_left_waiting() {
        let (port, _) = serve(
            vec![
                Mailbox {
                    name: "INBOX",
                    validity: 1,
                    messages: vec![(
                        1,
                        mail(
                            "q@x",
                            "Ada <ada@example.com>",
                            None,
                            "Mon, 2 Mar 2026 09:00:00 +0000",
                            "can you make the offsite?",
                        ),
                    )],
                },
                Mailbox {
                    name: "Sent",
                    validity: 1,
                    messages: vec![(
                        1,
                        mail(
                            "r@x",
                            "C <c@example.com>",
                            Some("q@x"),
                            "Mon, 2 Mar 2026 09:30:00 +0000",
                            "yes, see you there",
                        ),
                    )],
                },
            ],
            "pw",
        );
        let (db, source) = db_for(port);
        sync(&db, &source, "pw", &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(db.count_conversations().unwrap(), 1);
        assert!(db.threads_awaiting_reply(10).unwrap().is_empty(), "the user replied last");
    }

    #[test]
    fn whoever_wrote_what_is_in_the_sent_folder_is_asked_about_and_folded_in_on_a_yes() {
        let with_subject = |raw: String, subject: &str| raw.replace("Subject: Offsite", &format!("Subject: {subject}"));
        let at = |minute: u32| format!("Mon, 2 Mar 2026 09:{minute:02}:00 +0000");
        // Pat sends some of the user's mail for them, and writes to them too.
        let pat = |id: &str, subject: &str, minute: u32| {
            with_subject(mail(id, "Pat <pat@example.com>", None, &at(minute), "booked it"), subject)
        };
        let (port, _) = serve(
            vec![
                Mailbox {
                    name: "INBOX",
                    validity: 1,
                    messages: vec![
                        (1, mail("q1@x", "Ada <ada@example.com>", None, &at(0), "can you make the offsite?")),
                        (
                            2,
                            with_subject(
                                mail("q2@x", "Grace <grace@example.com>", None, &at(1), "numbers?"),
                                "Numbers",
                            ),
                        ),
                        (3, pat("p1@x", "Keys", 2)),
                        (4, pat("p2@x", "Plan", 3)),
                        (5, pat("p3@x", "Hello", 4)),
                    ],
                },
                Mailbox {
                    name: "Sent",
                    validity: 1,
                    messages: vec![
                        (1, mail("r1@x", "C at work <c@work.example>", Some("q1@x"), &at(30), "yes, see you there")),
                        (
                            2,
                            with_subject(
                                mail(
                                    "r2@x",
                                    "C at work <c@work.example>",
                                    Some("q2@x"),
                                    &at(40),
                                    "sending them tonight",
                                ),
                                "Re: Numbers",
                            ),
                        ),
                        (3, pat("d1@x", "Room", 50)),
                        (4, pat("d2@x", "Taxi", 51)),
                        (5, pat("d3@x", "Lunch", 52)),
                        // From an address that is already the user's: nothing to ask.
                        (6, with_subject(mail("s1@x", "C <c@example.com>", None, &at(55), "note to self"), "Note")),
                    ],
                },
            ],
            "pw",
        );
        let (db, source) = db_for(port);
        sync(&db, &source, "pw", &mut |_, _| {}, &|| false).unwrap();

        let people = db.sent_folder_people().unwrap();
        let listed: Vec<(&str, usize, usize)> =
            people.iter().map(|p| (p.address.as_str(), p.sent, p.owner.messages)).collect();
        assert_eq!(
            listed,
            vec![("c@work.example", 2, 2), ("pat@example.com", 3, 6)],
            "likeliest first: all of C at work's mail was in the Sent folder, half of Pat's"
        );
        assert_eq!(people[0].kind, "email");
        assert_eq!(people[0].owner.display_name, "C at work");
        assert_eq!(
            db.threads_awaiting_reply(10).unwrap().len(),
            8,
            "until then, what the user sent from work reads as someone waiting"
        );

        // No: Pat is not the user, and is not asked about again.
        db.keep_apart(&people[1].owner.participant_id).unwrap();
        assert_eq!(db.sent_folder_people().unwrap().len(), 1);

        // Yes, with what was shown: the work address is the user's, and so is what was sent from it.
        let added =
            db.add_user_address(crate::db::IdentifierKind::Email, &people[0].address, Some(&people[0].owner)).unwrap();
        assert_eq!(added.claimed.messages, 2);
        assert_eq!(added.claimed.people, 1);
        assert!(db.sent_folder_people().unwrap().is_empty());
        let waiting = db.threads_awaiting_reply(10).unwrap();
        assert_eq!(waiting.len(), 6, "Ada's and Grace's threads were answered; Pat's are still Pat's");
    }

    #[test]
    fn a_post_to_a_list_read_back_from_the_inbox_is_the_users_and_asks_about_nobody() {
        let at = "Mon, 2 Mar 2026 09:00:00 +0000";
        let (port, _) = serve(
            vec![
                Mailbox {
                    name: "INBOX",
                    validity: 1,
                    // The list's copy: the same message, its sender rewritten.
                    messages: vec![(1, mail("l1@x", "'C' via Team <team@lists.example>", None, at, "agenda attached"))],
                },
                Mailbox {
                    name: "Sent",
                    validity: 1,
                    messages: vec![(1, mail("l1@x", "C <c@example.com>", None, at, "agenda attached"))],
                },
            ],
            "pw",
        );
        let (db, source) = db_for(port);
        let s = sync(&db, &source, "pw", &mut |_, _| {}, &|| false).unwrap();
        assert_eq!((s.inserted, s.duplicates), (1, 1));
        assert_eq!(db.count_self_messages(None, None).unwrap(), 1, "the copy the user sent is the one kept");
        assert_eq!(db.count_participants().unwrap(), 0, "nobody is made up for the copy that was dropped");
        assert!(db.sent_folder_people().unwrap().is_empty(), "the list's copy was never in the Sent folder");
    }

    #[test]
    fn newsletters_in_the_inbox_are_read_but_not_left_waiting() {
        let newsletter = mail(
            "n@brand.example",
            "Brand <news@brand.example>",
            None,
            "Mon, 2 Mar 2026 10:00:00 +0000",
            "this week only: twenty percent off",
        )
        .replacen("MIME-Version", "List-Unsubscribe: <https://brand.example/u>\nMIME-Version", 1);
        let receipt = mail(
            "r@shop.example",
            "Shop <no-reply@shop.example>",
            None,
            "Mon, 2 Mar 2026 11:00:00 +0000",
            "order 1234 shipped",
        );
        let (port, _) = serve(
            vec![
                Mailbox {
                    name: "INBOX",
                    validity: 1,
                    messages: vec![
                        (
                            1,
                            mail(
                                "q@x",
                                "Ada <ada@example.com>",
                                None,
                                "Mon, 2 Mar 2026 09:00:00 +0000",
                                "free for a call?",
                            ),
                        ),
                        (2, newsletter),
                        (3, receipt),
                    ],
                },
                Mailbox { name: "Sent", validity: 1, messages: vec![] },
            ],
            "pw",
        );
        let (db, source) = db_for(port);
        sync(&db, &source, "pw", &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(db.count_messages().unwrap(), 3, "everything is read; nothing is thrown away");
        let waiting = db.threads_awaiting_reply(10).unwrap();
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].last_message, "free for a call?");
        assert_eq!(db.count_left_out().unwrap().automated, 2);
    }

    #[test]
    fn an_oversized_message_is_skipped_unfetched_and_does_not_stall_the_folder() {
        let huge = mail("big@x", "Ada <ada@example.com>", None, "Mon, 2 Mar 2026 09:00:00 +0000", "photos attached")
            .replacen("MIME-Version", "X-Test-Huge: yes\nMIME-Version", 1);
        let (port, seen) = serve(
            vec![
                Mailbox {
                    name: "INBOX",
                    validity: 1,
                    messages: vec![
                        (1, mail("s1@x", "Ada <ada@example.com>", None, "Mon, 2 Mar 2026 08:00:00 +0000", "hello")),
                        (2, huge),
                        (3, mail("s3@x", "Ada <ada@example.com>", None, "Mon, 2 Mar 2026 10:00:00 +0000", "and again")),
                    ],
                },
                Mailbox { name: "Sent", validity: 1, messages: vec![] },
            ],
            "pw",
        );
        let (db, source) = db_for(port);
        let s = sync(&db, &source, "pw", &mut |_, _| {}, &|| false).unwrap();
        assert_eq!((s.too_large, s.fetched, s.inserted), (1, 2, 2));
        let commands = seen.recv().unwrap();
        assert!(
            commands.iter().filter(|c| c.contains("BODY.PEEK")).all(|c| !c.contains(",2,") && !c.contains(" 2 ")),
            "{commands:?}"
        );
        let cfg: ImapSourceConfig = serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        assert_eq!(cfg.marks["INBOX"].last_uid, 3, "the position moved past it");
    }

    /// The same mailbox connected twice, or an export of it imported as well:
    /// each message is stored once, and the threads are the same threads.
    #[test]
    fn the_same_mail_through_two_sources_is_counted_once() {
        let boxes = || {
            vec![
                Mailbox {
                    name: "INBOX",
                    validity: 1,
                    messages: vec![(
                        1,
                        mail(
                            "q@x",
                            "Ada <ada@example.com>",
                            None,
                            "Mon, 2 Mar 2026 09:00:00 +0000",
                            "can you make it?",
                        ),
                    )],
                },
                Mailbox {
                    name: "Sent",
                    validity: 1,
                    messages: vec![(
                        1,
                        mail("r@x", "C <c@example.com>", Some("q@x"), "Mon, 2 Mar 2026 09:30:00 +0000", "yes"),
                    )],
                },
            ]
        };
        let (port, _) = serve(boxes(), "pw");
        let (db, first) = db_for(port);
        sync(&db, &first, "pw", &mut |_, _| {}, &|| false).unwrap();
        let (port2, _) = serve(boxes(), "pw");
        let config = ImapSourceConfig {
            account: ImapAccount {
                host: "127.0.0.1".into(),
                port: port2,
                username: "c@example.com".into(),
                security: Security::Plain,
            },
            folders: vec![],
            sent_folder: None,
            marks: BTreeMap::new(),
            last_checked_at: None,
        };
        let second = db
            .create_source(&crate::db::NewSource {
                connector: CONNECTOR.into(),
                name: "again".into(),
                channel: "email".into(),
                location: None,
                config: serde_json::to_value(&config).unwrap(),
            })
            .unwrap()
            .id;
        let s = sync(&db, &second, "pw", &mut |_, _| {}, &|| false).unwrap();
        assert_eq!((s.inserted, s.duplicates), (0, 2));
        assert_eq!(db.count_messages().unwrap(), 2);
        assert_eq!(db.count_self_messages(None, None).unwrap(), 1, "the user's reply is counted once");
    }

    #[test]
    fn a_failing_mailbox_backs_off_and_a_stale_status_does_not_stop_checking() {
        let (db, source) = db_for(1);
        let now = chrono::Utc::now();
        record_failure(&db, &source, "no password").unwrap();
        assert_eq!(db.get_source(&source).unwrap().unwrap().status, "failed");
        assert!(due(&db, now + chrono::Duration::minutes(20)).unwrap().is_empty(), "not straight back after a failure");
        assert!(!due(&db, now + chrono::Duration::minutes(61)).unwrap().is_empty(), "but it is tried again");
        let c = checking(&db).unwrap().unwrap();
        assert_eq!(c.failing.as_deref(), Some("no password"));

        db.set_source_status(&source, "importing", None).unwrap();
        assert_eq!(recover(&db).unwrap(), 1);
        assert_eq!(db.get_source(&source).unwrap().unwrap().status, "ready");
    }

    #[test]
    fn a_probe_finds_the_folders_and_warns_about_a_missing_sent_folder() {
        let (port, _) = serve(vec![Mailbox { name: "INBOX", validity: 1, messages: vec![] }], "pw");
        let account = ImapAccount { host: "localhost".into(), port, username: "c".into(), security: Security::Plain };
        let p = probe(&account, "pw").unwrap();
        assert_eq!(p.folders, vec!["INBOX".to_string()]);
        assert!(p.sent_folder.is_none());
        assert!(p.warnings.iter().any(|w| w.contains("sent folder")));
    }

    #[test]
    fn a_mailbox_is_due_on_its_interval_and_never_twice_at_once() {
        let (db, source) = db_for(1);
        let now = chrono::Utc::now();
        assert_eq!(due(&db, now).unwrap(), vec![source.clone()], "never checked: due");

        let mut cfg: ImapSourceConfig =
            serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        cfg.last_checked_at = Some(crate::ids::fmt_rfc3339(now - chrono::Duration::minutes(3)));
        db.set_source_config(&source, &serde_json::to_value(&cfg).unwrap()).unwrap();
        assert!(due(&db, now).unwrap().is_empty(), "checked three minutes ago");
        assert!(!due(&db, now + chrono::Duration::minutes(12)).unwrap().is_empty(), "and due fifteen after");

        db.create_job(JOB_KIND, &json!({ "sourceId": source }), true).unwrap();
        assert!(due(&db, now + chrono::Duration::minutes(60)).unwrap().is_empty(), "a queued check is not doubled");
    }

    #[test]
    fn checking_can_be_turned_off_and_cannot_be_made_rude() {
        let (db, _) = db_for(1);
        db.set_setting(INTERVAL_SETTING, &0).unwrap();
        assert_eq!(interval_minutes(&db).unwrap(), 0);
        assert!(due(&db, chrono::Utc::now()).unwrap().is_empty());
        db.set_setting(INTERVAL_SETTING, &1).unwrap();
        assert_eq!(interval_minutes(&db).unwrap(), MIN_INTERVAL_MINUTES);
        let c = checking(&db).unwrap().unwrap();
        assert_eq!((c.mailboxes, c.every_minutes, c.last_checked_at, c.failing), (1, MIN_INTERVAL_MINUTES, None, None));
        assert!(checking(&Db::open_in_memory().unwrap()).unwrap().is_none(), "no mailbox, nothing to say");
    }
}
