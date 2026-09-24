//! Signing in to a mail provider, for mailboxes that no longer take a
//! password.
//!
//! Microsoft stopped accepting passwords of any kind for IMAP on Outlook.com
//! and Hotmail in September 2024, and for every Microsoft 365 organisation
//! in 2022–23. What they take is OAuth 2.0: an access token, presented with
//! IMAP's `AUTHENTICATE XOAUTH2`.
//!
//! The flow is the one RFC 8252 describes for apps on a computer:
//!
//! * The user's own browser opens the provider's own sign-in page. Mimic
//!   never sees the password, and whatever the provider asks for — a second
//!   factor, a consent screen — happens there.
//! * The provider sends the browser back to a port on this computer
//!   (`Loopback`, bound to the loopback addresses only) with a one-time code,
//!   and `state`, which ties the answer to the sign-in this process started.
//! * The code is exchanged for tokens together with a PKCE verifier (RFC
//!   7636) that only this process knows, so a code seen by anything else is
//!   no use to it.
//! * The access token lasts about an hour; the refresh token is what is kept,
//!   in the secret store, sealed like a password. A check trades it for a new
//!   access token, and keeps the new refresh token the provider hands back.
//!
//! A provider must know Mimic as an app. Microsoft's registration gives a
//! client id, which is compiled in (`MIMIC_MICROSOFT_CLIENT_ID`) and is not a
//! secret: an app on someone's computer cannot keep one. A build made without
//! it cannot sign in with Microsoft, and says so rather than failing.
//!
//! Nothing here logs or returns a token, a code or a verifier: `Tokens` and
//! `Pkce` print as redacted, and errors carry the provider's short reason,
//! cut to one line.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Longest a token request may take.
const TOKEN_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest the browser may take to send its one request once connected.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// How long the user has to finish signing in.
pub const SIGN_IN_WINDOW: Duration = Duration::from_secs(5 * 60);

/// A provider Mimic can sign in to, and where its mail is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    /// Stored in a mailbox's settings (`ImapAccount.auth`).
    pub key: &'static str,
    /// As the screen names it.
    pub name: &'static str,
    pub authorize_url: String,
    pub token_url: String,
    pub scope: &'static str,
    pub imap_host: &'static str,
    pub imap_port: u16,
}

impl Provider {
    /// Outlook.com, Hotmail and Microsoft 365, through the tenant that takes
    /// both personal and work or school accounts. The scope is IMAP access
    /// as the user, and `offline_access` for a refresh token.
    pub fn microsoft() -> Self {
        Provider {
            key: "microsoft",
            name: "Microsoft",
            authorize_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize".into(),
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token".into(),
            scope: "https://outlook.office.com/IMAP.AccessAsUser.All offline_access",
            imap_host: "outlook.office365.com",
            imap_port: 993,
        }
    }
}

/// Mimic's client id with Microsoft, when this build was made with one.
pub fn microsoft_client_id() -> Option<&'static str> {
    option_env!("MIMIC_MICROSOFT_CLIENT_ID").map(str::trim).filter(|s| !s.is_empty())
}

#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    #[error("This copy of Mimic isn't set up to sign in with {0}.")]
    NotConfigured(&'static str),
    #[error("Signing in was stopped.")]
    Canceled,
    #[error("Signing in took too long, so I stopped waiting. Try again when you're ready.")]
    TimedOut,
    #[error("Signing in didn't finish: {0}")]
    Denied(String),
    #[error("{0} wants you to sign in again.")]
    SignInAgain(&'static str),
    #[error("{0} turned the sign-in down: {1}")]
    Refused(&'static str, String),
    #[error("I couldn't reach {0}'s sign-in service: {1}")]
    Network(&'static str, String),
    #[error("I couldn't listen for the sign-in on this computer: {0}")]
    Local(String),
    #[error("I couldn't open your browser to sign in: {0}")]
    Browser(String),
}

/// A token response: the access token, and the refresh token when the
/// provider sent one (a refresh may or may not bring a new one).
#[derive(Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Seconds the access token is good for, as the provider said.
    pub expires_in: u64,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("access_token", &"[redacted]")
            .field("refresh_token", &self.refresh_token.as_ref().map(|_| "[redacted]"))
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

/// The PKCE pair: the verifier stays in this process, the challenge goes in
/// the sign-in address.
#[derive(Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl std::fmt::Debug for Pkce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Pkce([redacted])")
    }
}

impl Pkce {
    /// 32 random bytes as the verifier (43 characters), its SHA-256 as the
    /// challenge (`S256`). The bytes come from `rand`'s thread generator, a
    /// cryptographically secure one seeded by the operating system.
    pub fn new() -> Self {
        let bytes: [u8; 32] = rand::random();
        Self::from_verifier(base64_url(&bytes))
    }

    pub fn from_verifier(verifier: String) -> Self {
        let challenge = base64_url(&Sha256::digest(verifier.as_bytes()));
        Pkce { verifier, challenge }
    }
}

impl Default for Pkce {
    fn default() -> Self {
        Self::new()
    }
}

/// A value for `state`: random, and nothing else.
pub fn new_state() -> String {
    let bytes: [u8; 16] = rand::random();
    base64_url(&bytes)
}

/// The address the browser is sent to.
pub fn authorization_url(
    provider: &Provider,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    pkce: &Pkce,
    login_hint: Option<&str>,
) -> String {
    let mut params = vec![
        ("client_id", client_id),
        ("response_type", "code"),
        ("redirect_uri", redirect_uri),
        ("response_mode", "query"),
        ("scope", provider.scope),
        ("state", state),
        ("code_challenge", pkce.challenge.as_str()),
        ("code_challenge_method", "S256"),
    ];
    if let Some(hint) = login_hint.map(str::trim).filter(|h| !h.is_empty()) {
        params.push(("login_hint", hint));
    }
    format!("{}?{}", provider.authorize_url, form_encode(&params))
}

/// Sign in: open the provider's page in the browser with `open`, wait for
/// the browser to come back (`SIGN_IN_WINDOW`, or until `should_stop`), and
/// exchange the code for tokens. Blocks; run it off the async runtime.
pub fn sign_in(
    provider: &Provider,
    client_id: &str,
    login_hint: Option<&str>,
    open: &dyn Fn(&str) -> Result<(), String>,
    should_stop: &dyn Fn() -> bool,
) -> Result<Tokens, OAuthError> {
    let loopback = Loopback::bind()?;
    let redirect_uri = loopback.redirect_uri();
    let pkce = Pkce::new();
    let state = new_state();
    let url = authorization_url(provider, client_id, &redirect_uri, &state, &pkce, login_hint);
    open(&url).map_err(|e| OAuthError::Browser(one_line(&e)))?;
    let code = loopback.wait_for_code(&state, Instant::now() + SIGN_IN_WINDOW, should_stop)?;
    exchange_code(provider, client_id, &code, &redirect_uri, &pkce)
}

// ------------------------------------------------------------------ loopback

/// Where the browser comes back to: a port on the loopback addresses only,
/// IPv4 and — when it can be had on the same port — IPv6, since `localhost`
/// may resolve to either.
pub struct Loopback {
    v4: TcpListener,
    v6: Option<TcpListener>,
    port: u16,
}

impl Loopback {
    pub fn bind() -> Result<Self, OAuthError> {
        let local = |e: std::io::Error| OAuthError::Local(e.to_string());
        let v4 = TcpListener::bind(("127.0.0.1", 0)).map_err(local)?;
        let port = v4.local_addr().map_err(local)?.port();
        let v6 = TcpListener::bind(("::1", port)).ok();
        v4.set_nonblocking(true).map_err(local)?;
        if let Some(l) = &v6 {
            l.set_nonblocking(true).map_err(local)?;
        }
        Ok(Loopback { v4, v6, port })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// The redirect address given to the provider. A provider that knows
    /// Mimic as an app on a computer takes `http://localhost` with any port.
    pub fn redirect_uri(&self) -> String {
        format!("http://localhost:{}", self.port)
    }

    /// Wait for the browser to come back with a code for this `state`, until
    /// `deadline` or `should_stop`. A request that is not the answer — a
    /// favicon, an answer to some other sign-in — is turned away and waited
    /// past; one carrying the provider's error ends the wait with it.
    ///
    /// Every connection is read at once and without waiting on it, so one
    /// that sends nothing — a browser's spare connection, or a page holding
    /// the port open — delays neither the real answer nor a stop, and is
    /// dropped after `REQUEST_TIMEOUT`.
    pub fn wait_for_code(
        &self,
        state: &str,
        deadline: Instant,
        should_stop: &dyn Fn() -> bool,
    ) -> Result<String, OAuthError> {
        let mut incoming: Vec<Incoming> = Vec::new();
        loop {
            if should_stop() {
                return Err(OAuthError::Canceled);
            }
            if Instant::now() >= deadline {
                return Err(OAuthError::TimedOut);
            }
            for listener in std::iter::once(&self.v4).chain(self.v6.iter()) {
                // At most so many a pass, so a flood of connections can't
                // keep the wait from a stop or its deadline.
                for _ in 0..OPEN_LIMIT {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            if stream.set_nonblocking(true).is_err() {
                                continue;
                            }
                            if incoming.len() >= OPEN_LIMIT {
                                incoming.remove(0);
                            }
                            incoming.push(Incoming { stream, request: Vec::new(), since: Instant::now() });
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        // Gone before it was taken: nothing to answer.
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::ConnectionAborted
                                    | std::io::ErrorKind::ConnectionReset
                                    | std::io::ErrorKind::Interrupted
                            ) => {}
                        Err(e) => return Err(OAuthError::Local(e.to_string())),
                    }
                }
            }
            let mut i = 0;
            while i < incoming.len() {
                match incoming[i].read_some() {
                    Reading::More if incoming[i].since.elapsed() < REQUEST_TIMEOUT => i += 1,
                    Reading::More | Reading::Gone => {
                        incoming.swap_remove(i);
                    }
                    Reading::Whole => {
                        let Incoming { stream, request, .. } = incoming.swap_remove(i);
                        if let Some(answer) = answer(stream, &request, state) {
                            return answer;
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// Most connections read at once while waiting; past it the oldest is dropped.
const OPEN_LIMIT: usize = 16;
/// The most of a request read: a browser's is a fraction of it.
const REQUEST_LIMIT: usize = 16 * 1024;

/// A connection to the loopback port, and what it has sent so far.
struct Incoming {
    stream: TcpStream,
    request: Vec<u8>,
    since: Instant,
}

enum Reading {
    /// Nothing more yet.
    More,
    /// The request's head has arrived (or all there will be of it).
    Whole,
    /// Closed, or broken, having sent nothing.
    Gone,
}

impl Incoming {
    /// Read what has arrived, without waiting for more.
    fn read_some(&mut self) -> Reading {
        let mut chunk = [0u8; 2048];
        loop {
            match self.stream.read(&mut chunk) {
                Ok(0) => return if self.request.is_empty() { Reading::Gone } else { Reading::Whole },
                Ok(n) => {
                    self.request.extend_from_slice(&chunk[..n]);
                    if self.request.windows(4).any(|w| w == b"\r\n\r\n") || self.request.len() >= REQUEST_LIMIT {
                        return Reading::Whole;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Reading::More,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return Reading::Gone,
            }
        }
    }
}

const SIGNED_IN: &str = "You're signed in. You can close this tab and go back to Mimic.";
const NOT_SIGNED_IN: &str = "Signing in didn't finish. You can close this tab; Mimic says what happened.";
const NOT_THIS_ONE: &str = "This isn't the sign-in Mimic is waiting for. Start again from Mimic.";

/// Answer one request from the browser. `Some` ends the wait.
fn answer(mut stream: TcpStream, request: &[u8], state: &str) -> Option<Result<String, OAuthError>> {
    // The answer is a few hundred bytes, written whole or not at all.
    stream.set_nonblocking(false).ok()?;
    stream.set_write_timeout(Some(REQUEST_TIMEOUT)).ok()?;
    let text = String::from_utf8_lossy(request);
    let target = text.lines().next().and_then(|l| l.strip_prefix("GET ")).and_then(|l| l.split(' ').next());
    let Some(target) = target else {
        respond(&mut stream, "400 Bad Request", NOT_THIS_ONE);
        return None;
    };
    let query = parse_query(target.split_once('?').map(|(_, q)| q).unwrap_or_default());
    let for_this = query.get("state").map(String::as_str) == Some(state);
    if let Some(code) = query.get("code").filter(|c| !c.is_empty()) {
        if !for_this {
            respond(&mut stream, "400 Bad Request", NOT_THIS_ONE);
            return None;
        }
        respond(&mut stream, "200 OK", SIGNED_IN);
        return Some(Ok(code.clone()));
    }
    if let Some(error) = query.get("error") {
        if !for_this {
            respond(&mut stream, "400 Bad Request", NOT_THIS_ONE);
            return None;
        }
        respond(&mut stream, "200 OK", NOT_SIGNED_IN);
        let why = query.get("error_description").filter(|d| !d.trim().is_empty()).unwrap_or(error);
        return Some(Err(OAuthError::Denied(one_line(why))));
    }
    respond(&mut stream, "404 Not Found", NOT_THIS_ONE);
    None
}

fn respond(stream: &mut TcpStream, status: &str, message: &str) {
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Mimic</title></head>\
         <body style=\"font-family: system-ui, sans-serif; margin: 3rem; max-width: 32rem\"><p>{message}</p></body></html>"
    );
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(reply.as_bytes());
    let _ = stream.flush();
}

// -------------------------------------------------------------------- tokens

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Exchange the code the browser brought back for tokens.
pub fn exchange_code(
    provider: &Provider,
    client_id: &str,
    code: &str,
    redirect_uri: &str,
    pkce: &Pkce,
) -> Result<Tokens, OAuthError> {
    token_request(
        provider,
        &[
            ("client_id", client_id),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", pkce.verifier.as_str()),
            ("scope", provider.scope),
        ],
    )
}

/// Trade a refresh token for a new access token (and, usually, a new refresh
/// token, which replaces the old one).
pub fn refresh(provider: &Provider, client_id: &str, refresh_token: &str) -> Result<Tokens, OAuthError> {
    token_request(
        provider,
        &[
            ("client_id", client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("scope", provider.scope),
        ],
    )
}

fn token_request(provider: &Provider, form: &[(&str, &str)]) -> Result<Tokens, OAuthError> {
    let network = |e: reqwest::Error| OAuthError::Network(provider.name, one_line(&e.to_string()));
    let client = reqwest::blocking::Client::builder().timeout(TOKEN_TIMEOUT).build().map_err(network)?;
    let response = client
        .post(&provider.token_url)
        .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(reqwest::header::ACCEPT, "application/json")
        .body(form_encode(form))
        .send()
        .map_err(network)?;
    let status = response.status();
    let parsed: TokenResponse = response
        .json()
        .map_err(|_| OAuthError::Refused(provider.name, format!("an answer that wasn't JSON ({status})")))?;
    if let Some(access_token) = parsed.access_token.filter(|t| !t.is_empty()) {
        return Ok(Tokens {
            access_token,
            refresh_token: parsed.refresh_token.filter(|t| !t.is_empty()),
            expires_in: parsed.expires_in.unwrap_or(0),
        });
    }
    match parsed.error.as_deref() {
        // The refresh token was revoked or has lapsed, or the provider wants
        // the user in front of it again (a changed password, new consent).
        Some("invalid_grant") | Some("interaction_required") | Some("consent_required") => {
            Err(OAuthError::SignInAgain(provider.name))
        }
        other => {
            let why = parsed.error_description.as_deref().filter(|d| !d.trim().is_empty()).or(other);
            Err(OAuthError::Refused(provider.name, one_line(why.unwrap_or(status.as_str()))))
        }
    }
}

// ------------------------------------------------------------------ encoding

/// SASL XOAUTH2's initial response, base64: `user=…^Aauth=Bearer …^A^A`.
pub fn xoauth2(user: &str, access_token: &str) -> String {
    base64_standard(format!("user={user}\x01auth=Bearer {access_token}\x01\x01").as_bytes())
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_with(bytes: &[u8], url: bool) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i > chunk.len() {
                if !url {
                    out.push('=');
                }
                continue;
            }
            let c = ALPHABET[((n >> (18 - 6 * i)) & 63) as usize];
            out.push(match (url, c) {
                (true, b'+') => '-',
                (true, b'/') => '_',
                (_, c) => c as char,
            });
        }
    }
    out
}

/// Base64 with padding (RFC 4648 §4).
pub fn base64_standard(bytes: &[u8]) -> String {
    base64_with(bytes, false)
}

/// Base64url without padding (RFC 4648 §5), as PKCE and `state` use it.
pub fn base64_url(bytes: &[u8]) -> String {
    base64_with(bytes, true)
}

/// `application/x-www-form-urlencoded`, as both the sign-in address and the
/// token request take it.
fn form_encode(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v))).collect::<Vec<_>>().join("&")
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    fn hex(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(hi), Some(lo)) => {
                    out.push(hi * 16 + lo);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (percent_decode(k), percent_decode(v)),
            None => (percent_decode(p), String::new()),
        })
        .collect()
}

/// A provider's text, cut to one short line: it goes into messages on
/// screen, and must not be able to carry anything long or multi-line.
fn one_line(s: &str) -> String {
    let line = s.lines().next().unwrap_or_default().trim();
    let mut out: String = line.chars().take(160).collect();
    if line.chars().count() > 160 {
        out.push('…');
    }
    out
}

/// A token endpoint on this computer, for tests: answers each request with
/// the next of `answers` (a status and a JSON body) and hands back the form
/// bodies it received.
#[cfg(test)]
pub(crate) mod fake {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    pub(crate) fn token_endpoint(answers: Vec<(u16, String)>) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = format!("http://127.0.0.1:{}/token", listener.local_addr().unwrap().port());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for (status, body) in answers {
                let Ok((stream, _)) = listener.accept() else { return };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut length = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    let line = line.trim_end();
                    if line.is_empty() {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':') {
                        if name.eq_ignore_ascii_case("content-length") {
                            length = value.trim().parse().unwrap_or(0);
                        }
                    }
                }
                let mut form = vec![0u8; length];
                reader.read_exact(&mut form).unwrap();
                tx.send(String::from_utf8_lossy(&form).into_owned()).unwrap();
                let mut w = stream;
                let reply = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                w.write_all(reply.as_bytes()).unwrap();
            }
        });
        (url, rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn the_pkce_challenge_is_rfc_7636s() {
        // RFC 7636, appendix B.
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into());
        assert_eq!(pkce.challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        let fresh = Pkce::new();
        assert_eq!(fresh.verifier.len(), 43);
        assert_ne!(fresh.verifier, Pkce::new().verifier);
        assert!(!format!("{fresh:?}").contains(&fresh.verifier));
    }

    #[test]
    fn base64_is_rfc_4648s_and_xoauth2_is_microsofts_example() {
        let cases =
            [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE=")];
        for (plain, encoded) in cases {
            assert_eq!(base64_standard(plain.as_bytes()), encoded);
        }
        assert_eq!(base64_url(&[0xfb, 0xff]), "-_8");
        // Microsoft's documentation for IMAP with OAuth.
        assert_eq!(
            xoauth2("test@contoso.onmicrosoft.com", "EwBAAl3BAAUFFpUAo7J3Ve0bjLBWZWCclRC3EoAA"),
            "dXNlcj10ZXN0QGNvbnRvc28ub25taWNyb3NvZnQuY29tAWF1dGg9QmVhcmVyIEV3QkFBbDNCQUFVRkZwVUFvN0ozVmUwYmpMQldaV0NjbFJDM0VvQUEBAQ=="
        );
    }

    #[test]
    fn the_sign_in_address_carries_the_challenge_and_never_the_verifier() {
        let pkce = Pkce::new();
        let url = authorization_url(
            &Provider::microsoft(),
            "client-1",
            "http://localhost:4321",
            "st4te",
            &pkce,
            Some(" c@outlook.com "),
        );
        assert!(url.starts_with("https://login.microsoftonline.com/common/oauth2/v2.0/authorize?client_id=client-1&"));
        for part in [
            "response_type=code",
            "redirect_uri=http%3A%2F%2Flocalhost%3A4321",
            "scope=https%3A%2F%2Foutlook.office.com%2FIMAP.AccessAsUser.All%20offline_access",
            "state=st4te",
            "code_challenge_method=S256",
            "login_hint=c%40outlook.com",
        ] {
            assert!(url.contains(part), "{part} in {url}");
        }
        assert!(url.contains(&format!("code_challenge={}", pkce.challenge)));
        assert!(!url.contains(&pkce.verifier));
    }

    /// What a browser sends back, and what it gets.
    fn browse(port: u16, target: &str) -> String {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(format!("GET {target} HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n").as_bytes()).unwrap();
        let mut reply = String::new();
        s.read_to_string(&mut reply).unwrap();
        reply
    }

    #[test]
    fn the_browser_coming_back_with_this_sign_ins_code_ends_the_wait() {
        let lb = Loopback::bind().unwrap();
        assert_eq!(lb.redirect_uri(), format!("http://localhost:{}", lb.port()));
        let port = lb.port();
        let browser = std::thread::spawn(move || {
            let icon = browse(port, "/favicon.ico");
            let other = browse(port, "/?code=stolen&state=someone-else");
            let ours = browse(port, "/?code=M.C5%2F0_ab-c&state=st4te&session_state=x");
            (icon, other, ours)
        });
        let code = lb.wait_for_code("st4te", Instant::now() + Duration::from_secs(20), &|| false).unwrap();
        assert_eq!(code, "M.C5/0_ab-c");
        let (icon, other, ours) = browser.join().unwrap();
        assert!(icon.starts_with("HTTP/1.1 404"), "{icon}");
        assert!(other.starts_with("HTTP/1.1 400"), "a code for another sign-in is turned away: {other}");
        assert!(ours.starts_with("HTTP/1.1 200") && ours.contains("You're signed in"), "{ours}");
    }

    #[test]
    fn the_providers_refusal_ends_the_wait_with_its_reason_on_one_line() {
        let lb = Loopback::bind().unwrap();
        let port = lb.port();
        std::thread::spawn(move || {
            browse(port, "/?error=access_denied&error_description=The+user+said+no.%0D%0ATrace+ID%3A+123&state=st4te")
        });
        let err = lb.wait_for_code("st4te", Instant::now() + Duration::from_secs(20), &|| false).unwrap_err();
        assert!(matches!(&err, OAuthError::Denied(why) if why == "The user said no."), "{err:?}");
    }

    #[test]
    fn a_wait_can_be_stopped_and_runs_out() {
        let lb = Loopback::bind().unwrap();
        let far = Instant::now() + Duration::from_secs(60);
        assert!(matches!(lb.wait_for_code("s", far, &|| true), Err(OAuthError::Canceled)));
        assert!(matches!(lb.wait_for_code("s", Instant::now(), &|| false), Err(OAuthError::TimedOut)));
    }

    #[test]
    fn a_connection_that_sends_nothing_holds_up_neither_the_answer_nor_a_stop() {
        let lb = Loopback::bind().unwrap();
        let port = lb.port();
        let idle: Vec<_> = (0..3).map(|_| std::net::TcpStream::connect(("127.0.0.1", port)).unwrap()).collect();
        let browser = std::thread::spawn(move || browse(port, "/?code=abc&state=st4te"));
        let started = Instant::now();
        let code = lb.wait_for_code("st4te", Instant::now() + Duration::from_secs(20), &|| false).unwrap();
        assert_eq!(code, "abc");
        assert!(started.elapsed() < REQUEST_TIMEOUT, "answered while the silent connections were open");
        assert!(browser.join().unwrap().starts_with("HTTP/1.1 200"));

        let held = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let asked = Instant::now();
        let stop = move || asked.elapsed() > Duration::from_millis(200);
        let stopped = lb.wait_for_code("st4te", Instant::now() + Duration::from_secs(20), &stop);
        assert!(matches!(stopped, Err(OAuthError::Canceled)), "{stopped:?}");
        assert!(asked.elapsed() < Duration::from_secs(2), "a stop is not held up by an open connection");
        drop((idle, held));
    }

    #[test]
    fn a_code_is_exchanged_with_the_verifier_and_a_refresh_brings_a_new_token() {
        let (url, forms) = fake::token_endpoint(vec![
            (200, r#"{"token_type":"Bearer","access_token":"at-1","refresh_token":"rt-1","expires_in":3599}"#.into()),
            (200, r#"{"token_type":"Bearer","access_token":"at-2","refresh_token":"rt-2","expires_in":3599}"#.into()),
        ]);
        let provider = Provider { token_url: url, ..Provider::microsoft() };
        let pkce = Pkce::new();
        let tokens = exchange_code(&provider, "client-1", "the-code", "http://localhost:4321", &pkce).unwrap();
        assert_eq!(tokens.access_token, "at-1");
        assert_eq!(tokens.refresh_token.as_deref(), Some("rt-1"));
        assert_eq!(tokens.expires_in, 3599);
        assert!(!format!("{tokens:?}").contains("at-1"), "tokens never print");
        let form = forms.recv().unwrap();
        for part in [
            "client_id=client-1",
            "grant_type=authorization_code",
            "code=the-code",
            "redirect_uri=http%3A%2F%2Flocalhost%3A4321",
            &format!("code_verifier={}", pkce.verifier),
        ] {
            assert!(form.contains(part), "{part} in {form}");
        }

        let again = refresh(&provider, "client-1", "rt-1").unwrap();
        assert_eq!((again.access_token.as_str(), again.refresh_token.as_deref()), ("at-2", Some("rt-2")));
        let form = forms.recv().unwrap();
        assert!(form.contains("grant_type=refresh_token") && form.contains("refresh_token=rt-1"), "{form}");
    }

    #[test]
    fn signing_in_opens_the_browser_and_exchanges_what_it_brings_back() {
        let (url, forms) = fake::token_endpoint(vec![(
            200,
            r#"{"access_token":"at-1","refresh_token":"rt-1","expires_in":3600}"#.into(),
        )]);
        let provider = Provider { token_url: url, ..Provider::microsoft() };
        let opened = std::sync::Mutex::new(String::new());
        // The "browser": signs in and comes back to the address it was given.
        let open = |address: &str| {
            *opened.lock().unwrap() = address.to_string();
            let query = parse_query(address.split_once('?').unwrap().1);
            let port: u16 = query["redirect_uri"].rsplit(':').next().unwrap().parse().unwrap();
            let back = format!("/?code=the-code&state={}", query["state"]);
            std::thread::spawn(move || browse(port, &back));
            Ok(())
        };
        let tokens = sign_in(&provider, "client-1", Some("c@outlook.com"), &open, &|| false).unwrap();
        assert_eq!(tokens.access_token, "at-1");
        let address = opened.lock().unwrap().clone();
        let challenge = parse_query(address.split_once('?').unwrap().1)["code_challenge"].clone();
        let form = parse_query(&forms.recv().unwrap());
        assert_eq!(form["code"], "the-code");
        assert_eq!(
            Pkce::from_verifier(form["code_verifier"].clone()).challenge,
            challenge,
            "the verifier that goes with the challenge"
        );
        assert_eq!(form["redirect_uri"], parse_query(address.split_once('?').unwrap().1)["redirect_uri"]);

        let no_browser = sign_in(&provider, "client-1", None, &|_| Err("no browser\nat all".into()), &|| false);
        assert!(matches!(no_browser, Err(OAuthError::Browser(why)) if why == "no browser"));
    }

    #[test]
    fn a_lapsed_refresh_token_asks_for_a_new_sign_in_and_other_refusals_say_why() {
        let (url, _forms) = fake::token_endpoint(vec![
            (400, r#"{"error":"invalid_grant","error_description":"AADSTS70008: The refresh token has expired.\r\nTrace ID: 1"}"#.into()),
            (400, r#"{"error":"invalid_client","error_description":"AADSTS700016: Application not found.\r\nTrace ID: 2"}"#.into()),
            (502, "<html>bad gateway</html>".into()),
        ]);
        let provider = Provider { token_url: url, ..Provider::microsoft() };
        assert!(matches!(refresh(&provider, "c", "old"), Err(OAuthError::SignInAgain("Microsoft"))));
        let refused = refresh(&provider, "c", "old").unwrap_err();
        assert_eq!(refused.to_string(), "Microsoft turned the sign-in down: AADSTS700016: Application not found.");
        let garbled = refresh(&provider, "c", "old").unwrap_err();
        assert!(matches!(garbled, OAuthError::Refused("Microsoft", _)), "{garbled:?}");
    }
}
