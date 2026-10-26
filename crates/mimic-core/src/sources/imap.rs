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
//!   A check writes nothing to the mailbox. The one write in this module is
//!   `copy_to_sent` (`APPEND`), of a reply Mimic sent, for a provider that
//!   keeps no copy itself.
//!
//! How it signs in: Gmail, iCloud, Fastmail and Yahoo accept an app password
//! (`LOGIN`). Outlook.com and Hotmail stopped accepting passwords of any kind
//! for IMAP in September 2024, and Microsoft 365 did in 2022–23: those sign
//! in with Microsoft. Gmail and Google Workspace can sign in with Google
//! instead of an app password (0.10.0-alpha.26). Both go through
//! `sources::oauth`, and present an access token with `AUTHENTICATE
//! XOAUTH2`. The secret store keeps the password, or the refresh token, under
//! the same key; a check trades a refresh token for an access token first
//! (`Credentials::credential`).
//!
//! The client is a small IMAP4rev1 subset written against `Read + Write`, so
//! the tests drive the real protocol code against a scripted server on a
//! loopback socket.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

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

/// How many of the Sent folder's newest messages `answering` reads the
/// reply headers of, and how many at a time.
const MOST_ANSWERING: usize = 1000;
const ANSWERING_CHUNK: usize = 200;

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
    /// How it signs in. Mailboxes connected before 0.10.0-alpha.14 have
    /// none stored, and use a password.
    #[serde(default)]
    pub auth: Auth,
}

/// How a mailbox signs in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Auth {
    /// An app password; the secret store keeps it.
    #[default]
    Password,
    /// Signed in with Microsoft; the secret store keeps the refresh token.
    Microsoft,
    /// Signed in with Google (0.10.0-alpha.26); likewise.
    Google,
}

impl Auth {
    /// The provider a mailbox signs in with, or none for a password.
    pub fn provider(self) -> Option<super::oauth::Provider> {
        match self {
            Auth::Password => None,
            Auth::Microsoft => Some(super::oauth::Provider::microsoft()),
            Auth::Google => Some(super::oauth::Provider::google()),
        }
    }
}

/// What one connection signs in with. Never printed.
#[derive(Clone)]
pub enum Credential {
    Password(String),
    /// An OAuth access token, for `AUTHENTICATE XOAUTH2`.
    AccessToken(String),
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Credential::Password(_) => "Credential::Password([redacted])",
            Credential::AccessToken(_) => "Credential::AccessToken([redacted])",
        })
    }
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
    /// When the last check that read the mailbox through began — not when
    /// it ended, and not one that failed or was stopped (0.10.0-alpha.44):
    /// everything sent before then is in what was read, so a reply held
    /// for a check knows whether the user answered first (`autoreply`).
    #[serde(default)]
    pub last_read_at: Option<String>,
    /// How replies go out through this mailbox, as the user set it
    /// (0.10.0-alpha.23). `None` for one connected before then, or never set:
    /// `sending_account` falls back to its provider's known server.
    #[serde(default)]
    pub sending: Option<super::smtp::SmtpAccount>,
    /// What goes under each reply sent from this mailbox, after the
    /// conventional `-- ` line (0.10.0-alpha.36); none until the user sets
    /// one.
    #[serde(default)]
    pub signature: Option<String>,
}

impl ImapSourceConfig {
    /// The signature as it goes under a reply: trimmed, and none when blank.
    pub fn signature(&self) -> Option<&str> {
        self.signature.as_deref().map(str::trim).filter(|s| !s.is_empty())
    }

    /// Where replies through this mailbox go out: as the user set it, or the
    /// server its provider is known to send from. `None` when neither.
    pub fn sending_account(&self) -> Option<super::smtp::SmtpAccount> {
        self.sending.clone().or_else(|| super::smtp::known_for(&self.account.host, &self.account.username))
    }
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
    #[error("The server didn't accept the sign-in ({0}). Sign in again under Your mail. If that doesn't help, IMAP may be turned off for this mailbox; for a work or school account, your admin can turn it on.")]
    SignIn(String),
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

/// A tagged "NO [CODE] human text" as the human text, cut to one line.
fn refusal(rest: &str) -> String {
    let mut reason = rest.split_once(' ').map(|(_, r)| r.trim()).unwrap_or_default();
    if reason.starts_with('[') {
        reason = reason.split_once(']').map(|(_, r)| r.trim()).unwrap_or(reason);
    }
    one_line(if reason.is_empty() { rest } else { reason })
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
            match literal_len(&text) {
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
                    super::transcript::note("imap <", &line.text);
                    return Ok(line);
                }
            }
        }
    }

    /// Read what has arrived: false when nothing has — the stream would have
    /// blocked (it doesn't, while waiting in `IDLE`) or its read timed out.
    fn fill_some(&mut self) -> Result<bool, ImapError> {
        let mut chunk = [0u8; 16 * 1024];
        match self.stream.read(&mut chunk) {
            Ok(0) => Err(ImapError::Io("the server closed the connection".into())),
            Ok(n) => {
                self.buf.extend_from_slice(&chunk[..n]);
                Ok(true)
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted
                ) =>
            {
                Ok(false)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Whether a whole response line, literals and all, is waiting to be read.
    fn has_whole_line(&self) -> bool {
        let mut at = 0usize;
        loop {
            let Some(p) = self.buf[at..].windows(2).position(|w| w == b"\r\n") else { return false };
            match literal_len(&String::from_utf8_lossy(&self.buf[at..at + p])) {
                None => return true,
                Some(n) => {
                    at = at.saturating_add(p + 2).saturating_add(n);
                    if at > self.buf.len() {
                        return false;
                    }
                }
            }
        }
    }

    /// The next response line as it arrives, or none once `deadline` passes
    /// or `should_stop` says so, both looked at between reads.
    fn line_by(&mut self, deadline: Instant, should_stop: &dyn Fn() -> bool) -> Result<Option<Line>, ImapError> {
        loop {
            if self.has_whole_line() {
                return self.read_line().map(Some);
            }
            if should_stop() || Instant::now() >= deadline {
                return Ok(None);
            }
            if self.buf.len() > MAX_MESSAGE_BYTES {
                return Err(ImapError::Protocol("a response line with no end".into()));
            }
            if !self.fill_some()? {
                std::thread::sleep(IDLE_PAUSE);
            }
        }
    }

    /// What the server says it can do (`CAPABILITY`), upper-cased.
    pub fn capabilities(&mut self) -> Result<Vec<String>, ImapError> {
        let lines = self.command("CAPABILITY")?;
        Ok(lines
            .iter()
            .filter_map(|l| l.text.strip_prefix("* CAPABILITY "))
            .flat_map(|r| r.split_whitespace().map(str::to_ascii_uppercase).collect::<Vec<_>>())
            .collect())
    }

    /// Begin `IDLE` (RFC 2177): send it, and wait for the go-ahead, keeping
    /// `count` to what the server says on the way. True when that says mail
    /// arrived. Nothing is read or changed by idling: the caller checks the
    /// mailbox.
    pub fn idle_start(&mut self, count: &mut Count) -> Result<bool, ImapError> {
        self.tag += 1;
        let tag = format!("m{}", self.tag);
        let tagged = format!("{tag} ");
        super::transcript::note("imap >", &format!("{tag} IDLE"));
        self.stream.write_all(format!("{tag} IDLE\r\n").as_bytes())?;
        self.stream.flush()?;
        let deadline = Instant::now() + TIMEOUT;
        let mut arrived = false;
        loop {
            let line = self
                .line_by(deadline, &|| false)?
                .ok_or_else(|| ImapError::Io("the server didn't answer IDLE".into()))?;
            if line.text.starts_with('+') {
                return Ok(arrived);
            }
            if let Some(rest) = line.text.strip_prefix(&tagged) {
                return Err(ImapError::Refused(refusal(rest)));
            }
            arrived |= count.hears(&line.text);
        }
    }

    /// Wait in `IDLE` until the server says mail arrived (a count above
    /// `count`, kept up to date meanwhile), `deadline` passes, or
    /// `should_stop`. A count it had already, or one lower, is not news:
    /// some servers say it each time a wait begins.
    pub fn idle_wait(
        &mut self,
        deadline: Instant,
        should_stop: &dyn Fn() -> bool,
        count: &mut Count,
    ) -> Result<IdleWait, ImapError> {
        let tagged = format!("m{} ", self.tag);
        loop {
            let Some(line) = self.line_by(deadline, should_stop)? else { return Ok(IdleWait::Quiet) };
            if let Some(rest) = line.text.strip_prefix(&tagged) {
                return if rest.to_ascii_uppercase().starts_with("OK") {
                    Ok(IdleWait::Ended)
                } else {
                    Err(ImapError::Refused(refusal(rest)))
                };
            }
            if count.hears(&line.text) {
                return Ok(IdleWait::Arrived);
            }
        }
    }

    /// End `IDLE`: `DONE`, and the server's tagged end, keeping `count` to
    /// what it says meanwhile. True when that says mail arrived.
    pub fn idle_done(&mut self, count: &mut Count) -> Result<bool, ImapError> {
        let tagged = format!("m{} ", self.tag);
        super::transcript::note("imap >", "DONE");
        self.stream.write_all(b"DONE\r\n")?;
        self.stream.flush()?;
        let deadline = Instant::now() + TIMEOUT;
        let mut arrived = false;
        loop {
            let line =
                self.line_by(deadline, &|| false)?.ok_or_else(|| ImapError::Io("the server didn't end IDLE".into()))?;
            if let Some(rest) = line.text.strip_prefix(&tagged) {
                return if rest.to_ascii_uppercase().starts_with("OK") {
                    Ok(arrived)
                } else {
                    Err(ImapError::Refused(refusal(rest)))
                };
            }
            arrived |= count.hears(&line.text);
        }
    }

    /// Send a command and collect every untagged line until its tagged reply.
    fn command(&mut self, cmd: &str) -> Result<Vec<Line>, ImapError> {
        self.tag += 1;
        let tag = format!("m{}", self.tag);
        super::transcript::note("imap >", &format!("{tag} {}", super::transcript::imap_command(cmd)));
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
                return Err(ImapError::Refused(refusal(rest)));
            }
            untagged.push(line);
        }
    }

    /// Sign in with an OAuth access token (SASL XOAUTH2, initial response on
    /// the command line). A server that answers with a challenge — the
    /// reason, as JSON — is sent the empty line it waits for, and then says
    /// no with the command's tag.
    pub fn authenticate_xoauth2(&mut self, username: &str, access_token: &str) -> Result<(), ImapError> {
        self.tag += 1;
        let tag = format!("m{}", self.tag);
        let response = super::oauth::xoauth2(username.trim(), access_token);
        super::transcript::note("imap >", &format!("{tag} AUTHENTICATE XOAUTH2 [the sign-in]"));
        self.stream.write_all(format!("{tag} AUTHENTICATE XOAUTH2 {response}\r\n").as_bytes())?;
        self.stream.flush()?;
        let mut challenges = 0;
        loop {
            let line = self.read_line()?;
            if line.text.starts_with('+') {
                challenges += 1;
                if challenges > 2 {
                    return Err(ImapError::Protocol("the server kept asking for more during sign-in".into()));
                }
                super::transcript::note("imap >", "(an empty line, as the server asked)");
                self.stream.write_all(b"\r\n")?;
                self.stream.flush()?;
                continue;
            }
            if let Some(rest) = line.text.strip_prefix(&format!("{tag} ")) {
                if rest.to_ascii_uppercase().starts_with("OK") {
                    return Ok(());
                }
                return Err(ImapError::SignIn(refusal(rest)));
            }
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

    /// Whether the open folder holds a message answering `message_id` — one
    /// whose `In-Reply-To` or `References` names it — among the newest
    /// `MOST_ANSWERING` it has had since `since` (the server's own date,
    /// `SINCE`), reading those two headers only (`BODY.PEEK`), so nothing is
    /// opened or marked (0.10.0-alpha.56). Not `SEARCH HEADER`: Exchange
    /// searches only the headers a message had on its way, which a Sent copy
    /// never had.
    pub fn answering(&mut self, message_id: &str, since: chrono::NaiveDate) -> Result<bool, ImapError> {
        let wanted = format!("<{}>", message_id.trim().trim_matches(['<', '>'])).to_lowercase();
        let lines = self.command(&format!("UID SEARCH SINCE {}", since.format("%-d-%b-%Y")))?;
        let mut uids: Vec<u32> = lines
            .iter()
            .filter_map(|l| l.text.strip_prefix("* SEARCH"))
            .flat_map(|r| r.split_whitespace().filter_map(|n| n.parse::<u32>().ok()).collect::<Vec<_>>())
            .collect();
        uids.sort_unstable();
        uids.dedup();
        let newest = &uids[uids.len().saturating_sub(MOST_ANSWERING)..];
        for chunk in newest.chunks(ANSWERING_CHUNK) {
            let set = chunk.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
            let lines =
                self.command(&format!("UID FETCH {set} (UID BODY.PEEK[HEADER.FIELDS (IN-REPLY-TO REFERENCES)])"))?;
            // As a literal, as servers send it, or inline in the line.
            let names = |bytes: &[u8]| String::from_utf8_lossy(bytes).to_lowercase().contains(&wanted);
            if lines.iter().any(|l| names(l.text.as_bytes()) || l.literals.iter().any(|b| names(b))) {
                return Ok(true);
            }
        }
        Ok(false)
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

    /// Put a message in a folder, marked as read: `APPEND`, the message
    /// as a literal after the server's go-ahead. Only `copy_to_sent` calls
    /// it, with a reply Mimic has just sent; nothing else in this module
    /// writes to a mailbox.
    pub fn append(&mut self, folder: &str, message: &[u8]) -> Result<(), ImapError> {
        self.tag += 1;
        let tag = format!("m{}", self.tag);
        let cmd = format!("{tag} APPEND {} (\\Seen) {{{}}}", quote(folder)?, message.len());
        super::transcript::note("imap >", &cmd);
        self.stream.write_all(format!("{cmd}\r\n").as_bytes())?;
        self.stream.flush()?;
        loop {
            let line = self.read_line()?;
            if line.text.starts_with('+') {
                break;
            }
            if let Some(rest) = line.text.strip_prefix(&format!("{tag} ")) {
                return Err(ImapError::Refused(refusal(rest)));
            }
        }
        super::transcript::note("imap >", &format!("[a message, {} bytes]", message.len()));
        self.stream.write_all(message)?;
        self.stream.write_all(b"\r\n")?;
        self.stream.flush()?;
        loop {
            let line = self.read_line()?;
            if let Some(rest) = line.text.strip_prefix(&format!("{tag} ")) {
                if rest.to_ascii_uppercase().starts_with("OK") {
                    return Ok(());
                }
                return Err(ImapError::Refused(refusal(rest)));
            }
        }
    }

    pub fn logout(&mut self) {
        let _ = self.command("LOGOUT");
    }
}

/// A literal's length, when a response line ends with one: `... {1234}`.
fn literal_len(text: &str) -> Option<usize> {
    text.strip_suffix('}')
        .and_then(|t| t.rfind('{').map(|i| &t[i + 1..]))
        .and_then(|n| n.trim_end_matches('+').parse::<usize>().ok())
}

/// How many messages the open folder holds, kept from what the server says
/// (0.10.0-alpha.44): `* n EXISTS` is the count, and each `* n EXPUNGE` one
/// fewer (RFC 3501 §7.4.1, no new count follows one). Mail arrived when a
/// count is above the one before — not when it is the same, which some
/// servers repeat each time a wait begins, nor after a message was removed
/// and another came, which a count compared with the first alone would miss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Count(pub u32);

impl Count {
    /// Take in one untagged line; true when it says mail arrived.
    fn hears(&mut self, text: &str) -> bool {
        let Some((n, word)) = text.trim().strip_prefix("* ").and_then(|t| t.split_once(' ')) else {
            return false;
        };
        if word.eq_ignore_ascii_case("EXISTS") {
            let Ok(n) = n.parse::<u32>() else { return false };
            let more = n > self.0;
            self.0 = n;
            more
        } else if word.eq_ignore_ascii_case("EXPUNGE") && n.parse::<u32>().is_ok() {
            self.0 = self.0.saturating_sub(1);
            false
        } else {
            false
        }
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

/// A folder's name as people read it: IMAP's modified UTF-7 (RFC 3501
/// §5.1.3) decoded, so `&AMk-l&AOk-ments envoy&AOk-s` is "Éléments envoyés"
/// (0.10.0-alpha.42). The name as it came is what is sent back to the
/// server; this is for matching and for showing. A name that isn't valid
/// modified UTF-7 is left as it came.
pub fn folder_name(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let Some(end) = after.find('-') else { return raw.to_string() };
        let chunk = &after[..end];
        if chunk.is_empty() {
            out.push('&');
        } else {
            let Some(units) = modified_base64(chunk) else { return raw.to_string() };
            match String::from_utf16(&units) {
                Ok(s) => out.push_str(&s),
                Err(_) => return raw.to_string(),
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Modified base64 (`,` for `/`, no padding) to UTF-16 code units; `None`
/// for anything else, or bytes that aren't whole units.
fn modified_base64(chunk: &str) -> Option<Vec<u16>> {
    let mut bytes = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in chunk.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b',' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    if bytes.len() % 2 != 0 {
        return None;
    }
    Some(bytes.chunks(2).map(|p| u16::from_be_bytes([p[0], p[1]])).collect())
}

/// Pick the folders to read: the inbox, and the sent folder by attribute
/// first, by the names the common servers use second, and by its name in
/// another language (`sources::is_sent_folder_name`) last — each name
/// matched as people read it (`folder_name`), and kept as it came.
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
            SENT_NAMES
                .iter()
                .find_map(|want| listed.iter().find(|(a, n)| folder_name(n).to_lowercase() == *want && selectable(a)))
        })
        .or_else(|| {
            // The last part of the name, for servers that keep folders under
            // the inbox (`INBOX.Gesendet`).
            let last = |n: &str| {
                let n = folder_name(n);
                n.rsplit(['/', '.']).next().unwrap_or(&n).to_string()
            };
            listed.iter().find(|(a, n)| super::is_sent_folder_name(&last(n)) && selectable(a))
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
    pub fn open(account: &ImapAccount, credential: &Credential) -> Result<Connection, ImapError> {
        // Refuse before connecting: a password or a token must never be
        // offered in the clear to anything but this computer.
        if account.security == Security::Plain && !is_loopback_host(&account.host) {
            return Err(ImapError::PlainRefused(account.host.clone()));
        }
        let addr = format!("{}:{}", account.host.trim(), account.port);
        super::transcript::note(
            "imap",
            &format!("connecting to {addr}{}", if account.security == Security::Tls { " over TLS" } else { "" }),
        );
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
        match credential {
            Credential::Password(password) => with_session!(&mut conn, s => s.login(&account.username, password))?,
            Credential::AccessToken(token) => {
                with_session!(&mut conn, s => s.authenticate_xoauth2(&account.username, token))?
            }
        }
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
    pub fn answering(&mut self, message_id: &str, since: chrono::NaiveDate) -> Result<bool, ImapError> {
        with_session!(self, s => s.answering(message_id, since))
    }
    pub fn fetch(&mut self, uids: &[u32]) -> Result<Vec<(u32, Vec<u8>)>, ImapError> {
        with_session!(self, s => s.fetch(uids))
    }
    pub fn sizes(&mut self, uids: &[u32]) -> Result<Vec<(u32, u64)>, ImapError> {
        with_session!(self, s => s.sizes(uids))
    }
    pub fn append(&mut self, folder: &str, message: &[u8]) -> Result<(), ImapError> {
        with_session!(self, s => s.append(folder, message))
    }
    pub fn logout(&mut self) {
        with_session!(self, s => s.logout())
    }
    pub fn capabilities(&mut self) -> Result<Vec<String>, ImapError> {
        with_session!(self, s => s.capabilities())
    }

    /// Wait in `IDLE` (RFC 2177) in the open folder: until the server says
    /// mail arrived (`Count`, kept up to date), `wait` has passed, or
    /// `should_stop`; then `DONE`, and the server's end of it. Whether mail
    /// arrived, and whether the server ended the wait itself.
    ///
    /// The wait reads without blocking, with a short pause between reads, so
    /// a stop is noticed while nothing arrives. Not a read timeout: Windows
    /// says a socket whose read timed out is in an indeterminate state and
    /// shouldn't be used again. Sending `IDLE` and `DONE`, and reading the
    /// answers, block as every other command does.
    pub fn idle(
        &mut self,
        wait: Duration,
        should_stop: &dyn Fn() -> bool,
        count: &mut Count,
    ) -> Result<(bool, bool), ImapError> {
        let until = Instant::now() + wait;
        let mut arrived = with_session!(self, s => s.idle_start(count))?;
        if !arrived {
            self.socket().set_nonblocking(true)?;
            let waited = with_session!(self, s => s.idle_wait(until, should_stop, count));
            let restored = self.socket().set_nonblocking(false);
            let waited = waited?;
            restored?;
            match waited {
                // The server ended it: there is nothing to end.
                IdleWait::Ended => return Ok((false, true)),
                IdleWait::Arrived => arrived = true,
                IdleWait::Quiet => {}
            }
        }
        let said = with_session!(self, s => s.idle_done(count))?;
        Ok((arrived || said, false))
    }

    fn socket(&self) -> &TcpStream {
        match self {
            Connection::Tls(s) => &s.stream.sock,
            Connection::Plain(s) => &s.stream,
        }
    }
}

// ----------------------------------------------------------------- watching

/// How long a wait in `IDLE` pauses between reads that found nothing, and so
/// how soon it notices a stop.
const IDLE_PAUSE: Duration = Duration::from_millis(100);

/// How a wait in `IDLE` ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleWait {
    /// The server said mail arrived.
    Arrived,
    /// The wait was over, or told to stop.
    Quiet,
    /// The server ended the `IDLE` itself.
    Ended,
}

/// How long a watch waits in one `IDLE`, and how long it keeps one
/// connection.
#[derive(Debug, Clone, Copy)]
pub struct Watching {
    /// One `IDLE` is ended and begun again after this: far inside the 29
    /// minutes RFC 2177 allows, and inside the few minutes after which a home
    /// router may forget a quiet connection.
    pub renew: Duration,
    /// A watch ends after this, for the caller to connect again with a
    /// credential that is still good: a sign-in's access token lasts about an
    /// hour, and a server may close a connection signed in with one that ran out.
    pub session: Duration,
}

impl Watching {
    pub const USUAL: Watching = Watching { renew: Duration::from_secs(4 * 60), session: Duration::from_secs(45 * 60) };
}

/// How a watch ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Watched {
    /// `should_stop` said so.
    Stopped,
    /// `Watching.session` passed: connect again.
    Renew,
    /// The server can't say when mail arrives (no `IDLE`): the schedule is all there is.
    NoIdle,
}

/// Watch a folder for new mail (IMAP `IDLE`, RFC 2177), calling `arrived`
/// each time the server says some came, so a check can be queued at once
/// rather than at the next scheduled one. It opens the folder with `EXAMINE`
/// and reads nothing: no message, no flag — the check does the reading.
/// `listening` is told true once the server is listening for it, and false
/// when the watch ends, however it ends (0.10.0-alpha.30: the home screen says
/// so only while it is true).
pub fn watch(
    account: &ImapAccount,
    folder: &str,
    credential: &Credential,
    times: Watching,
    should_stop: &dyn Fn() -> bool,
    listening: &dyn Fn(bool),
    arrived: &mut dyn FnMut(),
) -> Result<Watched, ImapError> {
    let watched = watch_inner(account, folder, credential, times, should_stop, listening, arrived);
    listening(false);
    watched
}

fn watch_inner(
    account: &ImapAccount,
    folder: &str,
    credential: &Credential,
    times: Watching,
    should_stop: &dyn Fn() -> bool,
    listening: &dyn Fn(bool),
    arrived: &mut dyn FnMut(),
) -> Result<Watched, ImapError> {
    let mut conn = Connection::open(account, credential)?;
    if !conn.capabilities()?.iter().any(|c| c == "IDLE") {
        conn.logout();
        return Ok(Watched::NoIdle);
    }
    let (_, exists) = conn.examine(folder)?;
    let mut count = Count(exists);
    listening(true);
    let started = Instant::now();
    let mut quick = 0;
    loop {
        if should_stop() {
            conn.logout();
            return Ok(Watched::Stopped);
        }
        let left = times.session.saturating_sub(started.elapsed());
        if left.is_zero() {
            conn.logout();
            return Ok(Watched::Renew);
        }
        let round = Instant::now();
        let (came, ended) = conn.idle(left.min(times.renew), should_stop, &mut count)?;
        if came {
            arrived();
        }
        // A server that ends every wait at once would have it begun again
        // without a pause: a few in a row and the watch is given up, for the
        // caller's growing pause.
        if ended && round.elapsed() < QUICK_ROUND {
            quick += 1;
            if quick >= QUICK_ROUNDS {
                conn.logout();
                return Err(ImapError::Protocol("the server keeps ending its wait at once".into()));
            }
        } else {
            quick = 0;
        }
    }
}

/// A wait in `IDLE` the server ends sooner than this is one ended at once;
/// `QUICK_ROUNDS` of them in a row end the watch.
const QUICK_ROUND: Duration = Duration::from_secs(2);
const QUICK_ROUNDS: u32 = 5;

/// Put a copy of a reply Mimic sent in the mailbox's Sent folder, marked as
/// read, for a provider that doesn't keep one itself (`smtp::keeps_sent`):
/// the one write to a mailbox Mimic makes, and only of what it sent.
pub fn copy_to_sent(config: &ImapSourceConfig, credential: &Credential, message: &[u8]) -> Result<(), ImapError> {
    let mut conn = Connection::open(&config.account, credential)?;
    let sent = match &config.sent_folder {
        Some(folder) => Some(folder.clone()),
        None => choose_folders(&conn.list()?).1,
    };
    let Some(sent) = sent else {
        conn.logout();
        return Err(ImapError::Refused("this mailbox has no Sent folder I could find".into()));
    };
    let appended = conn.append(&sent, message);
    conn.logout();
    appended
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

/// Whether `folder` holds a reply to `message_id` — the Sent folder, asked
/// just before a reply goes by itself, so one sent from Mimic on another
/// computer, or by the user since the last check, is seen (0.10.0-alpha.56).
/// Read-only: `EXAMINE`, a search by date and two headers of what it finds
/// (`Session::answering`); nothing is opened.
pub fn reply_in_folder(
    account: &ImapAccount,
    credential: &Credential,
    folder: &str,
    message_id: &str,
    since: chrono::NaiveDate,
) -> Result<bool, ImapError> {
    let mut conn = Connection::open(account, credential)?;
    conn.examine(folder)?;
    let found = conn.answering(message_id, since)?;
    conn.logout();
    Ok(found)
}

pub fn probe(account: &ImapAccount, credential: &Credential) -> Result<ImapProbe, ImapError> {
    let mut conn = Connection::open(account, credential)?;
    let listed = conn.list()?;
    let (folders, sent_folder) = choose_folders(&listed);
    let mut counts = Vec::new();
    for f in &folders {
        let (_, exists) = conn.examine(f)?;
        // As people read it; `folders` keeps it as the server gave it.
        counts.push((folder_name(f), exists));
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
    credential: &Credential,
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
    let began = crate::ids::now_rfc3339();
    // What was said to the server is kept with a failure, to show why
    // (`transcript`: never a password, a token or a message).
    let (result, said) =
        super::transcript::recording(|| sync_inner(db, source_id, &mut config, credential, on_progress, should_stop));
    // Every attempt is recorded, successful or not, so the schedule waits a
    // whole interval before the next one rather than retrying every minute.
    config.last_checked_at = Some(crate::ids::now_rfc3339());
    if result.is_ok() {
        config.last_read_at = Some(began);
    }
    // How replies go out, and what they are signed with, may have been
    // changed while this check ran; that is the user's, and is kept rather
    // than written over with what was read at the start.
    if let Some(latest) =
        db.get_source(source_id)?.and_then(|s| serde_json::from_value::<ImapSourceConfig>(s.config).ok())
    {
        config.sending = latest.sending;
        config.signature = latest.signature;
    }
    db.set_source_config(source_id, &serde_json::to_value(&config).unwrap_or(Value::Null))?;
    match &result {
        Ok(_) => db.set_source_status(source_id, "imported", None)?,
        Err(ImapError::Canceled) => db.set_source_status(source_id, "ready", None)?,
        Err(e) => {
            db.set_source_status(source_id, "failed", Some(&json!({ "message": e.to_string(), "transcript": said })))?
        }
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
    #[error("This mailbox signs in with {0}, not a password. Sign in again instead.")]
    SignsIn(&'static str),
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
    if let Some(provider) = config.account.auth.provider() {
        return Err(PasswordError::SignsIn(provider.name));
    }
    probe(&config.account, &Credential::Password(password.to_string()))?;
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
    credential: &Credential,
    on_progress: &mut dyn FnMut(usize, &str),
    should_stop: &dyn Fn() -> bool,
) -> Result<SyncSummary, ImapError> {
    let mut conn = Connection::open(&config.account, credential)?;
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
        if let Err(e) = importer.take(convo) {
            // What was written stays; what it changed is measured again.
            importer.abandon();
            return Err(e.into());
        }
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
/// Checking more often than this is not polite to a mail server. Two
/// minutes since 0.10.0-alpha.24, when a reply sent by itself began to wait
/// for the mailbox to be read again.
pub const MIN_INTERVAL_MINUTES: i64 = 2;
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
    /// How many are being listened to for mail as it arrives (`watch`). Only
    /// the app knows; `checking` says 0, and the desktop fills it in.
    #[serde(default)]
    pub listening: usize,
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
        listening: 0,
        every_minutes: interval_minutes(db)?,
        last_checked_at: last,
        failing,
    }))
}

// ------------------------------------------------------------------- job

pub const JOB_KIND: &str = "check_mailbox";

/// Reads a mailbox's password, or refresh token, from wherever the app keeps
/// secrets.
pub type PasswordFor = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;
/// Saves a secret: a refresh token the provider replaced when it was used.
pub type SaveSecret = Arc<dyn Fn(&str, &str) -> Result<(), String> + Send + Sync>;

/// What mailboxes sign in with, worked out per check.
pub struct Credentials {
    password_for: PasswordFor,
    save: SaveSecret,
    /// Each provider a mailbox may sign in with, and the client id it signs
    /// in as (none: it can't) — this build's, or the user's own registration
    /// saved since (0.10.0-alpha.59, `register`).
    sign_ins: std::sync::RwLock<Vec<(Auth, super::oauth::Provider, Option<String>)>>,
    /// Access tokens still good, per mailbox, and when to stop trusting
    /// each: a check every few minutes needn't ask for a new one every time.
    kept: std::sync::Mutex<HashMap<String, (String, std::time::Instant)>>,
}

impl Credentials {
    pub fn new(
        password_for: PasswordFor,
        save: SaveSecret,
        microsoft: super::oauth::Provider,
        microsoft_client: Option<String>,
    ) -> Self {
        Credentials {
            password_for,
            save,
            sign_ins: std::sync::RwLock::new(vec![(Auth::Microsoft, microsoft, microsoft_client)]),
            kept: Default::default(),
        }
    }

    /// Mailboxes that sign in with `auth` do it with `provider`, and this
    /// client id (none: this build can't).
    pub fn signing_in_with(self, auth: Auth, provider: super::oauth::Provider, client: Option<String>) -> Self {
        self.register(auth, provider, client);
        self
    }

    /// From now on, mailboxes that sign in with `auth` do it with `provider`
    /// and this client id (none: they can't). A sign-in made with another
    /// client id can't be renewed with this one: the provider asks for a new
    /// sign-in, which the mailbox says.
    pub fn register(&self, auth: Auth, provider: super::oauth::Provider, client: Option<String>) {
        let mut sign_ins = self.sign_ins.write().unwrap_or_else(|p| p.into_inner());
        sign_ins.retain(|(a, _, _)| *a != auth);
        sign_ins.push((auth, provider, client));
    }

    /// The provider and client id a new sign-in with `auth` uses, when there
    /// is one.
    pub fn sign_in_with(&self, auth: Auth) -> Option<(super::oauth::Provider, String)> {
        let sign_ins = self.sign_ins.read().unwrap_or_else(|p| p.into_inner());
        sign_ins.iter().find(|(a, _, _)| *a == auth).and_then(|(_, p, c)| Some((p.clone(), c.clone()?)))
    }

    /// What a mailbox signs in with now: its password; or, for one signed in
    /// with Microsoft or Google, an access token — kept from an earlier check while it
    /// has at least five minutes left, or else traded for the refresh token,
    /// with the refresh token the provider hands back saved in its place.
    /// `Err` is the sentence the mailbox shows.
    pub fn credential(&self, auth: Auth, source_id: &str) -> Result<Credential, String> {
        let key = secret_key(source_id);
        match auth {
            Auth::Password => (self.password_for)(&key).map(Credential::Password).ok_or_else(|| {
                "I don't have a password I can use for this mailbox. Give it to me again with New password, under Your mail.".to_string()
            }),
            Auth::Microsoft | Auth::Google => {
                let now = std::time::Instant::now();
                let mut kept = self.kept.lock().unwrap_or_else(|p| p.into_inner());
                if let Some((token, until)) = kept.get(source_id) {
                    if *until > now {
                        return Ok(Credential::AccessToken(token.clone()));
                    }
                }
                let found = {
                    let sign_ins = self.sign_ins.read().unwrap_or_else(|p| p.into_inner());
                    sign_ins.iter().find(|(a, _, _)| *a == auth).map(|(_, p, c)| (p.clone(), c.clone()))
                };
                let Some((provider, client)) = found else {
                    return Err("This copy of Mimic can't sign in to this mailbox, so I can't reach it.".into());
                };
                let name = provider.name;
                let Some(client) = client.as_deref() else {
                    return Err(format!(
                        "This copy of Mimic can't sign in with {name}, so I can't reach this mailbox. Settings, under Signing in with Microsoft and Google, says how to set that up."
                    ));
                };
                let Some(refresh) = (self.password_for)(&key) else {
                    return Err(format!(
                        "I don't have a {name} sign-in I can use for this mailbox. Sign in again under Your mail."
                    ));
                };
                let tokens = super::oauth::refresh(&provider, client, &refresh).map_err(|e| match e {
                    super::oauth::OAuthError::SignInAgain(who) => {
                        format!("{who} wants you to sign in again for this mailbox. Do it under Your mail.")
                    }
                    other => other.to_string(),
                })?;
                if let Some(next) = tokens.refresh_token.as_deref().filter(|t| *t != refresh) {
                    // The old one may stop working now a new one is issued.
                    // One that could not be kept is said, never shown.
                    if let Err(e) = (self.save)(&key, next) {
                        tracing::warn!(target: "secrets", error = %e, "a mailbox's new sign-in could not be saved");
                    }
                }
                let lasts = std::time::Duration::from_secs(tokens.expires_in.saturating_sub(300));
                kept.insert(source_id.to_string(), (tokens.access_token.clone(), now + lasts));
                Ok(Credential::AccessToken(tokens.access_token))
            }
        }
    }

    /// Forget a mailbox's kept access token: the server turned it down, or
    /// the mailbox was signed in again.
    pub fn forget(&self, source_id: &str) {
        self.kept.lock().unwrap_or_else(|p| p.into_inner()).remove(source_id);
    }

    /// Change what a mailbox signs in with — keep a new sign-in, or remove
    /// the saved one with the mailbox — once no check is trading the old one
    /// for a token, and forget the token that came from it. Without the wait,
    /// a check finishing a refresh would save what it was handed over the
    /// new sign-in, or bring back a removed mailbox's.
    pub fn settle<R>(&self, source_id: &str, change: impl FnOnce() -> R) -> R {
        let mut kept = self.kept.lock().unwrap_or_else(|p| p.into_inner());
        let changed = change();
        kept.remove(source_id);
        changed
    }
}

pub struct CheckMailboxExecutor {
    credentials: Arc<Credentials>,
}

impl CheckMailboxExecutor {
    pub fn shared(credentials: Arc<Credentials>) -> Arc<dyn crate::jobs::JobExecutor> {
        Arc::new(CheckMailboxExecutor { credentials })
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
        let credentials = self.credentials.clone();
        Box::pin(async move {
            let source_id = ctx.job.payload["sourceId"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| crate::jobs::JobError::Failed("no mailbox was named".into()))?;
            let auth = ctx
                .db
                .get_source(&source_id)?
                .and_then(|s| serde_json::from_value::<ImapSourceConfig>(s.config).ok())
                .map(|c| c.account.auth)
                .unwrap_or_default();
            // A Microsoft mailbox may need a new access token first, which is
            // a request of its own.
            let credential = match tokio::task::block_in_place(|| credentials.credential(auth, &source_id)) {
                Ok(c) => c,
                Err(why) => {
                    record_failure(&ctx.db, &source_id, &why)?;
                    return Err(crate::jobs::JobError::Failed(why));
                }
            };
            let db = ctx.db.clone();
            let progress_ctx = ctx.clone();
            let stop_ctx = ctx.clone();
            let summary = tokio::task::block_in_place(|| {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    sync(
                        &db,
                        &source_id,
                        &credential,
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
                Err(e) => {
                    // A token the server turned down is not tried again.
                    if matches!(e, ImapError::SignIn(_)) {
                        credentials.forget(&source_id);
                    }
                    Err(crate::jobs::JobError::Failed(e.to_string()))
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pw(password: &str) -> Credential {
        Credential::Password(password.into())
    }
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
        let in_german = vec![(vec![], "INBOX".to_string()), (vec![], "Gesendete Elemente".to_string())];
        assert_eq!(choose_folders(&in_german).1.as_deref(), Some("Gesendete Elemente"));
        let nested = vec![(vec![], "INBOX".to_string()), (vec![], "INBOX.Verzonden".to_string())];
        assert_eq!(choose_folders(&nested).1.as_deref(), Some("INBOX.Verzonden"));

        let none = vec![(vec![], "INBOX".to_string()), (vec!["\\noselect".into()], "Sent".to_string())];
        assert_eq!(
            choose_folders(&none),
            (vec!["INBOX".into()], None),
            "a folder that cannot be opened is not a sent folder"
        );
    }

    #[test]
    fn a_folder_named_in_another_language_is_read_as_people_read_it_and_kept_as_it_came() {
        assert_eq!(folder_name("&AMk-l&AOk-ments envoy&AOk-s"), "Éléments envoyés");
        assert_eq!(folder_name("INBOX.Gel&APY-schte Elemente"), "INBOX.Gelöschte Elemente");
        assert_eq!(folder_name("Tom &- Jerry"), "Tom & Jerry");
        assert_eq!(folder_name("&ZeVnLIqe-"), "日本語");
        assert_eq!(folder_name("INBOX"), "INBOX");
        for broken in ["&AMk", "&A*k-", "&AM-"] {
            assert_eq!(folder_name(broken), broken, "left as it came");
        }

        let listed = vec![
            (vec!["\\haschildren".to_string()], "INBOX".to_string()),
            (vec![], "&AMk-l&AOk-ments supprim&AOk-s".to_string()),
            (vec![], "&AMk-l&AOk-ments envoy&AOk-s".to_string()),
        ];
        let (folders, sent) = choose_folders(&listed);
        assert_eq!(sent.as_deref(), Some("&AMk-l&AOk-ments envoy&AOk-s"), "sent back to the server as it came");
        assert_eq!(folders, vec!["INBOX".to_string(), "&AMk-l&AOk-ments envoy&AOk-s".to_string()]);
        let under_inbox = vec![(vec![], "INBOX".to_string()), (vec![], "INBOX.Gesendete Objekte".to_string())];
        assert_eq!(choose_folders(&under_inbox).1.as_deref(), Some("INBOX.Gesendete Objekte"));
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
        let account = ImapAccount {
            host: "imap.example.com".into(),
            port: 143,
            username: "c".into(),
            security: Security::Plain,
            auth: Auth::Password,
        };
        assert!(matches!(Connection::open(&account, &pw("secret")), Err(ImapError::PlainRefused(_))));
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
                    } else if let Some(arg) = cmd.strip_prefix("AUTHENTICATE XOAUTH2 ") {
                        // `password` is the access token this server takes.
                        let said = String::from_utf8_lossy(&crate::sources::mime::decode_base64(arg)).into_owned();
                        if said.starts_with("user=") && said.ends_with(&format!("\x01auth=Bearer {password}\x01\x01")) {
                            w.write_all(format!("{tag} OK AUTHENTICATE completed.\r\n").as_bytes()).unwrap();
                        } else {
                            // The reason as a challenge, as Gmail does; the
                            // client answers with an empty line.
                            w.write_all(b"+ eyJzdGF0dXMiOiI0MDEifQ==\r\n").unwrap();
                            let mut answer = String::new();
                            r.read_line(&mut answer).unwrap();
                            seen.push(format!("(challenge answered with {:?})", answer.trim_end()));
                            w.write_all(format!("{tag} NO AUTHENTICATE failed.\r\n").as_bytes()).unwrap();
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
                    } else if upper.starts_with("APPEND") {
                        // `APPEND "Sent" (\Seen) {n}`: the go-ahead, then n
                        // bytes and a line ending.
                        let n: usize = cmd.rsplit('{').next().unwrap().trim_end_matches('}').parse().unwrap();
                        w.write_all(b"+ Ready for literal data\r\n").unwrap();
                        let mut body = vec![0u8; n];
                        std::io::Read::read_exact(&mut r, &mut body).unwrap();
                        let mut end = String::new();
                        r.read_line(&mut end).unwrap();
                        seen.push(format!("(appended {})", String::from_utf8_lossy(&body)));
                        w.write_all(format!("{tag} OK [APPENDUID 1 99] APPEND completed\r\n").as_bytes()).unwrap();
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
                auth: Auth::Password,
            },
            folders: vec![],
            sent_folder: None,
            marks: BTreeMap::new(),
            last_checked_at: None,
            last_read_at: None,
            sending: None,
            signature: None,
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

        let first = sync(&db, &source, &pw("app-pass"), &mut |_, _| {}, &|| false).unwrap();
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
        let second = sync(&db, &source, &pw("app-pass"), &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(second.fetched, 0);
        assert_eq!(second.inserted, 0);
    }

    /// Credentials for tests: secrets in memory, saves recorded, Microsoft's
    /// token endpoint replaced with `token_url`.
    /// Every (key, value) a `Credentials` saved, in order.
    type Saved = Arc<std::sync::Mutex<Vec<(String, String)>>>;

    fn credentials(secrets: Vec<(String, String)>, token_url: &str, client: Option<&str>) -> (Credentials, Saved) {
        let store: Arc<std::sync::Mutex<HashMap<String, String>>> =
            Arc::new(std::sync::Mutex::new(secrets.into_iter().collect()));
        let saved = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (read, write, log) = (store.clone(), store, saved.clone());
        let creds = Credentials::new(
            Arc::new(move |k: &str| read.lock().unwrap().get(k).cloned()),
            Arc::new(move |k: &str, v: &str| {
                write.lock().unwrap().insert(k.into(), v.into());
                log.lock().unwrap().push((k.into(), v.into()));
                Ok(())
            }),
            super::super::oauth::Provider { token_url: token_url.into(), ..super::super::oauth::Provider::microsoft() },
            client.map(str::to_string),
        );
        (creds, saved)
    }

    #[test]
    fn a_mailbox_signed_in_with_microsoft_is_read_with_a_token_and_never_a_password() {
        let (url, forms) = super::super::oauth::fake::token_endpoint(vec![(
            200,
            r#"{"access_token":"at-1","refresh_token":"rt-2","expires_in":3599}"#.into(),
        )]);
        let inbox = Mailbox {
            name: "INBOX",
            validity: 1,
            messages: vec![(1, mail("q@x", "Ada <ada@example.com>", None, "Mon, 2 Mar 2026 09:00:00 +0000", "hello?"))],
        };
        let (port, seen) = serve(vec![inbox, Mailbox { name: "Sent", validity: 1, messages: vec![] }], "at-1");
        let (db, source) = db_for(port);
        let mut config: ImapSourceConfig =
            serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        config.account.auth = Auth::Microsoft;
        db.set_source_config(&source, &serde_json::to_value(&config).unwrap()).unwrap();

        let (creds, saved) = credentials(vec![(secret_key(&source), "rt-1".into())], &url, Some("client-1"));
        let credential = creds.credential(Auth::Microsoft, &source).unwrap();
        assert!(matches!(&credential, Credential::AccessToken(t) if t == "at-1"));
        assert!(!format!("{credential:?}").contains("at-1"), "a credential never prints");
        let form = forms.recv().unwrap();
        assert!(form.contains("grant_type=refresh_token") && form.contains("refresh_token=rt-1"), "{form}");
        assert_eq!(
            *saved.lock().unwrap(),
            vec![(secret_key(&source), "rt-2".to_string())],
            "the new refresh token is kept"
        );

        let s = sync(&db, &source, &credential, &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(s.inserted, 1);
        let commands = seen.recv().unwrap();
        assert!(commands.iter().any(|c| c.contains("AUTHENTICATE XOAUTH2 ")), "{commands:?}");
        assert!(!commands.iter().any(|c| c.to_ascii_uppercase().contains("LOGIN")), "no password is ever sent");

        // The next check within the hour uses the token it has: the token
        // endpoint, which answers once, is not asked again.
        assert!(matches!(creds.credential(Auth::Microsoft, &source), Ok(Credential::AccessToken(t)) if t == "at-1"));
        creds.forget(&source);
        assert!(creds.credential(Auth::Microsoft, &source).is_err(), "forgotten, it has to ask again");
    }

    #[test]
    fn a_token_the_server_turns_down_asks_for_a_new_sign_in() {
        let (port, seen) = serve(vec![Mailbox { name: "INBOX", validity: 1, messages: vec![] }], "at-good");
        let (db, source) = db_for(port);
        let err =
            sync(&db, &source, &Credential::AccessToken("at-stale".into()), &mut |_, _| {}, &|| false).unwrap_err();
        assert!(matches!(&err, ImapError::SignIn(why) if why == "AUTHENTICATE failed."), "{err:?}");
        assert!(err.to_string().contains("Sign in again under Your mail"));
        let commands = seen.recv().unwrap();
        assert!(commands.contains(&"(challenge answered with \"\")".to_string()), "{commands:?}");
        assert_eq!(db.get_source(&source).unwrap().unwrap().status, "failed");
    }

    #[test]
    fn credentials_say_what_is_missing_and_what_to_do() {
        let (url, _forms) = super::super::oauth::fake::token_endpoint(vec![(
            400,
            r#"{"error":"invalid_grant","error_description":"AADSTS70008: expired"}"#.into(),
        )]);
        let (none, _) = credentials(vec![], &url, Some("client-1"));
        assert!(none.credential(Auth::Password, "s1").unwrap_err().contains("New password"));
        assert!(none.credential(Auth::Microsoft, "s1").unwrap_err().contains("Sign in again under Your mail"));
        let (unconfigured, _) = credentials(vec![(secret_key("s1"), "rt".into())], &url, None);
        assert!(unconfigured.credential(Auth::Microsoft, "s1").unwrap_err().contains("can't sign in with Microsoft"));
        let (lapsed, saved) = credentials(vec![(secret_key("s1"), "rt".into())], &url, Some("client-1"));
        assert_eq!(
            lapsed.credential(Auth::Microsoft, "s1").unwrap_err(),
            "Microsoft wants you to sign in again for this mailbox. Do it under Your mail."
        );
        assert!(saved.lock().unwrap().is_empty());
        let (with_password, _) = credentials(vec![(secret_key("s1"), "app-pass".into())], &url, None);
        assert!(
            matches!(with_password.credential(Auth::Password, "s1"), Ok(Credential::Password(p)) if p == "app-pass")
        );
    }

    #[test]
    fn a_new_sign_in_takes_the_place_of_the_old_one_and_of_its_token() {
        let (url, forms) = super::super::oauth::fake::token_endpoint(vec![
            (200, r#"{"access_token":"at-1","refresh_token":"rt-2","expires_in":3599}"#.into()),
            (200, r#"{"access_token":"at-9","expires_in":3599}"#.into()),
        ]);
        let (creds, saved) = credentials(vec![(secret_key("s1"), "rt-1".into())], &url, Some("client-1"));
        assert!(matches!(creds.credential(Auth::Microsoft, "s1"), Ok(Credential::AccessToken(t)) if t == "at-1"));
        assert!(forms.recv().unwrap().contains("refresh_token=rt-1"));

        let kept = creds.settle("s1", || (creds.save)(&secret_key("s1"), "rt-new"));
        assert!(kept.is_ok());
        // The token from the old sign-in is not used again: the new sign-in
        // is traded for one.
        assert!(matches!(creds.credential(Auth::Microsoft, "s1"), Ok(Credential::AccessToken(t)) if t == "at-9"));
        let form = forms.recv().unwrap();
        assert!(form.contains("refresh_token=rt-new"), "{form}");
        assert_eq!(
            *saved.lock().unwrap(),
            vec![(secret_key("s1"), "rt-2".to_string()), (secret_key("s1"), "rt-new".to_string())],
            "no refresh token was handed back the second time, so the new sign-in stays"
        );
    }

    #[test]
    fn a_microsoft_mailbox_takes_no_password() {
        let (port, _) = serve(vec![Mailbox { name: "INBOX", validity: 1, messages: vec![] }], "pw");
        let (db, source) = db_for(port);
        let mut config: ImapSourceConfig =
            serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        config.account.auth = Auth::Microsoft;
        db.set_source_config(&source, &serde_json::to_value(&config).unwrap()).unwrap();
        let err = replace_password(&db, &source, "pw", |_| Ok(())).unwrap_err();
        assert!(matches!(err, PasswordError::SignsIn("Microsoft")));
        config.account.auth = Auth::Google;
        db.set_source_config(&source, &serde_json::to_value(&config).unwrap()).unwrap();
        let err = replace_password(&db, &source, "pw", |_| Ok(())).unwrap_err();
        assert_eq!(err.to_string(), "This mailbox signs in with Google, not a password. Sign in again instead.");
    }

    #[test]
    fn a_mailbox_signed_in_with_google_is_refreshed_with_googles_client_and_keeps_its_refresh_token() {
        let (url, forms) = super::super::oauth::fake::token_endpoint(vec![
            // Google hands back no new refresh token.
            (200, r#"{"access_token":"g-at-1","expires_in":3599,"scope":"https://mail.google.com/"}"#.into()),
            (400, r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#.into()),
        ]);
        let (creds, saved) = credentials(vec![(secret_key("s1"), "g-rt".into())], &url, Some("ms-client"));
        let creds = creds.signing_in_with(
            Auth::Google,
            super::super::oauth::Provider {
                token_url: url.clone(),
                client_secret: Some("g-secret".into()),
                ..super::super::oauth::Provider::google()
            },
            Some("g-client".into()),
        );
        assert!(matches!(creds.credential(Auth::Google, "s1"), Ok(Credential::AccessToken(t)) if t == "g-at-1"));
        let form = forms.recv().unwrap();
        for part in ["client_id=g-client", "client_secret=g-secret", "refresh_token=g-rt"] {
            assert!(form.contains(part), "{part} in {form}");
        }
        assert!(saved.lock().unwrap().is_empty(), "the refresh token it has is kept as it is");
        creds.forget("s1");
        assert_eq!(
            creds.credential(Auth::Google, "s1").unwrap_err(),
            "Google wants you to sign in again for this mailbox. Do it under Your mail."
        );

        let (without, _) = credentials(vec![(secret_key("s1"), "g-rt".into())], &url, Some("ms-client"));
        assert_eq!(
            without
                .signing_in_with(Auth::Google, super::super::oauth::Provider::google(), None)
                .credential(Auth::Google, "s1")
                .unwrap_err(),
            "This copy of Mimic can't sign in with Google, so I can't reach this mailbox. Settings, under Signing in with Microsoft and Google, says how to set that up."
        );
        let (unregistered, _) = credentials(vec![(secret_key("s1"), "g-rt".into())], &url, Some("ms-client"));
        assert!(unregistered.credential(Auth::Google, "s1").unwrap_err().contains("can't sign in to this mailbox"));
    }

    /// A server to watch: takes any LOGIN, says whether it can IDLE, and in
    /// its first IDLE says a message arrived after a moment; later ones say
    /// nothing until DONE. Records what it was sent.
    fn idle_server(can_idle: bool) -> (u16, std::sync::mpsc::Receiver<Vec<String>>) {
        idle_server_saying(can_idle, "* 4 EXISTS", false)
    }

    /// An IDLE server that says `first` in the first wait (the folder holds
    /// 3), or ends every wait at once.
    fn idle_server_saying(
        can_idle: bool,
        first: &'static str,
        ends_at_once: bool,
    ) -> (u16, std::sync::mpsc::Receiver<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                // Each line as it is written: the client waits on them.
                stream.set_nodelay(true).unwrap();
                let mut seen = Vec::new();
                let mut w = stream.try_clone().unwrap();
                let mut r = BufReader::new(stream);
                w.write_all(b"* OK ready\r\n").unwrap();
                let mut idles = 0;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    let line = line.trim_end().to_string();
                    seen.push(line.clone());
                    let (tag, cmd) = line.split_once(' ').unwrap();
                    let upper = cmd.to_ascii_uppercase();
                    if upper.starts_with("LOGOUT") {
                        w.write_all(format!("* BYE\r\n{tag} OK bye\r\n").as_bytes()).unwrap();
                        break;
                    }
                    let reply = if upper.starts_with("LOGIN") {
                        format!("{tag} OK logged in\r\n")
                    } else if upper == "CAPABILITY" {
                        let idle = if can_idle { " IDLE" } else { "" };
                        format!("* CAPABILITY IMAP4rev1{idle} AUTH=XOAUTH2\r\n{tag} OK done\r\n")
                    } else if upper.starts_with("EXAMINE") {
                        format!("* 3 EXISTS\r\n* OK [UIDVALIDITY 1] ok\r\n{tag} OK [READ-ONLY] done\r\n")
                    } else if upper.starts_with("UID SEARCH SINCE ") {
                        format!("* SEARCH 5 6\r\n* SEARCH 7\r\n{tag} OK done\r\n")
                    } else if upper.starts_with("UID FETCH ")
                        && upper.contains("BODY.PEEK[HEADER.FIELDS (IN-REPLY-TO REFERENCES)]")
                    {
                        // 5 answers nothing here; 6 by its References, folded;
                        // 7 by its In-Reply-To, sent inline.
                        let six = "References: <first@x.example>\r\n <Answered@x.example>\r\n\r\n";
                        format!(
                            "* 1 FETCH (UID 5 BODY[HEADER.FIELDS (IN-REPLY-TO REFERENCES)] {{4}}\r\n\r\n\r\n)\r\n\
                             * 2 FETCH (UID 6 BODY[HEADER.FIELDS (IN-REPLY-TO REFERENCES)] {{{}}}\r\n{six})\r\n\
                             * 3 FETCH (UID 7 BODY[HEADER.FIELDS (IN-REPLY-TO REFERENCES)] \"In-Reply-To: <late@x.example>\")\r\n\
                             {tag} OK done\r\n",
                            six.len()
                        )
                    } else if upper == "IDLE" && ends_at_once {
                        w.write_all(format!("+ idling\r\n{tag} OK IDLE terminated\r\n").as_bytes()).unwrap();
                        continue;
                    } else if upper == "IDLE" {
                        idles += 1;
                        w.write_all(b"+ idling\r\n").unwrap();
                        if idles == 1 {
                            std::thread::sleep(Duration::from_millis(300));
                            w.write_all(format!("{first}\r\n").as_bytes()).unwrap();
                        } else {
                            w.write_all(b"* OK Still here\r\n").unwrap();
                        }
                        let mut done = String::new();
                        if r.read_line(&mut done).unwrap_or(0) == 0 {
                            break;
                        }
                        seen.push(done.trim_end().to_string());
                        format!("{tag} OK IDLE terminated\r\n")
                    } else {
                        format!("{tag} BAD unknown\r\n")
                    };
                    w.write_all(reply.as_bytes()).unwrap();
                }
                let _ = tx.send(seen);
            }
        });
        (port, rx)
    }

    fn here(port: u16) -> ImapAccount {
        ImapAccount {
            host: "127.0.0.1".into(),
            port,
            username: "c@example.com".into(),
            security: Security::Plain,
            auth: Auth::Password,
        }
    }

    /// Only these are ever sent while watching: nothing that reads a message
    /// or changes anything.
    fn only_watching(commands: &[String]) {
        for c in commands {
            let verb = c.split_whitespace().nth(1).unwrap_or(c).to_ascii_uppercase();
            assert!(["LOGIN", "CAPABILITY", "EXAMINE", "IDLE", "DONE", "LOGOUT"].contains(&verb.as_str()), "{c}");
        }
    }

    #[test]
    fn a_watched_inbox_says_when_mail_arrives_reads_nothing_and_stops_when_asked() {
        let (port, seen) = idle_server(true);
        let arrivals = std::cell::Cell::new(0);
        let started = Instant::now();
        let should_stop = || arrivals.get() >= 1 && started.elapsed() > Duration::from_millis(1500);
        let mut arrived = || arrivals.set(arrivals.get() + 1);
        let times = Watching { renew: Duration::from_secs(20), session: Duration::from_secs(60) };
        let heard = std::cell::RefCell::new(Vec::new());
        let listening = |on: bool| heard.borrow_mut().push(on);
        let watched = watch(&here(port), "INBOX", &pw("x"), times, &should_stop, &listening, &mut arrived).unwrap();
        assert_eq!(watched, Watched::Stopped);
        assert_eq!(*heard.borrow(), vec![true, false], "listening while it waited, and not after");
        assert_eq!(arrivals.get(), 1, "the one message that arrived, once");
        assert!(
            started.elapsed() < Duration::from_millis(1500) + Duration::from_secs(3),
            "a stop is noticed while idle"
        );
        let commands = seen.recv().unwrap();
        only_watching(&commands);
        assert!(commands.iter().any(|c| c.ends_with("EXAMINE \"INBOX\"")), "{commands:?}");
        assert_eq!(commands.iter().filter(|c| c.ends_with(" IDLE")).count(), 2, "{commands:?}");
        assert_eq!(commands.iter().filter(|c| c.as_str() == "DONE").count(), 2, "every IDLE is ended: {commands:?}");
    }

    #[test]
    fn a_server_that_says_the_count_it_had_is_not_mail_arriving() {
        let (port, _seen) = idle_server_saying(true, "* 3 EXISTS", false);
        let arrivals = std::cell::Cell::new(0);
        let started = Instant::now();
        let should_stop = || started.elapsed() > Duration::from_millis(1200);
        let mut arrived = || arrivals.set(arrivals.get() + 1);
        let times = Watching { renew: Duration::from_secs(20), session: Duration::from_secs(60) };
        let watched = watch(&here(port), "INBOX", &pw("x"), times, &should_stop, &|_| {}, &mut arrived).unwrap();
        assert_eq!(watched, Watched::Stopped);
        assert_eq!(arrivals.get(), 0, "3 was what the folder held");
    }

    #[test]
    fn mail_that_comes_after_one_was_removed_is_mail_arriving() {
        // 3 held; one removed (2), then one came (3): the same count as at
        // first, and new mail all the same.
        let (port, _seen) = idle_server_saying(true, "* 2 EXPUNGE\r\n* 3 EXISTS", false);
        let arrivals = std::cell::Cell::new(0);
        let started = Instant::now();
        let should_stop = || started.elapsed() > Duration::from_millis(1200);
        let mut arrived = || arrivals.set(arrivals.get() + 1);
        let times = Watching { renew: Duration::from_secs(20), session: Duration::from_secs(60) };
        watch(&here(port), "INBOX", &pw("x"), times, &should_stop, &|_| {}, &mut arrived).unwrap();
        assert_eq!(arrivals.get(), 1);
    }

    #[test]
    fn a_reply_in_the_sent_folder_is_found_by_what_it_answers() {
        let (port, seen) = idle_server_saying(true, "", false);
        let since = chrono::NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        assert!(reply_in_folder(&here(port), &pw("x"), "Sent", "answered@x.example", since).unwrap(), "References");
        assert!(reply_in_folder(&here(port), &pw("x"), "Sent", "<late@x.example>", since).unwrap(), "In-Reply-To");
        assert!(!reply_in_folder(&here(port), &pw("x"), "Sent", "other@x.example", since).unwrap());
        // Only reading, and only two headers.
        let commands = seen.recv().unwrap();
        assert!(commands.iter().any(|c| c.ends_with("EXAMINE \"Sent\"")), "{commands:?}");
        assert!(commands.iter().any(|c| c.ends_with("UID SEARCH SINCE 1-Oct-2026")), "{commands:?}");
        assert!(
            commands
                .iter()
                .any(|c| c.ends_with("UID FETCH 5,6,7 (UID BODY.PEEK[HEADER.FIELDS (IN-REPLY-TO REFERENCES)])")),
            "{commands:?}"
        );
        for c in &commands {
            let verb = c.split_whitespace().nth(1).unwrap_or(c).to_ascii_uppercase();
            assert!(["LOGIN", "CAPABILITY", "EXAMINE", "UID", "LOGOUT"].contains(&verb.as_str()), "{c}");
            assert!(!c.to_ascii_uppercase().contains("BODY["), "nothing opened: {c}");
        }
    }

    #[test]
    fn the_count_is_kept_from_what_the_server_says() {
        let mut count = Count(3);
        assert!(!count.hears("* 3 EXISTS"), "the same count");
        assert!(!count.hears("* 1 EXPUNGE"));
        assert_eq!(count, Count(2));
        assert!(count.hears("* 3 exists"), "one more than after the removal");
        assert!(!count.hears("* 1 EXISTS"), "fewer is not news");
        assert_eq!(count, Count(1));
        assert!(!count.hears("* OK Still here"));
        assert!(!count.hears("* 4 FETCH (FLAGS (\\Seen))"));
        assert_eq!(count, Count(1));
    }

    #[test]
    fn a_server_that_ends_every_wait_at_once_is_left_for_the_callers_pause() {
        let (port, seen) = idle_server_saying(true, "", true);
        let times = Watching { renew: Duration::from_secs(20), session: Duration::from_secs(60) };
        let e = watch(&here(port), "INBOX", &pw("x"), times, &|| false, &|_| {}, &mut || {}).unwrap_err();
        assert!(matches!(e, ImapError::Protocol(_)), "{e}");
        let commands = seen.recv().unwrap();
        assert_eq!(commands.iter().filter(|c| c.ends_with(" IDLE")).count() as u32, QUICK_ROUNDS, "{commands:?}");
    }

    #[test]
    fn a_watch_is_renewed_and_ends_for_a_fresh_connection() {
        let (port, seen) = idle_server(true);
        let arrivals = std::cell::Cell::new(0);
        let mut arrived = || arrivals.set(arrivals.get() + 1);
        let times = Watching { renew: Duration::from_millis(200), session: Duration::from_secs(2) };
        let started = Instant::now();
        let watched = watch(&here(port), "INBOX", &pw("x"), times, &|| false, &|_| {}, &mut arrived).unwrap();
        assert_eq!(watched, Watched::Renew);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(arrivals.get(), 1);
        let commands = seen.recv().unwrap();
        only_watching(&commands);
        assert!(commands.iter().filter(|c| c.ends_with(" IDLE")).count() >= 3, "renewed: {commands:?}");
    }

    #[test]
    fn a_server_that_cannot_say_when_mail_arrives_is_left_to_the_schedule() {
        let (port, seen) = idle_server(false);
        let heard = std::cell::RefCell::new(Vec::new());
        let listening = |on: bool| heard.borrow_mut().push(on);
        let watched =
            watch(&here(port), "INBOX", &pw("x"), Watching::USUAL, &|| false, &listening, &mut || {}).unwrap();
        assert_eq!(watched, Watched::NoIdle);
        assert_eq!(*heard.borrow(), vec![false], "never listening");
        let commands = seen.recv().unwrap();
        only_watching(&commands);
        assert!(!commands.iter().any(|c| c.contains("EXAMINE") || c.contains("IDLE")), "{commands:?}");
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
        sync(&db, &source, &pw("pw"), &mut |_, _| {}, &|| false).unwrap();
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
        let s = sync(&db, &source, &pw("pw"), &mut |_, _| {}, &|| false).unwrap();
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
        sync(&db, &source, &pw("pw"), &mut |_, _| {}, &|| false).unwrap();
        let (port2, _) = serve(make(2), "pw");
        let mut cfg: ImapSourceConfig =
            serde_json::from_value(db.get_source(&source).unwrap().unwrap().config).unwrap();
        cfg.account.port = port2;
        db.set_source_config(&source, &serde_json::to_value(&cfg).unwrap()).unwrap();
        let s = sync(&db, &source, &pw("pw"), &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(s.renumbered, vec!["INBOX".to_string()]);
        assert_eq!(s.fetched, 1);
        assert_eq!(s.inserted, 0);
        assert_eq!(s.duplicates, 1);
    }

    #[test]
    fn a_wrong_password_says_so_and_marks_the_source_failed() {
        let (port, _) = serve(vec![Mailbox { name: "INBOX", validity: 1, messages: vec![] }], "right");
        let (db, source) = db_for(port);
        let err = sync(&db, &source, &pw("wrong"), &mut |_, _| {}, &|| false).unwrap_err();
        assert!(matches!(err, ImapError::Login(_)), "{err}");
        assert!(err.to_string().contains("app password"));
        let s = db.get_source(&source).unwrap().unwrap();
        assert_eq!(s.status, "failed");
        let last = s.last_error.unwrap();
        assert!(!last.to_string().contains("wrong"), "the password never reaches an error");

        // What was said is kept with it, without the password.
        let said: Vec<String> = serde_json::from_value(last["transcript"].clone()).unwrap();
        assert_eq!(said[0], format!("imap connecting to 127.0.0.1:{port}"));
        assert!(said[1].starts_with("imap < * OK"), "{said:?}");
        assert!(said.iter().any(|l| l.ends_with("LOGIN [the username and password]")), "{said:?}");
        assert!(said.iter().any(|l| l.contains("NO [AUTHENTICATIONFAILED]")), "the server's answer: {said:?}");
        assert!(!said.iter().any(|l| l.contains("wrong") || l.contains("c@example.com\"")), "{said:?}");
    }

    #[test]
    fn a_new_password_is_kept_only_when_it_logs_in_and_nothing_imported_is_touched() {
        let inbox = vec![(1, mail("a@x", "Ada <ada@x>", None, "Mon, 1 Sep 2026 09:00:00 +0000", "Lunch?"))];
        let (port, _) = serve(vec![Mailbox { name: "INBOX", validity: 1, messages: inbox }], "right");
        let (db, source) = db_for(port);
        sync(&db, &source, &pw("right"), &mut |_, _| {}, &|| false).unwrap();
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
        let err = sync(&db, &source, &pw("pw"), &mut |_, _| {}, &|| {
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
        let s = sync(&db, &source, &pw("pw"), &mut |_, _| {}, &|| false).unwrap();
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
        sync(&db, &source, &pw("pw"), &mut |_, _| {}, &|| false).unwrap();
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
        sync(&db, &source, &pw("pw"), &mut |_, _| {}, &|| false).unwrap();
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
        sync(&db, &source, &pw("pw"), &mut |_, _| {}, &|| false).unwrap();

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
        let s = sync(&db, &source, &pw("pw"), &mut |_, _| {}, &|| false).unwrap();
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
        sync(&db, &source, &pw("pw"), &mut |_, _| {}, &|| false).unwrap();
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
        let s = sync(&db, &source, &pw("pw"), &mut |_, _| {}, &|| false).unwrap();
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
        sync(&db, &first, &pw("pw"), &mut |_, _| {}, &|| false).unwrap();
        let (port2, _) = serve(boxes(), "pw");
        let config = ImapSourceConfig {
            account: ImapAccount {
                host: "127.0.0.1".into(),
                port: port2,
                username: "c@example.com".into(),
                security: Security::Plain,
                auth: Auth::Password,
            },
            folders: vec![],
            sent_folder: None,
            marks: BTreeMap::new(),
            last_checked_at: None,
            last_read_at: None,
            sending: None,
            signature: None,
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
        let s = sync(&db, &second, &pw("pw"), &mut |_, _| {}, &|| false).unwrap();
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
        let account = ImapAccount {
            host: "localhost".into(),
            port,
            username: "c".into(),
            security: Security::Plain,
            auth: Auth::Password,
        };
        let p = probe(&account, &pw("pw")).unwrap();
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
    fn a_sent_reply_is_copied_to_the_sent_folder_read_and_nothing_else_is_written() {
        let boxes = vec![
            Mailbox { name: "INBOX", validity: 1, messages: vec![] },
            Mailbox { name: "Sent", validity: 1, messages: vec![] },
        ];
        let (port, seen) = serve(boxes, "pw");
        let config = ImapSourceConfig {
            account: ImapAccount {
                host: "127.0.0.1".into(),
                port,
                username: "c@example.com".into(),
                security: Security::Plain,
                auth: Auth::Password,
            },
            folders: vec![],
            sent_folder: None,
            marks: BTreeMap::new(),
            last_checked_at: None,
            last_read_at: None,
            sending: None,
            signature: None,
        };
        let reply = b"From: c@example.com\r\nTo: ada@example.com\r\nSubject: Re: Lunch\r\n\r\nsee you there\r\n";
        copy_to_sent(&config, &pw("pw"), reply).unwrap();
        let commands = seen.recv().unwrap();
        let append = commands.iter().find(|c| c.contains("APPEND")).expect("an APPEND");
        assert!(append.ends_with(&format!("APPEND \"Sent\" (\\Seen) {{{}}}", reply.len())), "{append}");
        assert!(commands.iter().any(|c| c.contains("(appended From: c@example.com")), "{commands:?}");
        assert!(
            !commands.iter().any(|c| ["STORE", "EXPUNGE", "DELETE", "COPY", "MOVE", "SELECT"].iter().any(|w| c
                .to_ascii_uppercase()
                .split_whitespace()
                .nth(1)
                == Some(w))),
            "only the copy is written: {commands:?}"
        );
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
