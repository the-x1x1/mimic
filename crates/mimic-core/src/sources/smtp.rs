//! Sending: SMTP, through the user's own mailbox.
//!
//! A reply Mimic sends goes out exactly as one the user sends from their own
//! mail program: from their address, through their provider's server, signed
//! in the way their mailbox already signs in to read — the app password, or
//! the Microsoft sign-in (`AUTH XOAUTH2` with the access token the mailbox
//! checks with). Nothing passes through anything of Mimic's.
//!
//! What it does:
//!
//! * Connects with implicit TLS (`SmtpSecurity::Tls`, usually port 465) or
//!   plain and then `STARTTLS` (`SmtpSecurity::StartTls`, usually 587),
//!   verified against the bundled Mozilla root store. A server asked for
//!   STARTTLS that does not offer it is left before anything is said, so a
//!   password is never offered in the clear. Plain text all the way is only
//!   for this computer (a local bridge, and the tests).
//! * Signs in with `AUTH PLAIN` (or `LOGIN`, when that is all a server
//!   offers) for a password, and `AUTH XOAUTH2` for an access token.
//! * Hands over one message to its recipients: `MAIL FROM`, `RCPT TO` for
//!   each, `DATA` with every line that starts with a full stop doubled.
//! * Tells apart a message that was certainly not sent — refused, or the
//!   connection lost before it was all handed over — from one that may have
//!   gone: the connection lost after the end of the message was written, with
//!   no answer. The second is `SmtpError::Unsure`, and nothing that sends
//!   tries again on its own after it.
//!
//! The client is a small RFC 5321 subset written against `Read + Write`, like
//! the IMAP client, so the tests drive the real protocol code against a
//! scripted server on a loopback socket.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::imap::{is_loopback_host, Credential};
use super::oauth::base64_standard;

/// Longest a single read from the server may take.
const TIMEOUT: Duration = Duration::from_secs(60);
/// Everything one reply may hold. A server that says more is not answering.
const MAX_REPLY_BYTES: usize = 64 * 1024;
/// The largest message Mimic hands over. A reply in words is a few
/// kilobytes; this is only a backstop.
pub const MAX_MESSAGE_BYTES: usize = 10 * 1024 * 1024;
/// What Mimic calls itself to the server. An address literal rather than the
/// computer's name, which says nothing about the computer.
const EHLO: &str = "EHLO [127.0.0.1]";

/// How to send through a mailbox. Kept in the mailbox's `sources.config`
/// beside how it reads (`ImapSourceConfig.sending`); the password is not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmtpAccount {
    pub host: String,
    pub port: u16,
    pub security: SmtpSecurity,
    /// Who it signs in as: the mailbox's own username.
    pub username: String,
    /// The address replies go out from.
    pub from_address: String,
    /// Whether a copy of each reply is put in the mailbox's Sent folder
    /// (0.10.0-alpha.25): `None` for what the provider is known to need —
    /// nothing for those that keep one themselves (`keeps_sent`).
    #[serde(default)]
    pub copy_to_sent: Option<bool>,
}

impl SmtpAccount {
    /// Whether Mimic puts a copy of a reply in the Sent folder itself.
    pub fn copies_to_sent(&self) -> bool {
        self.copy_to_sent.unwrap_or_else(|| !keeps_sent(&self.host))
    }
}

/// The servers whose providers put what is sent through them in the Sent
/// folder themselves, so a copy from Mimic would be a second one: Gmail,
/// Outlook.com and Microsoft 365, Yahoo and AOL, and Proton Mail Bridge on
/// this computer. Any other is copied to unless the user says not to.
pub fn keeps_sent(smtp_host: &str) -> bool {
    matches!(
        smtp_host.trim().to_ascii_lowercase().as_str(),
        "smtp.gmail.com"
            | "smtp.googlemail.com"
            | "smtp-mail.outlook.com"
            | "smtp.office365.com"
            | "smtp.mail.yahoo.com"
            | "smtp.aol.com"
            | "127.0.0.1"
            | "localhost"
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SmtpSecurity {
    /// Encrypted from the first byte: usually port 465.
    Tls,
    /// Plain at first, encrypted with `STARTTLS` before anything else is
    /// said: usually port 587. A server that does not offer it is not used.
    StartTls,
    /// Never encrypted: this computer only.
    Plain,
}

#[derive(Debug, thiserror::Error)]
pub enum SmtpError {
    #[error("Couldn't reach {0} to send. Check the server name and port, and that you're online.")]
    Connect(String),
    #[error("The secure connection to {0} failed: {1}")]
    Tls(String, String),
    #[error("A plain, unencrypted connection is only allowed to this computer. Use TLS or STARTTLS for {0}.")]
    PlainRefused(String),
    #[error(
        "{0} didn't offer to encrypt the connection, so I didn't sign in or send anything. Try port 465 with TLS."
    )]
    NoStartTls(String),
    #[error("The mail server turned down the sign-in for sending ({0}). Gmail, iCloud and Yahoo want an app password; a work Microsoft 365 mailbox may need its admin to allow sending from apps (SMTP AUTH).")]
    SignIn(String),
    #[error("The mail server wouldn't send to {0}: {1}")]
    Recipient(String, String),
    #[error("The mail server said no: {0}")]
    Refused(String),
    #[error("The mail server's reply didn't make sense: {0}")]
    Protocol(String),
    #[error("The connection dropped before the message was handed over, so it wasn't sent: {0}")]
    Io(String),
    #[error("The connection dropped just as the message was handed over, so it may or may not have gone. Look in your Sent folder before sending it again.")]
    Unsure,
    #[error("That message is larger than the mail server takes, so it wasn't sent.")]
    TooLarge,
}

impl From<std::io::Error> for SmtpError {
    fn from(e: std::io::Error) -> Self {
        SmtpError::Io(e.to_string())
    }
}

impl SmtpError {
    /// Whether the message may have reached the server. Only then is it
    /// unsafe to send it again.
    pub fn may_have_gone(&self) -> bool {
        matches!(self, SmtpError::Unsure)
    }
}

/// Whether Microsoft 365 turned down the sign-in because the organisation
/// turned off sending from apps by SMTP — `SmtpClientAuthentication is
/// disabled`, or "locked by your organization's security defaults policy"
/// — then nothing was sent, and the same message can go through Microsoft
/// Graph (0.10.0-alpha.94, `graph_mail`).
pub fn turned_off_for_apps(e: &SmtpError) -> bool {
    match e {
        SmtpError::SignIn(why) => {
            let why = why.to_ascii_lowercase();
            why.contains("smtpclientauthentication")
                || why.contains("smtp_auth_disabled")
                || why.contains("security defaults")
        }
        _ => false,
    }
}

/// A server's text, cut to one short line: it goes into error messages.
fn one_line(s: &str) -> String {
    let line = s.lines().next().unwrap_or_default().trim();
    let mut out: String = line.chars().take(160).collect();
    if line.chars().count() > 160 {
        out.push('…');
    }
    out
}

// ------------------------------------------------------------------ protocol

/// One reply: its code and the text of each of its lines.
#[derive(Debug, Clone)]
struct Reply {
    code: u16,
    lines: Vec<String>,
}

impl Reply {
    fn text(&self) -> String {
        let joined = self.lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ");
        if joined.is_empty() {
            self.code.to_string()
        } else {
            one_line(&format!("{} {joined}", self.code))
        }
    }
}

/// What the server said it can do, from its answer to `EHLO`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Extensions {
    starttls: bool,
    /// Sign-in mechanisms, upper case.
    auth: Vec<String>,
    /// The largest message it takes, when it says.
    size: Option<u64>,
}

fn extensions(reply: &Reply) -> Extensions {
    let mut ext = Extensions::default();
    // The first line is the server's name; each one after is a keyword.
    for line in reply.lines.iter().skip(1) {
        let mut words = line.split_whitespace();
        let Some(keyword) = words.next() else { continue };
        let keyword = keyword.to_ascii_uppercase();
        if keyword == "STARTTLS" {
            ext.starttls = true;
        } else if keyword == "AUTH" {
            ext.auth.extend(words.map(|w| w.to_ascii_uppercase()));
        } else if let Some(first) = keyword.strip_prefix("AUTH=") {
            // An older spelling, still sent by some servers.
            ext.auth.push(first.to_string());
            ext.auth.extend(words.map(|w| w.to_ascii_uppercase()));
        } else if keyword == "SIZE" {
            ext.size = words.next().and_then(|n| n.parse::<u64>().ok()).filter(|n| *n > 0);
        }
    }
    ext.auth.dedup();
    ext
}

pub struct Session<S: Read + Write> {
    stream: S,
    buf: Vec<u8>,
}

impl<S: Read + Write> Session<S> {
    fn new(stream: S) -> Self {
        Session { stream, buf: Vec::new() }
    }

    fn fill(&mut self) -> Result<(), SmtpError> {
        let mut chunk = [0u8; 4096];
        let n = self.stream.read(&mut chunk)?;
        if n == 0 {
            return Err(SmtpError::Io("the server closed the connection".into()));
        }
        self.buf.extend_from_slice(&chunk[..n]);
        Ok(())
    }

    /// One line of a reply, without its line ending. A bare LF ends a line
    /// too: a server that sends one is wrong, but what it meant is clear.
    fn read_line(&mut self) -> Result<String, SmtpError> {
        loop {
            if let Some(p) = self.buf.iter().position(|b| *b == b'\n') {
                let raw: Vec<u8> = self.buf.drain(..=p).collect();
                let text = String::from_utf8_lossy(&raw[..p]).trim_end_matches('\r').to_string();
                super::transcript::note("smtp <", &text);
                return Ok(text);
            }
            if self.buf.len() > MAX_REPLY_BYTES {
                return Err(SmtpError::Protocol("a reply with no end".into()));
            }
            self.fill()?;
        }
    }

    /// A whole reply: `250-…` lines until `250 …`.
    fn read_reply(&mut self) -> Result<Reply, SmtpError> {
        let mut lines = Vec::new();
        let mut held = 0usize;
        loop {
            let line = self.read_line()?;
            held += line.len();
            if held > MAX_REPLY_BYTES {
                return Err(SmtpError::Protocol("a reply with no end".into()));
            }
            let bytes = line.as_bytes();
            if bytes.len() < 3 || !bytes[..3].iter().all(u8::is_ascii_digit) {
                return Err(SmtpError::Protocol(one_line(&line)));
            }
            let code: u16 = line[..3].parse().map_err(|_| SmtpError::Protocol(one_line(&line)))?;
            let more = bytes.get(3) == Some(&b'-');
            lines.push(line.get(4..).unwrap_or_default().to_string());
            if !more {
                return Ok(Reply { code, lines });
            }
        }
    }

    fn send_line(&mut self, line: &str) -> Result<(), SmtpError> {
        self.send_shown(line, line)
    }

    /// Send `line`, kept in the transcript as `shown`: a secret is never
    /// kept, only what it was.
    fn send_shown(&mut self, line: &str, shown: &str) -> Result<(), SmtpError> {
        super::transcript::note("smtp >", shown);
        self.stream.write_all(line.as_bytes())?;
        self.stream.write_all(b"\r\n")?;
        self.stream.flush()?;
        Ok(())
    }

    fn command(&mut self, line: &str) -> Result<Reply, SmtpError> {
        self.send_line(line)?;
        self.read_reply()
    }

    /// A command carrying a secret, kept in the transcript as `shown`.
    fn secret(&mut self, line: &str, shown: &str) -> Result<Reply, SmtpError> {
        self.send_shown(line, shown)?;
        self.read_reply()
    }

    /// The server's greeting.
    fn greeting(&mut self) -> Result<(), SmtpError> {
        let r = self.read_reply()?;
        if r.code != 220 {
            return Err(SmtpError::Refused(r.text()));
        }
        Ok(())
    }

    fn hello(&mut self) -> Result<Extensions, SmtpError> {
        let r = self.command(EHLO)?;
        if r.code != 250 {
            return Err(SmtpError::Refused(r.text()));
        }
        Ok(extensions(&r))
    }

    /// Sign in. A password with `PLAIN`, or with `LOGIN` when the server
    /// offers that and not `PLAIN`; an access token with `XOAUTH2`, its
    /// response sent after the server's go-ahead, as Microsoft documents it.
    /// A token turned down comes back as a challenge carrying the reason,
    /// which is answered with an empty line before the server says no.
    fn authenticate(&mut self, ext: &Extensions, username: &str, credential: &Credential) -> Result<(), SmtpError> {
        let username = username.trim();
        let refused = |r: Reply| SmtpError::SignIn(r.text());
        match credential {
            Credential::AccessToken(token) => {
                let r = self.command("AUTH XOAUTH2")?;
                if r.code != 334 {
                    return Err(refused(r));
                }
                let r = self.secret(&super::oauth::xoauth2(username, token), "[the sign-in]")?;
                match r.code {
                    235 => Ok(()),
                    334 => {
                        let r = self.command("")?;
                        Err(refused(r))
                    }
                    _ => Err(refused(r)),
                }
            }
            Credential::Password(password) => {
                let offers = |m: &str| ext.auth.iter().any(|a| a == m);
                if offers("LOGIN") && !offers("PLAIN") {
                    let r = self.command("AUTH LOGIN")?;
                    if r.code != 334 {
                        return Err(refused(r));
                    }
                    let r = self.secret(&base64_standard(username.as_bytes()), "[the username]")?;
                    if r.code != 334 {
                        return Err(refused(r));
                    }
                    let r = self.secret(&base64_standard(password.as_bytes()), "[the password]")?;
                    if r.code != 235 {
                        return Err(refused(r));
                    }
                    return Ok(());
                }
                let response = base64_standard(format!("\0{username}\0{password}").as_bytes());
                let r = self.secret(&format!("AUTH PLAIN {response}"), "AUTH PLAIN [the username and password]")?;
                if r.code != 235 {
                    return Err(refused(r));
                }
                Ok(())
            }
        }
    }

    /// Hand one message to its recipients. Until the last line of the
    /// message is written, a lost connection means it was not sent (the
    /// server throws away a message it did not see the end of); after it,
    /// with no answer, it may have been.
    fn hand_over(&mut self, from: &str, recipients: &[String], message: &[u8]) -> Result<(), SmtpError> {
        let r = self.command(&format!("MAIL FROM:<{}>", envelope_address(from)?))?;
        if r.code != 250 {
            return Err(SmtpError::Refused(r.text()));
        }
        for to in recipients {
            let r = self.command(&format!("RCPT TO:<{}>", envelope_address(to)?))?;
            if r.code != 250 && r.code != 251 {
                // Nothing goes to anyone when one of them is refused: a
                // reply that reaches only some of the people it names is
                // not the reply that was approved.
                let _ = self.command("RSET");
                return Err(SmtpError::Recipient(to.trim().to_string(), r.text()));
            }
        }
        let r = self.command("DATA")?;
        if r.code != 354 {
            return Err(SmtpError::Refused(r.text()));
        }
        let stuffed = dot_stuff(message);
        super::transcript::note("smtp >", &format!("[a message, {} bytes]", message.len()));
        self.stream.write_all(&stuffed)?;
        let unsure = |_| SmtpError::Unsure;
        self.stream.write_all(b".\r\n").map_err(unsure)?;
        self.stream.flush().map_err(unsure)?;
        // The whole message is with the server now: an answer lost or
        // garbled leaves it unknown whether it went.
        let r = self.read_reply().map_err(|_| SmtpError::Unsure)?;
        if r.code != 250 {
            return Err(SmtpError::Refused(r.text()));
        }
        Ok(())
    }

    fn quit(&mut self) {
        let _ = self.command("QUIT");
    }
}

/// An address as it goes between `<` and `>`: nothing that could end the
/// command early or start another.
fn envelope_address(address: &str) -> Result<&str, SmtpError> {
    let a = address.trim();
    if a.is_empty() || a.chars().any(|c| c.is_control() || c.is_whitespace() || c == '<' || c == '>') {
        return Err(SmtpError::Protocol(format!("\"{}\" can't be sent to", one_line(a))));
    }
    Ok(a)
}

/// A message as `DATA` carries it: every line ending CRLF (a bare LF or CR
/// becomes one), every line that starts with `.` given a second one, and a
/// line ending at the end, so the `.` that ends it stands on a line of its
/// own.
pub fn dot_stuff(message: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(message.len() + 64);
    let mut line_start = true;
    let mut i = 0;
    while i < message.len() {
        let b = message[i];
        if line_start && b == b'.' {
            out.push(b'.');
        }
        match b {
            b'\r' if message.get(i + 1) == Some(&b'\n') => {
                out.extend_from_slice(b"\r\n");
                i += 2;
                line_start = true;
                continue;
            }
            b'\r' | b'\n' => {
                out.extend_from_slice(b"\r\n");
                line_start = true;
            }
            _ => {
                out.push(b);
                line_start = false;
            }
        }
        i += 1;
    }
    if !out.ends_with(b"\r\n") {
        out.extend_from_slice(b"\r\n");
    }
    out
}

// --------------------------------------------------------------- connecting

/// The connection, before and after it is encrypted.
pub enum Stream {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(buf),
            Stream::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.write(buf),
            Stream::Tls(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Stream::Plain(s) => s.flush(),
            Stream::Tls(s) => s.flush(),
        }
    }
}

fn encrypt(host: &str, tcp: TcpStream) -> Result<Stream, SmtpError> {
    let tls = |e: String| SmtpError::Tls(host.to_string(), e);
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| tls(e.to_string()))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(host.trim().to_string()).map_err(|e| tls(e.to_string()))?;
    let client = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| tls(e.to_string()))?;
    Ok(Stream::Tls(Box::new(rustls::StreamOwned::new(client, tcp))))
}

/// Connect, encrypt as the account says, and sign in. Returns the session
/// and what the server said it can do.
fn open(account: &SmtpAccount, credential: &Credential) -> Result<(Session<Stream>, Extensions), SmtpError> {
    let (mut session, ext) = connect(account)?;
    session.authenticate(&ext, &account.username, credential)?;
    Ok((session, ext))
}

/// What a sending server says it can do, reached without signing in
/// (0.10.0-alpha.88): whether the connection is encrypted, the ways it
/// offers to sign in once it is, and the largest message it takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmtpReach {
    pub encrypted: bool,
    pub sign_in: Vec<String>,
    pub largest: Option<u64>,
}

/// Reach the sending server as a send would — connect, encrypt as the
/// account says, say hello — and leave without signing in or sending: no
/// password or token is needed or offered.
pub fn reach(account: &SmtpAccount) -> Result<SmtpReach, SmtpError> {
    let (mut session, ext) = connect(account)?;
    session.quit();
    Ok(SmtpReach { encrypted: account.security != SmtpSecurity::Plain, sign_in: ext.auth, largest: ext.size })
}

/// Connect, encrypt as the account says, and say hello: everything before
/// signing in.
fn connect(account: &SmtpAccount) -> Result<(Session<Stream>, Extensions), SmtpError> {
    let host = account.host.trim();
    // Refused before connecting: a password or a token is never offered in
    // the clear to anything but this computer.
    if account.security == SmtpSecurity::Plain && !is_loopback_host(host) {
        return Err(SmtpError::PlainRefused(host.to_string()));
    }
    let addr = format!("{host}:{}", account.port);
    super::transcript::note(
        "smtp",
        &format!(
            "connecting to {addr}{}",
            match account.security {
                SmtpSecurity::Tls => " over TLS",
                SmtpSecurity::StartTls => ", to encrypt with STARTTLS",
                SmtpSecurity::Plain => "",
            }
        ),
    );
    let tcp = TcpStream::connect(&addr).map_err(|_| SmtpError::Connect(addr.clone()))?;
    tcp.set_read_timeout(Some(TIMEOUT))?;
    tcp.set_write_timeout(Some(TIMEOUT))?;
    let over_tls = |e: SmtpError| match e {
        SmtpError::Io(why) => SmtpError::Tls(host.to_string(), why),
        other => other,
    };
    let mut session = match account.security {
        SmtpSecurity::Tls => {
            let mut s = Session::new(encrypt(host, tcp)?);
            s.greeting().map_err(over_tls)?;
            s
        }
        SmtpSecurity::StartTls | SmtpSecurity::Plain => {
            let mut s = Session::new(Stream::Plain(tcp));
            s.greeting()?;
            s
        }
    };
    let mut ext = session.hello()?;
    if account.security == SmtpSecurity::StartTls {
        if !ext.starttls {
            session.quit();
            return Err(SmtpError::NoStartTls(host.to_string()));
        }
        let r = session.command("STARTTLS")?;
        if r.code != 220 {
            return Err(SmtpError::NoStartTls(host.to_string()));
        }
        let Session { stream, buf } = session;
        // Anything the server sent before the encryption starts would be
        // read as if it came after: refused rather than trusted.
        if !buf.is_empty() {
            return Err(SmtpError::Protocol("the server said more than it should before encrypting".into()));
        }
        let Stream::Plain(tcp) = stream else {
            return Err(SmtpError::Protocol("already encrypted".into()));
        };
        session = Session::new(encrypt(host, tcp)?);
        // What was said before encryption is forgotten; ask again.
        ext = session.hello().map_err(over_tls)?;
    }
    Ok((session, ext))
}

/// Send one message: `message` is the whole of it, headers and body, as
/// `outgoing::Outgoing::to_bytes` writes it; `from` and `recipients` are the
/// envelope.
pub fn send(
    account: &SmtpAccount,
    credential: &Credential,
    from: &str,
    recipients: &[String],
    message: &[u8],
) -> Result<(), SmtpError> {
    if message.len() > MAX_MESSAGE_BYTES {
        return Err(SmtpError::TooLarge);
    }
    if recipients.is_empty() {
        return Err(SmtpError::Protocol("there was no one to send it to".into()));
    }
    let (mut session, ext) = open(account, credential)?;
    if ext.size.is_some_and(|max| message.len() as u64 > max) {
        session.quit();
        return Err(SmtpError::TooLarge);
    }
    session.hand_over(from, recipients, message)?;
    session.quit();
    Ok(())
}

/// Connect and sign in, and send nothing: whether a mailbox can send.
pub fn probe(account: &SmtpAccount, credential: &Credential) -> Result<(), SmtpError> {
    let (mut session, _) = open(account, credential)?;
    session.quit();
    Ok(())
}

/// Where a mailbox sends from, for the providers whose servers are known:
/// the server that goes with the one it reads from. `None` for any other,
/// which the user sets up by hand.
pub fn known_for(imap_host: &str, username: &str) -> Option<SmtpAccount> {
    let host = imap_host.trim().to_ascii_lowercase();
    let username = username.trim().to_string();
    let from_address = if username.contains('@') { username.clone() } else { String::new() };
    let domain = from_address.rsplit_once('@').map(|(_, d)| d.to_ascii_lowercase()).unwrap_or_default();
    let (smtp, port, security) = match host.as_str() {
        "imap.gmail.com" | "imap.googlemail.com" => ("smtp.gmail.com", 465, SmtpSecurity::Tls),
        // Outlook.com, Hotmail and Live sign in to send at their own server;
        // a work or school Microsoft 365 mailbox at Microsoft 365's.
        "outlook.office365.com" | "imap-mail.outlook.com" | "imap.outlook.com" => {
            if is_microsoft_consumer(&domain) {
                ("smtp-mail.outlook.com", 587, SmtpSecurity::StartTls)
            } else {
                ("smtp.office365.com", 587, SmtpSecurity::StartTls)
            }
        }
        "imap.mail.me.com" => ("smtp.mail.me.com", 587, SmtpSecurity::StartTls),
        "imap.mail.yahoo.com" => ("smtp.mail.yahoo.com", 465, SmtpSecurity::Tls),
        "imap.aol.com" => ("smtp.aol.com", 465, SmtpSecurity::Tls),
        "imap.fastmail.com" => ("smtp.fastmail.com", 465, SmtpSecurity::Tls),
        "imap.zoho.com" => ("smtp.zoho.com", 465, SmtpSecurity::Tls),
        "imap.zoho.eu" => ("smtp.zoho.eu", 465, SmtpSecurity::Tls),
        // GoDaddy's Workspace Email, and Namecheap's Private Email.
        "imap.secureserver.net" => ("smtpout.secureserver.net", 465, SmtpSecurity::Tls),
        "mail.privateemail.com" => ("mail.privateemail.com", 465, SmtpSecurity::Tls),
        "imap.gmx.com" => ("mail.gmx.com", 465, SmtpSecurity::Tls),
        "imap.gmx.net" => ("mail.gmx.net", 465, SmtpSecurity::Tls),
        "imap.web.de" => ("smtp.web.de", 587, SmtpSecurity::StartTls),
        "imap.mail.com" => ("smtp.mail.com", 465, SmtpSecurity::Tls),
        "imap.mail.ru" => ("smtp.mail.ru", 465, SmtpSecurity::Tls),
        "imap.yandex.com" | "imap.yandex.ru" => ("smtp.yandex.com", 465, SmtpSecurity::Tls),
        "imap.comcast.net" => ("smtp.comcast.net", 587, SmtpSecurity::StartTls),
        "imap.mail.att.net" => ("smtp.mail.att.net", 465, SmtpSecurity::Tls),
        "imap.qq.com" => ("smtp.qq.com", 465, SmtpSecurity::Tls),
        // Proton Mail Bridge, on this computer, on its default ports.
        "127.0.0.1" | "localhost" => ("127.0.0.1", 1025, SmtpSecurity::Plain),
        _ => return None,
    };
    Some(SmtpAccount { host: smtp.to_string(), port, security, username, from_address, copy_to_sent: None })
}

/// Microsoft's own mail domains — not an organisation's.
fn is_microsoft_consumer(domain: &str) -> bool {
    let first = domain.split('.').next().unwrap_or_default();
    matches!(first, "outlook" | "hotmail" | "live" | "msn" | "windowslive" | "passport")
}

/// A scripted SMTP server for tests, here and in `crate::sending`.
#[cfg(test)]
pub(crate) mod test_server {
    use crate::sources::mime::decode_base64;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc;

    #[derive(Clone, Copy)]
    pub(crate) struct Behaviour {
        pub(crate) offer_starttls: bool,
        pub(crate) auth: &'static str,
        /// The password, or the access token, this server takes.
        pub(crate) secret: &'static str,
        pub(crate) refuse_rcpt: Option<&'static str>,
        /// Close the connection once the message is in, without answering.
        pub(crate) drop_after_data: bool,
        pub(crate) size: Option<usize>,
        /// Refuse every sign-in as Microsoft 365 does for an organisation
        /// that turned off sending from apps (0.10.0-alpha.94).
        pub(crate) turned_off: bool,
    }

    impl Default for Behaviour {
        fn default() -> Self {
            Behaviour {
                offer_starttls: false,
                auth: "PLAIN LOGIN XOAUTH2",
                secret: "app-pass",
                refuse_rcpt: None,
                drop_after_data: false,
                size: None,
                turned_off: false,
            }
        }
    }

    #[derive(Debug, Default)]
    pub(crate) struct Transcript {
        pub(crate) commands: Vec<String>,
        /// The message as it arrived after `DATA`, dot-stuffing and all.
        pub(crate) data: Vec<u8>,
    }

    /// A tiny SMTP server: enough of EHLO, AUTH, MAIL, RCPT and DATA to drive
    /// the real client over a real socket, recording every line it was sent.
    /// What Microsoft 365 says to a sign-in when the organisation turned off
    /// sending from apps.
    pub(crate) const TURNED_OFF: &str = "535 5.7.139 Authentication unsuccessful, SmtpClientAuthentication is disabled for the Tenant. Visit https://aka.ms/smtp_auth_disabled for more information.\r\n";

    pub(crate) fn serve(b: Behaviour) -> (u16, mpsc::Receiver<Transcript>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut w = stream.try_clone().unwrap();
            let mut r = BufReader::new(stream);
            let mut t = Transcript::default();
            let next = |r: &mut BufReader<TcpStream>, t: &mut Transcript| -> Option<String> {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 {
                    return None;
                }
                let line = line.trim_end_matches(['\r', '\n']).to_string();
                t.commands.push(line.clone());
                Some(line)
            };
            w.write_all(b"220 test.example ESMTP ready\r\n").unwrap();
            while let Some(cmd) = next(&mut r, &mut t) {
                let upper = cmd.to_ascii_uppercase();
                let reply: String = if upper.starts_with("EHLO") {
                    let mut s = String::from("250-test.example greets you\r\n");
                    if b.offer_starttls {
                        s.push_str("250-STARTTLS\r\n");
                    }
                    if let Some(n) = b.size {
                        s.push_str(&format!("250-SIZE {n}\r\n"));
                    }
                    s.push_str(&format!("250 AUTH {}\r\n", b.auth));
                    s
                } else if let Some(arg) = cmd.strip_prefix("AUTH PLAIN ") {
                    if decode_base64(arg) == format!("\0c@example.com\0{}", b.secret).into_bytes() {
                        "235 2.7.0 Accepted\r\n".into()
                    } else {
                        "535 5.7.8 Username and Password not accepted\r\n".into()
                    }
                } else if upper == "AUTH LOGIN" {
                    w.write_all(b"334 VXNlcm5hbWU6\r\n").unwrap();
                    let user = next(&mut r, &mut t).unwrap();
                    w.write_all(b"334 UGFzc3dvcmQ6\r\n").unwrap();
                    let pass = next(&mut r, &mut t).unwrap();
                    if decode_base64(&user) == b"c@example.com" && decode_base64(&pass) == b.secret.as_bytes() {
                        "235 2.7.0 Accepted\r\n".into()
                    } else {
                        "535 5.7.8 Authentication failed\r\n".into()
                    }
                } else if upper == "AUTH XOAUTH2" {
                    w.write_all(b"334 \r\n").unwrap();
                    let said = String::from_utf8_lossy(&decode_base64(&next(&mut r, &mut t).unwrap())).into_owned();
                    if b.turned_off {
                        TURNED_OFF.into()
                    } else if said == format!("user=c@example.com\x01auth=Bearer {}\x01\x01", b.secret) {
                        "235 2.7.0 Authentication successful\r\n".into()
                    } else {
                        // The reason as a challenge, as Gmail and Microsoft do.
                        w.write_all(b"334 eyJzdGF0dXMiOiI0MDEifQ==\r\n").unwrap();
                        let answer = next(&mut r, &mut t).unwrap();
                        assert_eq!(answer, "", "a refused token's challenge is answered with an empty line");
                        "535 5.7.3 Authentication unsuccessful\r\n".into()
                    }
                } else if upper.starts_with("MAIL FROM:") || upper == "RSET" {
                    "250 2.1.0 OK\r\n".into()
                } else if let Some(to) = upper.strip_prefix("RCPT TO:") {
                    if b.refuse_rcpt.is_some_and(|x| to.contains(&x.to_ascii_uppercase())) {
                        "550 5.1.1 No such user here\r\n".into()
                    } else {
                        "250 2.1.5 OK\r\n".into()
                    }
                } else if upper == "DATA" {
                    w.write_all(b"354 Go ahead\r\n").unwrap();
                    loop {
                        let mut raw = Vec::new();
                        if r.read_until(b'\n', &mut raw).unwrap_or(0) == 0 {
                            break;
                        }
                        if raw == b".\r\n" {
                            break;
                        }
                        t.data.extend_from_slice(&raw);
                    }
                    if b.drop_after_data {
                        break;
                    }
                    "250 2.0.0 OK queued\r\n".into()
                } else if upper == "STARTTLS" {
                    "454 4.7.0 TLS not available\r\n".into()
                } else if upper == "QUIT" {
                    w.write_all(b"221 2.0.0 Bye\r\n").unwrap();
                    break;
                } else {
                    "502 5.5.2 Unknown command\r\n".into()
                };
                w.write_all(reply.as_bytes()).unwrap();
            }
            let _ = tx.send(t);
        });
        (port, rx)
    }
}

#[cfg(test)]
mod tests {
    use super::test_server::{serve, Behaviour};
    use super::*;

    fn account(port: u16, security: SmtpSecurity) -> SmtpAccount {
        SmtpAccount {
            host: "127.0.0.1".into(),
            port,
            security,
            username: "c@example.com".into(),
            from_address: "c@example.com".into(),
            copy_to_sent: None,
        }
    }

    fn pw(p: &str) -> Credential {
        Credential::Password(p.into())
    }

    const MESSAGE: &[u8] = b"From: c@example.com\r\nTo: ada@example.com\r\nSubject: Re: Offsite\r\n\r\nYes, I'll be there.\r\n.hidden line\r\n..two\r\n";

    #[test]
    fn a_reply_is_handed_over_whole_with_its_full_stops_doubled() {
        let (port, seen) = serve(Behaviour::default());
        send(
            &account(port, SmtpSecurity::Plain),
            &pw("app-pass"),
            "c@example.com",
            &["ada@example.com".into()],
            MESSAGE,
        )
        .unwrap();
        let t = seen.recv().unwrap();
        assert_eq!(t.commands[0], "EHLO [127.0.0.1]", "the computer's own name is not given");
        assert!(t.commands[1].starts_with("AUTH PLAIN "), "{:?}", t.commands);
        assert_eq!(t.commands[2], "MAIL FROM:<c@example.com>");
        assert_eq!(t.commands[3], "RCPT TO:<ada@example.com>");
        assert_eq!(t.commands[4], "DATA");
        assert_eq!(t.commands.last().map(String::as_str), Some("QUIT"));
        assert_eq!(
            t.data,
            b"From: c@example.com\r\nTo: ada@example.com\r\nSubject: Re: Offsite\r\n\r\nYes, I'll be there.\r\n..hidden line\r\n...two\r\n",
            "a line starting with a full stop has another put before it, so it can't end the message"
        );
    }

    #[test]
    fn what_was_said_is_kept_without_the_password_or_the_message() {
        let (port, _seen) = serve(Behaviour { auth: "LOGIN", ..Behaviour::default() });
        let (sent, said) = crate::sources::transcript::recording(|| {
            send(
                &account(port, SmtpSecurity::Plain),
                &pw("app-pass"),
                "c@example.com",
                &["ada@example.com".into()],
                MESSAGE,
            )
        });
        sent.unwrap();
        assert_eq!(said[0], format!("smtp connecting to 127.0.0.1:{port}"));
        assert!(said.contains(&"smtp > AUTH LOGIN".to_string()), "{said:?}");
        assert!(
            said.contains(&"smtp > [the username]".to_string()) && said.contains(&"smtp > [the password]".to_string())
        );
        assert!(said.contains(&"smtp > RCPT TO:<ada@example.com>".to_string()));
        assert!(said.contains(&format!("smtp > [a message, {} bytes]", MESSAGE.len())), "{said:?}");
        assert!(said.iter().any(|l| l.starts_with("smtp < 250")), "the server's side: {said:?}");
        let all = said.join("\n");
        let secret = base64_standard(b"app-pass");
        assert!(!all.contains("app-pass") && !all.contains(&secret), "{all}");
        assert!(!all.contains("Offsite") && !all.contains("be there"), "nothing of the message: {all}");
    }

    #[test]
    fn reaching_a_server_signs_in_to_nothing_and_says_what_it_offers() {
        let (port, seen) = serve(Behaviour { size: Some(1000), ..Behaviour::default() });
        let reached = reach(&account(port, SmtpSecurity::Plain)).unwrap();
        assert_eq!(reached.sign_in, vec!["PLAIN".to_string(), "LOGIN".to_string(), "XOAUTH2".to_string()]);
        assert_eq!(reached.largest, Some(1000));
        assert!(!reached.encrypted, "plain, on this computer");
        let t = seen.recv().unwrap();
        assert_eq!(t.commands, vec!["EHLO [127.0.0.1]".to_string(), "QUIT".to_string()], "nothing else said");

        // Plain to anywhere else is refused before connecting, as a send is.
        let far = SmtpAccount { host: "smtp.example.com".into(), ..account(port, SmtpSecurity::Plain) };
        assert!(matches!(reach(&far), Err(SmtpError::PlainRefused(_))));
    }

    #[test]
    fn login_is_used_only_when_plain_is_not_offered() {
        let (port, seen) = serve(Behaviour { auth: "LOGIN", ..Behaviour::default() });
        probe(&account(port, SmtpSecurity::Plain), &pw("app-pass")).unwrap();
        let t = seen.recv().unwrap();
        assert_eq!(t.commands[1], "AUTH LOGIN");
        assert!(!t.commands.iter().any(|c| c.starts_with("MAIL")), "a probe sends nothing");
    }

    #[test]
    fn microsoft_365_with_sending_from_apps_off_is_told_from_a_sign_in_turned_down() {
        let (port, seen) = serve(Behaviour { secret: "token-1", turned_off: true, ..Behaviour::default() });
        let token = Credential::AccessToken("token-1".into());
        let e =
            send(&account(port, SmtpSecurity::Plain), &token, "c@example.com", &["ada@example.com".into()], MESSAGE)
                .unwrap_err();
        assert!(turned_off_for_apps(&e), "{e}");
        assert!(!e.may_have_gone());
        let t = seen.recv().unwrap();
        assert!(!t.commands.iter().any(|c| c.starts_with("MAIL")), "nothing was sent");
        // A token or password turned down for itself is not that.
        let (port, _seen) = serve(Behaviour { secret: "token-1", ..Behaviour::default() });
        let e = probe(&account(port, SmtpSecurity::Plain), &Credential::AccessToken("token-0".into())).unwrap_err();
        assert!(!turned_off_for_apps(&e), "{e}");
        assert!(!turned_off_for_apps(&SmtpError::Unsure));
        // Security defaults say it their own way.
        assert!(turned_off_for_apps(&SmtpError::SignIn(
            "535 5.7.139 Authentication unsuccessful, user is locked by your organization's security defaults policy. Contact your administrator.".into()
        )));
    }

    #[test]
    fn a_password_turned_down_is_said_and_nothing_is_sent() {
        let (port, seen) = serve(Behaviour::default());
        let e = send(
            &account(port, SmtpSecurity::Plain),
            &pw("wrong"),
            "c@example.com",
            &["ada@example.com".into()],
            MESSAGE,
        )
        .unwrap_err();
        assert!(matches!(e, SmtpError::SignIn(ref why) if why.contains("535")), "{e}");
        assert!(!e.may_have_gone());
        drop(e);
        // The server is still waiting for the next command; the client hung up.
        let t = seen.recv().unwrap();
        assert!(!t.commands.iter().any(|c| c.starts_with("MAIL") || c == "DATA"));
    }

    #[test]
    fn an_access_token_signs_in_with_xoauth2_and_one_turned_down_is_answered_then_reported() {
        let (port, seen) = serve(Behaviour { secret: "token-1", ..Behaviour::default() });
        let token = Credential::AccessToken("token-1".into());
        send(&account(port, SmtpSecurity::Plain), &token, "c@example.com", &["ada@example.com".into()], MESSAGE)
            .unwrap();
        let t = seen.recv().unwrap();
        assert_eq!(t.commands[1], "AUTH XOAUTH2", "the response follows the server's go-ahead");

        let (port, _seen) = serve(Behaviour { secret: "token-1", ..Behaviour::default() });
        let stale = Credential::AccessToken("token-0".into());
        let e = probe(&account(port, SmtpSecurity::Plain), &stale).unwrap_err();
        assert!(matches!(e, SmtpError::SignIn(ref why) if why.contains("535")), "{e}");
    }

    #[test]
    fn a_recipient_turned_down_is_named_and_the_message_goes_to_no_one() {
        let (port, seen) = serve(Behaviour { refuse_rcpt: Some("bob@example.com"), ..Behaviour::default() });
        let e = send(
            &account(port, SmtpSecurity::Plain),
            &pw("app-pass"),
            "c@example.com",
            &["ada@example.com".into(), "bob@example.com".into()],
            MESSAGE,
        )
        .unwrap_err();
        assert!(matches!(e, SmtpError::Recipient(ref who, _) if who == "bob@example.com"), "{e}");
        drop(e);
        let t = seen.recv().unwrap();
        assert!(t.commands.contains(&"RSET".to_string()));
        assert!(!t.commands.contains(&"DATA".to_string()), "nothing was handed over");
    }

    #[test]
    fn a_connection_lost_after_the_message_is_in_may_have_sent_it() {
        let (port, _seen) = serve(Behaviour { drop_after_data: true, ..Behaviour::default() });
        let e = send(
            &account(port, SmtpSecurity::Plain),
            &pw("app-pass"),
            "c@example.com",
            &["ada@example.com".into()],
            MESSAGE,
        )
        .unwrap_err();
        assert!(e.may_have_gone(), "{e}");
    }

    #[test]
    fn a_message_larger_than_the_server_takes_is_not_handed_over() {
        let (port, seen) = serve(Behaviour { size: Some(16), ..Behaviour::default() });
        let e = send(
            &account(port, SmtpSecurity::Plain),
            &pw("app-pass"),
            "c@example.com",
            &["ada@example.com".into()],
            MESSAGE,
        )
        .unwrap_err();
        assert!(matches!(e, SmtpError::TooLarge), "{e}");
        drop(e);
        assert!(!seen.recv().unwrap().commands.iter().any(|c| c.starts_with("MAIL")));
    }

    #[test]
    fn a_server_asked_to_encrypt_that_cannot_is_left_before_signing_in() {
        // Not offered at all…
        let (port, seen) = serve(Behaviour::default());
        let e = probe(&account(port, SmtpSecurity::StartTls), &pw("app-pass")).unwrap_err();
        assert!(matches!(e, SmtpError::NoStartTls(_)), "{e}");
        drop(e);
        let t = seen.recv().unwrap();
        assert!(
            !t.commands.iter().any(|c| c.starts_with("AUTH")),
            "no password offered in the clear: {:?}",
            t.commands
        );

        // …or offered and then turned down.
        let (port, seen) = serve(Behaviour { offer_starttls: true, ..Behaviour::default() });
        let e = probe(&account(port, SmtpSecurity::StartTls), &pw("app-pass")).unwrap_err();
        assert!(matches!(e, SmtpError::NoStartTls(_)), "{e}");
        drop(e);
        drop(seen);
    }

    #[test]
    fn plain_text_to_a_remote_server_is_refused_before_connecting() {
        let remote = SmtpAccount { host: "smtp.example.com".into(), ..account(25, SmtpSecurity::Plain) };
        assert!(matches!(probe(&remote, &pw("secret")), Err(SmtpError::PlainRefused(_))));
    }

    #[test]
    fn an_address_that_could_end_the_command_is_refused() {
        let (port, _seen) = serve(Behaviour::default());
        let e = send(
            &account(port, SmtpSecurity::Plain),
            &pw("app-pass"),
            "c@example.com",
            &["ada@example.com>\r\nRCPT TO:<eve@example.com".into()],
            MESSAGE,
        )
        .unwrap_err();
        assert!(matches!(e, SmtpError::Protocol(_)), "{e}");
    }

    #[test]
    fn dot_stuffing_makes_every_line_end_crlf_and_doubles_leading_full_stops() {
        assert_eq!(dot_stuff(b"a\nb\r\nc"), b"a\r\nb\r\nc\r\n");
        assert_eq!(dot_stuff(b".\n..x\r\n"), b"..\r\n...x\r\n");
        assert_eq!(dot_stuff(b"x\ry"), b"x\r\ny\r\n", "a bare CR ends a line too");
        assert_eq!(dot_stuff(b""), b"\r\n");
    }

    #[test]
    fn extensions_are_read_from_the_answer_to_ehlo() {
        let reply = Reply {
            code: 250,
            lines: vec![
                "smtp.example.com at your service".into(),
                "SIZE 35882577".into(),
                "8BITMIME".into(),
                "STARTTLS".into(),
                "AUTH LOGIN PLAIN XOAUTH2".into(),
                "AUTH=LOGIN".into(),
            ],
        };
        let ext = extensions(&reply);
        assert!(ext.starttls);
        assert_eq!(ext.size, Some(35882577));
        assert!(ext.auth.contains(&"XOAUTH2".to_string()) && ext.auth.contains(&"PLAIN".to_string()));
    }

    #[test]
    fn a_copy_goes_to_the_sent_folder_only_where_the_provider_keeps_none() {
        let gmail = known_for("imap.gmail.com", "c@gmail.com").unwrap();
        assert!(!gmail.copies_to_sent(), "Gmail keeps its own");
        let icloud = known_for("imap.mail.me.com", "c@icloud.com").unwrap();
        assert!(icloud.copies_to_sent());
        assert!(!SmtpAccount { copy_to_sent: Some(false), ..icloud }.copies_to_sent(), "the user says");
        let read: SmtpAccount = serde_json::from_value(serde_json::json!({
            "host": "smtp.fastmail.com", "port": 465, "security": "tls",
            "username": "c@fastmail.com", "fromAddress": "c@fastmail.com"
        }))
        .unwrap();
        assert_eq!(read.copy_to_sent, None, "set up before there was a choice");
        assert!(read.copies_to_sent());
    }

    #[test]
    fn the_known_providers_send_from_their_own_servers() {
        let gmail = known_for("imap.gmail.com", "c@gmail.com").unwrap();
        assert_eq!((gmail.host.as_str(), gmail.port, gmail.security), ("smtp.gmail.com", 465, SmtpSecurity::Tls));
        assert_eq!(gmail.from_address, "c@gmail.com");
        let outlook = known_for("outlook.office365.com", "c@outlook.com").unwrap();
        assert_eq!((outlook.host.as_str(), outlook.security), ("smtp-mail.outlook.com", SmtpSecurity::StartTls));
        let hotmail_uk = known_for("outlook.office365.com", "c@hotmail.co.uk").unwrap();
        assert_eq!(hotmail_uk.host, "smtp-mail.outlook.com");
        let work = known_for("outlook.office365.com", "c@contoso.com").unwrap();
        assert_eq!(work.host, "smtp.office365.com");
        let workspace = known_for("imap.gmail.com", "c@formicaria.us").unwrap();
        assert_eq!((workspace.host.as_str(), workspace.from_address.as_str()), ("smtp.gmail.com", "c@formicaria.us"));
        for (imap, smtp) in [
            ("imap.zoho.eu", "smtp.zoho.eu"),
            ("imap.secureserver.net", "smtpout.secureserver.net"),
            ("mail.privateemail.com", "mail.privateemail.com"),
            ("imap.gmx.net", "mail.gmx.net"),
            ("imap.mail.com", "smtp.mail.com"),
            ("imap.yandex.com", "smtp.yandex.com"),
            ("imap.mail.att.net", "smtp.mail.att.net"),
            ("imap.qq.com", "smtp.qq.com"),
        ] {
            let found = known_for(imap, "c@example.org").unwrap();
            assert_eq!((found.host.as_str(), found.port, found.security), (smtp, 465, SmtpSecurity::Tls));
        }
        for (imap, smtp) in [("imap.web.de", "smtp.web.de"), ("imap.comcast.net", "smtp.comcast.net")] {
            let found = known_for(imap, "c@example.org").unwrap();
            assert_eq!((found.host.as_str(), found.port, found.security), (smtp, 587, SmtpSecurity::StartTls));
        }
        assert!(known_for("mail.example.org", "c@example.org").is_none(), "anything else is set up by hand");
        assert_eq!(
            known_for("imap.gmail.com", "c").unwrap().from_address,
            "",
            "a username that isn't an address isn't one"
        );
    }
}
