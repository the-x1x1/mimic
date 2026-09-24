//! The `discord` connector: the messages in a Discord data package.
//!
//! Discord hands over "all of my data" (Settings → Data & Privacy → Request
//! all of my data) as `package.zip`: `account/user.json` is the account, and
//! `messages/c<channel id>/messages.json` — `messages.csv` in packages before
//! 2023 — holds every message that account sent in that channel, with
//! `channel.json` beside it and `messages/index.json` naming each channel.
//! Newer packages capitalise the folders and files (`Messages/`,
//! `Account/`); names are compared lower-cased, and the package is read
//! zipped, unzipped, or unzipped into one more folder.
//!
//! What this reads, and what it cannot:
//!
//! * **Only what the account wrote.** Nothing anyone else said is in the
//!   package, so every conversation here is the user's side of it: these
//!   teach Mimic how the user writes, and can be shown to a draft as messages
//!   the user sent, never as a reply to anything; none of them waits on the
//!   home screen.
//! * **Whose account it is** is the user's to say: its messages come from
//!   `discord:<account id>` — and from the account's email, when the package
//!   has it, which is the user's already if they gave it to Mimic. The check
//!   names the account (`one_writer`), and the import refuses until one of
//!   its addresses is the user's (`sole_author`): read as someone else's,
//!   every channel would wait for a reply.
//! * **Words, not attachments.** A message that was only a file or a picture
//!   has no words, and is counted and left out. Mentions (`<@123>`), channel
//!   links and custom emoji are written as `@someone`, `#channel` and
//!   `:name:`, since the package does not say who or what they were, and a
//!   timestamp tag (`<t:1700000000:f>`) as the time it shows, on this
//!   computer's clock.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use serde_json::Value;

use super::{
    validate_by_dry_run, AuthorRef, CommunicationSource, DiscoveredConversation, LocationKind, RawMessage, SourceError,
    SourceMetadata, SourceResult, ValidationReport, WriterAddress, WriterName,
};
use crate::db::repo_people::IdentifierInput;
use crate::db::IdentifierKind;

pub struct DiscordSource;

/// What an account id is prefixed with to make the address its messages
/// come from.
pub const ACCOUNT_PREFIX: &str = "discord:";

/// Why a package without its account is refused.
const NO_ACCOUNT: &str =
    "This package has no account in it (account/user.json), so I can't tell whose messages these are.";

/// Where a package is, for when the check finds none.
const WHERE_IT_IS: &str =
    "Choose the package.zip Discord sent, or the folder it unzipped to, with its messages and account folders.";

/// The most read from any one file of a package: its messages, not its media.
const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// The account the package is from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
}

impl Account {
    /// The writer of every message in the package.
    pub fn author(&self) -> AuthorRef {
        let mut identifiers =
            vec![IdentifierInput::new(IdentifierKind::AccountId, format!("{ACCOUNT_PREFIX}{}", self.id))];
        if let Some(email) = &self.email {
            identifiers.push(IdentifierInput::new(IdentifierKind::Email, email));
        }
        AuthorRef { display_name: self.name.clone(), identifiers }
    }
}

/// One message the account sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentMessage {
    pub id: String,
    pub sent_at: Option<String>,
    pub body: String,
}

/// One channel's messages: a direct message, a group, or a server's channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub messages: Vec<SentMessage>,
    /// Messages that were only an attachment.
    pub without_words: usize,
}

/// A whole package, as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub account: Option<Account>,
    pub channels: Vec<Channel>,
}

/// The files of a package, from its zip or its folder, by their path inside
/// it with `/` between the parts.
enum Files {
    Zip(zip::ZipArchive<std::fs::File>),
    Folder(PathBuf),
}

impl Files {
    fn open(location: &Path) -> SourceResult<(Files, Vec<String>)> {
        if location.is_dir() {
            let mut names = Vec::new();
            // Deep enough for a package in a folder of its own
            // (`package/messages/c1/messages.json`), and no deeper, so a
            // folder that is not a package is not walked to the bottom.
            let mut stack = vec![(location.to_path_buf(), 0)];
            while let Some((dir, depth)) = stack.pop() {
                for entry in std::fs::read_dir(&dir)? {
                    let path = entry?.path();
                    if path.is_dir() {
                        if depth < 3 {
                            stack.push((path, depth + 1));
                        }
                    } else if let Ok(rel) = path.strip_prefix(location) {
                        let parts: Vec<String> =
                            rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                        names.push(parts.join("/"));
                    }
                }
            }
            names.sort();
            return Ok((Files::Folder(location.to_path_buf()), names));
        }
        let file = std::fs::File::open(location)?;
        let archive = zip::ZipArchive::new(file)
            .map_err(|e| SourceError::Malformed(format!("This isn't a zip Discord could have made: {e}")))?;
        let mut names: Vec<String> = archive.file_names().map(str::to_string).collect();
        names.sort();
        Ok((Files::Zip(archive), names))
    }

    fn read(&mut self, name: &str) -> SourceResult<Vec<u8>> {
        let mut bytes = Vec::new();
        match self {
            Files::Zip(archive) => {
                let entry = archive.by_name(name).map_err(|e| SourceError::Malformed(e.to_string()))?;
                entry.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
            }
            Files::Folder(root) => {
                std::fs::File::open(root.join(name))?.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
            }
        }
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(SourceError::Malformed(format!("{name} is too large to be messages.")));
        }
        Ok(bytes)
    }
}

/// Where the package's own folders start, lower-cased — the root, or one
/// folder down when it was unzipped into one — found by `messages/index.json`
/// or a channel's messages, and not by anything else that happens to be
/// called messages. `None` when there is no package; an error when there are
/// two, rather than reading one of them.
fn root_of(names: &[String]) -> SourceResult<Option<String>> {
    let mut roots: Vec<String> = names
        .iter()
        .flat_map(|n| {
            let lower = n.to_ascii_lowercase();
            lower
                .match_indices("messages/")
                .filter_map(|(at, _)| {
                    let (before, after) = (&lower[..at], &lower[at + "messages/".len()..]);
                    let at_a_root = before.is_empty() || (before.ends_with('/') && before.matches('/').count() == 1);
                    let package = after == "index.json"
                        || channel_file(after).is_some_and(|(_, f)| f == "messages.json" || f == "messages.csv");
                    (at_a_root && package).then(|| before.to_string())
                })
                .collect::<Vec<_>>()
        })
        .collect();
    roots.sort();
    roots.dedup();
    match roots.len() {
        0 => Ok(None),
        1 => Ok(roots.pop()),
        _ => Err(SourceError::Malformed(
            "This folder holds more than one Discord package. Choose the one you want to read.".into(),
        )),
    }
}

/// A channel folder's id and file, from a lower-cased path under the
/// package's `messages/`: `c123/messages.json` gives ("123", "messages.json").
fn channel_file(rest: &str) -> Option<(&str, &str)> {
    let (dir, file) = rest.split_once('/')?;
    let id = dir.strip_prefix('c')?;
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) && !file.contains('/')).then_some((id, file))
}

/// Discord's timestamps as UTC RFC 3339: `2020-06-12 18:21:06.781000+00:00`
/// in older packages, `2023-05-01 10:00:00` (UTC) in newer.
fn utc(ts: &str) -> Option<String> {
    let ts = ts.trim();
    let at = DateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S%.f%:z")
        .or_else(|_| DateTime::parse_from_rfc3339(ts))
        .map(|t| t.with_timezone(&Utc))
        .or_else(|_| {
            NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S%.f")
                .or_else(|_| NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.f"))
                .map(|n| Utc.from_utc_datetime(&n))
        })
        .ok()?;
    Some(at.to_rfc3339_opts(SecondsFormat::Secs, true))
}

/// A message's text as the user would read it: a mention, a channel link or a
/// custom emoji written as the package cannot say it — `@someone`,
/// `#channel`, `:name:` — and a timestamp tag as the time it shows.
/// Anything else in angle brackets is the user's own, kept as written.
pub fn readable(contents: &str) -> String {
    let mut out = String::with_capacity(contents.len());
    let mut rest = contents;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        // Markup is short and has no space or second `<` in it, so a stray
        // `<` ("i <3 <@123>") is only itself, and what follows is still read.
        let end = after.find(|c: char| c == '>' || c == '<' || c.is_whitespace());
        match end.filter(|&e| after[e..].starts_with('>')).and_then(|e| markup(&after[..e]).map(|t| (e, t))) {
            Some((e, text)) => {
                out.push_str(&text);
                rest = &after[e + 1..];
            }
            None => {
                out.push('<');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// What one tag between `<` and `>` reads as, if it is Discord's markup.
fn markup(inner: &str) -> Option<String> {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if let Some(id) = inner.strip_prefix("@!").or_else(|| inner.strip_prefix('@')) {
        return match id.strip_prefix('&') {
            Some(role) => digits(role).then(|| "@role".to_string()),
            None => digits(id).then(|| "@someone".to_string()),
        };
    }
    if let Some(id) = inner.strip_prefix('#') {
        return digits(id).then(|| "#channel".to_string());
    }
    if let Some(at) = inner.strip_prefix("t:") {
        let (seconds, style) = at.split_once(':').unwrap_or((at, "f"));
        let at = Local.timestamp_opt(seconds.parse().ok().filter(|_| digits(seconds))?, 0).single()?;
        let format = match style {
            "t" => "%H:%M",
            "T" => "%H:%M:%S",
            "d" | "D" => "%Y-%m-%d",
            "f" | "F" | "R" => "%Y-%m-%d %H:%M",
            _ => return None,
        };
        return Some(at.format(format).to_string());
    }
    let emoji = inner.strip_prefix("a:").or_else(|| inner.strip_prefix(':'))?;
    emoji.rsplit_once(':').filter(|(name, id)| !name.is_empty() && digits(id)).map(|(name, _)| format!(":{name}:"))
}

/// The rows of a CSV file with a header, as maps from column to value.
/// Quoted fields may hold commas, doubled quotes and line breaks.
fn csv_rows(text: &str) -> Vec<HashMap<String, String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                // A line break inside a field is a line break, however the
                // file ended its lines.
                '\r' if chars.peek() == Some(&'\n') => {}
                '\r' => field.push('\n'),
                c => field.push(c),
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            c => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    let mut rows = rows.into_iter();
    let Some(header) = rows.next() else { return Vec::new() };
    let header: Vec<String> = header.iter().map(|h| h.trim().trim_start_matches('\u{feff}').to_string()).collect();
    rows.filter(|r| r.iter().any(|f| !f.is_empty()))
        .map(|r| header.iter().cloned().zip(r.into_iter().chain(std::iter::repeat(String::new()))).collect())
        .collect()
}

/// A field of a message, by any of the names packages have given it.
fn field<'a>(message: &'a HashMap<String, String>, names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|n| message.get(*n)).map(String::as_str)
}

/// The messages of one channel's file.
fn channel_messages(file: &str, bytes: &[u8]) -> SourceResult<Vec<HashMap<String, String>>> {
    let text = String::from_utf8_lossy(bytes);
    if file.eq_ignore_ascii_case("messages.csv") {
        return Ok(csv_rows(&text));
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| SourceError::Malformed(format!("A channel's messages.json could not be read: {e}")))?;
    let Some(items) = value.as_array() else {
        return Err(SourceError::Malformed("A channel's messages.json is not a list of messages.".into()));
    };
    Ok(items
        .iter()
        .filter_map(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(k, v)| {
                    let v = match v {
                        Value::String(s) => s.clone(),
                        Value::Null => String::new(),
                        other => other.to_string(),
                    };
                    (k.clone(), v)
                })
                .collect()
        })
        .collect())
}

/// What a channel is called: its line in `index.json` ("Direct Message with
/// ada", "general in My Server"), else what `channel.json` says, else its id.
fn channel_name(id: &str, index: Option<&Value>, meta: Option<&Value>) -> String {
    if let Some(name) = index.and_then(|i| i.get(id)).and_then(Value::as_str).filter(|n| !n.trim().is_empty()) {
        return name.trim().to_string();
    }
    if let Some(meta) = meta {
        let name = meta.get("name").and_then(Value::as_str);
        let guild = meta.get("guild").and_then(|g| g.get("name")).and_then(Value::as_str);
        match (name, guild) {
            (Some(n), Some(g)) => return format!("{n} in {g}"),
            (Some(n), None) => return n.to_string(),
            _ => {}
        }
    }
    format!("Discord channel {id}")
}

/// A package opened: its files, and each one's lower-cased path under the
/// package's own folders.
struct Opened {
    files: Files,
    by_path: HashMap<String, String>,
}

impl Opened {
    /// `None` when there is no package at `location`.
    fn at(location: &Path) -> SourceResult<Option<Opened>> {
        let (files, names) = Files::open(location)?;
        let Some(root) = root_of(&names)? else { return Ok(None) };
        let by_path = names
            .into_iter()
            .filter_map(|n| {
                let key = n.to_ascii_lowercase().strip_prefix(root.as_str())?.to_string();
                (!key.is_empty() && !key.ends_with('/')).then_some((key, n))
            })
            .collect();
        Ok(Some(Opened { files, by_path }))
    }

    fn json(&mut self, key: &str) -> Option<Value> {
        let name = self.by_path.get(key)?;
        serde_json::from_slice(&self.files.read(name).ok()?).ok()
    }

    /// The account, from `account/user.json`.
    fn account(&mut self) -> Option<Account> {
        let u = self.json("account/user.json")?;
        let id = match u.get("id")? {
            Value::String(s) => s.trim().to_string(),
            Value::Number(n) => n.to_string(),
            _ => return None,
        };
        if id.is_empty() {
            return None;
        }
        let name = ["global_name", "username"]
            .iter()
            .find_map(|k| u.get(*k).and_then(Value::as_str).filter(|s| !s.trim().is_empty()))
            .unwrap_or("Your Discord account")
            .to_string();
        let email = u.get("email").and_then(Value::as_str).filter(|e| e.contains('@')).map(str::to_string);
        Some(Account { id, name, email })
    }
}

/// Read a package: its account, and every channel's messages, oldest first.
pub fn read_package(location: &Path) -> SourceResult<Package> {
    let Some(mut opened) = Opened::at(location)? else {
        return Ok(Package { account: None, channels: Vec::new() });
    };
    let account = opened.account();
    let index = opened.json("messages/index.json");
    let mut keys: Vec<(String, String)> = opened.by_path.iter().map(|(k, n)| (k.clone(), n.clone())).collect();
    keys.sort();
    let mut channels = Vec::new();
    for (key, name) in keys {
        let Some(rest) = key.strip_prefix("messages/") else { continue };
        let Some((id, file)) = channel_file(rest) else { continue };
        if file != "messages.json" && file != "messages.csv" {
            continue;
        }
        // A package that has both keeps the JSON.
        if file == "messages.csv" && opened.by_path.contains_key(&format!("messages/c{id}/messages.json")) {
            continue;
        }
        let meta = opened.json(&format!("messages/c{id}/channel.json"));
        let rows = channel_messages(file, &opened.files.read(&name)?)?;
        let mut channel = Channel {
            id: id.to_string(),
            name: channel_name(id, index.as_ref(), meta.as_ref()),
            messages: Vec::new(),
            without_words: 0,
        };
        for row in rows {
            let Some(message_id) = field(&row, &["ID", "Id", "id"]).map(str::trim).filter(|s| !s.is_empty()) else {
                continue;
            };
            let body = readable(field(&row, &["Contents", "contents", "Content"]).unwrap_or("")).trim().to_string();
            if body.is_empty() {
                channel.without_words += 1;
                continue;
            }
            channel.messages.push(SentMessage {
                id: message_id.to_string(),
                sent_at: field(&row, &["Timestamp", "timestamp"]).and_then(utc),
                body,
            });
        }
        // Discord's ids grow with time: oldest first, whatever order the file had.
        channel
            .messages
            .sort_by(|a, b| (a.id.len(), a.id.as_str(), &a.sent_at).cmp(&(b.id.len(), b.id.as_str(), &b.sent_at)));
        channels.push(channel);
    }
    Ok(Package { account, channels })
}

impl CommunicationSource for DiscordSource {
    fn metadata(&self) -> SourceMetadata {
        SourceMetadata {
            connector: "discord",
            display_name: "Discord data package",
            channel: "chat",
            description: "The package.zip Discord sends when you request all of your data, or the folder it unzips to. It holds only what you wrote, so it teaches me how you write, not how you reply.",
            location_kind: LocationKind::FileOrFolder,
            extensions: &["zip"],
        }
    }

    fn discover(&self, location: &Path) -> SourceResult<Vec<PathBuf>> {
        Ok(if location.exists() { vec![location.to_path_buf()] } else { Vec::new() })
    }

    fn validate(&self, location: &Path) -> SourceResult<ValidationReport> {
        let mut report = validate_by_dry_run(self, location)?;
        let package = read_package(location)?;
        report.one_writer = true;
        let refused = match &package.account {
            Some(account) => {
                let author = account.author();
                let id = &author.identifiers[0];
                let messages = package.channels.iter().map(|c| c.messages.len()).sum();
                report.names = vec![WriterName {
                    name: account.name.clone(),
                    kind: id.kind.as_str().to_string(),
                    value: id.value.clone(),
                    normalized: id.normalized(),
                    messages,
                    chat_named_after: false,
                    also: author.identifiers[1..].iter().map(WriterAddress::of).collect(),
                }];
                None
            }
            None if !package.channels.is_empty() => Some(NO_ACCOUNT.to_string()),
            None => Some(format!("I can't find a Discord package here. {WHERE_IT_IS}")),
        };
        // Refused, the one reason is all there is to say: nothing was read,
        // so nothing the dry run says about what was read applies.
        if let Some(reason) = refused {
            report.ok = false;
            report.blockers = vec![reason];
            report.warnings.clear();
            return Ok(report);
        }
        report.warnings.push(
            "A Discord package holds only what you wrote — nothing anyone else said — so it teaches me how you write, not how you reply to anyone.".into(),
        );
        let without: usize = package.channels.iter().map(|c| c.without_words).sum();
        if without > 0 {
            report.warnings.push(format!(
                "{} only a file or a picture, with no words, so {} left out.",
                if without == 1 { "1 message was".to_string() } else { format!("{without} messages were") },
                if without == 1 { "it's" } else { "they're" },
            ));
        }
        Ok(report)
    }

    fn import(
        &self,
        location: &Path,
        sink: &mut dyn FnMut(DiscoveredConversation) -> SourceResult<()>,
    ) -> SourceResult<()> {
        let package = read_package(location)?;
        let Some(account) = package.account else { return Ok(()) };
        let author = account.author();
        for channel in package.channels {
            if channel.messages.is_empty() {
                continue;
            }
            let messages = channel
                .messages
                .into_iter()
                .map(|m| RawMessage {
                    external_id: format!("{ACCOUNT_PREFIX}{}", m.id),
                    author: author.clone(),
                    sent_at: m.sent_at,
                    body: m.body,
                    metadata: Value::Null,
                })
                .collect();
            sink(DiscoveredConversation {
                external_id: format!("{ACCOUNT_PREFIX}channel:{}", channel.id),
                subject: Some(channel.name),
                channel: "chat".into(),
                messages,
            })?;
        }
        Ok(())
    }

    /// Always the account: a package is one writer's, so one with no
    /// account — or a location that no longer holds a package — is refused
    /// rather than imported as nothing.
    fn sole_author(&self, location: &Path) -> SourceResult<Option<AuthorRef>> {
        let gone = || {
            SourceError::Malformed(format!(
                "The Discord package isn't at {} any more. Put it back there and import again.",
                location.display()
            ))
        };
        if !location.exists() {
            return Err(gone());
        }
        let Some(mut opened) = Opened::at(location)? else { return Err(gone()) };
        match opened.account() {
            Some(account) => Ok(Some(account.author())),
            None => Err(SourceError::Malformed(NO_ACCOUNT.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn mentions_channels_and_custom_emoji_are_written_as_the_package_can_say_them() {
        assert_eq!(
            readable("hey <@123> and <@!456>, see <#789> <:party:111> <a:wave:222>"),
            "hey @someone and @someone, see #channel :party: :wave:"
        );
        assert_eq!(readable("ping <@&42> now"), "ping @role now");
        // Anything else in angle brackets is the user's.
        assert_eq!(
            readable("i <3 this <b>really</b> <https://example.com>"),
            "i <3 this <b>really</b> <https://example.com>"
        );
        assert_eq!(readable("unclosed <@12"), "unclosed <@12");
        // A stray `<` is only itself, and what follows it is still read.
        assert_eq!(readable("i <3 <@123>, <<#7>> x<"), "i <3 @someone, <#channel> x<");
        assert_eq!(readable("a < b <@1 > c"), "a < b <@1 > c");
    }

    #[test]
    fn a_timestamp_tag_is_the_time_it_shows_on_this_computer() {
        let at = Local.timestamp_opt(1_700_000_000, 0).single().unwrap();
        assert_eq!(readable("from <t:1700000000:R> on"), format!("from {} on", at.format("%Y-%m-%d %H:%M")));
        assert_eq!(readable("<t:1700000000>"), at.format("%Y-%m-%d %H:%M").to_string());
        assert_eq!(readable("<t:1700000000:D>"), at.format("%Y-%m-%d").to_string());
        assert_eq!(readable("<t:1700000000:t>"), at.format("%H:%M").to_string());
        // Not a time Discord writes: kept as written.
        assert_eq!(readable("<t:soon> <t:1700000000:x>"), "<t:soon> <t:1700000000:x>");
    }

    #[test]
    fn a_csv_is_read_with_its_quoted_commas_quotes_and_line_breaks() {
        let rows = csv_rows("ID,Timestamp,Contents,Attachments\r\n1,2020-06-12 18:21:06.781000+00:00,\"hi, you\",\n2,2020-06-12 18:22:00.000000+00:00,\"she said \"\"yes\"\"\nand left\",https://cdn/x.png\n");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["Contents"], "hi, you");
        assert_eq!(rows[1]["Contents"], "she said \"yes\"\nand left");
        assert_eq!(rows[1]["Attachments"], "https://cdn/x.png");
        // A line break inside a field is a line break, whatever ended the lines.
        let rows = csv_rows("ID,Contents\r\n1,\"one\r\ntwo\rthree\"\r\n");
        assert_eq!(rows[0]["Contents"], "one\ntwo\nthree");
    }

    #[test]
    fn discord_times_are_read_old_and_new() {
        assert_eq!(utc("2020-06-12 18:21:06.781000+00:00").as_deref(), Some("2020-06-12T18:21:06Z"));
        assert_eq!(utc("2020-06-12 20:21:06+02:00").as_deref(), Some("2020-06-12T18:21:06Z"));
        assert_eq!(utc("2023-05-01 10:00:00").as_deref(), Some("2023-05-01T10:00:00Z"));
        assert_eq!(utc("2023-05-01T10:00:00.5+00:00").as_deref(), Some("2023-05-01T10:00:00Z"));
        assert_eq!(utc("yesterday"), None);
    }

    /// A package; with `capital`, named as newer packages name things, and
    /// then some (`C222/Channel.json`).
    fn package(dir: &Path, capital: bool) {
        let m = if capital { "Messages" } else { "messages" };
        let a = if capital { "Account" } else { "account" };
        let c222 = if capital { "C222" } else { "c222" };
        let channel = if capital { "Channel.json" } else { "channel.json" };
        std::fs::create_dir_all(dir.join(a)).unwrap();
        std::fs::create_dir_all(dir.join(m).join("c111")).unwrap();
        std::fs::create_dir_all(dir.join(m).join(c222)).unwrap();
        std::fs::write(
            dir.join(a).join("user.json"),
            r#"{"id": "90001", "username": "cee", "global_name": "C", "email": "c@example.com"}"#,
        )
        .unwrap();
        std::fs::write(dir.join(m).join("index.json"), r#"{"111": "Direct Message with ada", "222": null}"#).unwrap();
        std::fs::write(
            dir.join(m).join("c111").join("messages.json"),
            r#"[{"ID": 1003, "Timestamp": "2023-05-01 10:05:00", "Contents": "see you <@55>", "Attachments": ""},
                {"ID": 1001, "Timestamp": "2023-05-01 10:00:00", "Contents": "drinks friday?", "Attachments": ""},
                {"ID": 1002, "Timestamp": "2023-05-01 10:01:00", "Contents": "", "Attachments": "https://cdn/p.png"}]"#,
        )
        .unwrap();
        std::fs::write(
            dir.join(m).join(c222).join(channel),
            r#"{"id": "222", "type": 0, "name": "general", "guild": {"id": "9", "name": "Book club"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join(m).join(c222).join("messages.csv"),
            "ID,Timestamp,Contents,Attachments\n2001,2020-06-12 18:21:06.781000+00:00,\"chapter 3, anyone?\",\n",
        )
        .unwrap();
    }

    #[test]
    fn a_package_is_read_from_its_folder_whatever_its_folders_are_called() {
        for capital in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("package");
            package(&root, capital);
            // Unzipped into a folder of its own: the package is found inside.
            let read = read_package(dir.path()).unwrap();
            let account = read.account.unwrap();
            assert_eq!(
                (account.id.as_str(), account.name.as_str(), account.email.as_deref()),
                ("90001", "C", Some("c@example.com"))
            );
            let channels: Vec<(&str, Vec<&str>, usize)> = read
                .channels
                .iter()
                .map(|c| (c.name.as_str(), c.messages.iter().map(|m| m.body.as_str()).collect(), c.without_words))
                .collect();
            assert_eq!(
                channels,
                [
                    ("Direct Message with ada", vec!["drinks friday?", "see you @someone"], 1),
                    ("general in Book club", vec!["chapter 3, anyone?"], 0),
                ],
                "capital folders: {capital}"
            );
            assert_eq!(read.channels[1].messages[0].sent_at.as_deref(), Some("2020-06-12T18:21:06Z"));
        }
    }

    #[test]
    fn a_package_zip_is_read_and_checked() {
        let dir = tempfile::tempdir().unwrap();
        let unzipped = dir.path().join("unzipped");
        package(&unzipped, false);
        let path = dir.path().join("package.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for rel in [
            "account/user.json",
            "messages/index.json",
            "messages/c111/messages.json",
            "messages/c222/channel.json",
            "messages/c222/messages.csv",
        ] {
            zip.start_file(rel, options).unwrap();
            zip.write_all(&std::fs::read(unzipped.join(rel)).unwrap()).unwrap();
        }
        zip.start_file("activity/analytics/events-2023.json", options).unwrap();
        zip.write_all(b"{}").unwrap();
        zip.finish().unwrap();

        let report = DiscordSource.validate(&path).unwrap();
        assert!(report.ok, "{report:?}");
        assert_eq!((report.conversations, report.messages), (2, 3));
        let name = &report.names[0];
        assert_eq!(
            (name.name.as_str(), name.kind.as_str(), name.value.as_str(), name.messages),
            ("C", "account_id", "discord:90001", 3)
        );
        assert!(report.one_writer, "the import waits until the account is the user's");
        let also: Vec<(&str, &str)> = name.also.iter().map(|a| (a.kind.as_str(), a.normalized.as_str())).collect();
        assert_eq!(also, [("email", "c@example.com")], "the email makes it the user's as well");
        let author = DiscordSource.sole_author(&path).unwrap().unwrap();
        assert_eq!(
            author,
            Account { id: "90001".into(), name: "C".into(), email: Some("c@example.com".into()) }.author()
        );
        assert!(report.warnings.iter().any(|w| w.contains("only what you wrote")));
        assert!(report.warnings.iter().any(|w| w.starts_with("1 message was only a file")));

        let mut seen = Vec::new();
        DiscordSource
            .import(&path, &mut |c| {
                seen.push((
                    c.external_id,
                    c.subject,
                    c.messages.iter().map(|m| m.external_id.clone()).collect::<Vec<_>>(),
                ));
                Ok(())
            })
            .unwrap();
        assert_eq!(
            seen[0],
            (
                "discord:channel:111".to_string(),
                Some("Direct Message with ada".to_string()),
                vec!["discord:1001".to_string(), "discord:1003".to_string()]
            )
        );
    }

    #[test]
    fn a_package_without_its_account_or_a_folder_without_a_package_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        package(dir.path(), false);
        std::fs::remove_file(dir.path().join("account").join("user.json")).unwrap();
        let report = DiscordSource.validate(dir.path()).unwrap();
        assert!(!report.ok);
        assert_eq!(report.blockers, [NO_ACCOUNT], "one reason, and nothing about what was not read");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        let empty = tempfile::tempdir().unwrap();
        std::fs::write(empty.path().join("notes.txt"), "nothing here").unwrap();
        let report = DiscordSource.validate(empty.path()).unwrap();
        assert!(!report.ok);
        assert_eq!(report.blockers.len(), 1);
        assert!(report.blockers[0].starts_with("I can't find a Discord package here"), "{report:?}");
        assert!(report.warnings.is_empty() && report.names.is_empty());
        // Nothing to import is refused, not imported as nothing.
        let Err(SourceError::Malformed(gone)) = DiscordSource.sole_author(empty.path()) else {
            panic!("a folder without a package was taken for one")
        };
        assert!(gone.contains("Put it back there"), "{gone}");
        // Moved or deleted: the same, not the system's own words.
        let Err(SourceError::Malformed(missing)) = DiscordSource.sole_author(&empty.path().join("package.zip")) else {
            panic!("a package that is not there was taken for one")
        };
        assert!(missing.contains("package.zip any more"), "{missing}");
        assert!(matches!(DiscordSource.sole_author(dir.path()), Err(SourceError::Malformed(m)) if m == NO_ACCOUNT));
    }

    #[test]
    fn a_package_is_found_one_folder_down_and_no_deeper_and_two_are_not_read_as_one() {
        let dir = tempfile::tempdir().unwrap();
        package(&dir.path().join("a").join("b"), false);
        let read = read_package(dir.path()).unwrap();
        assert!(read.account.is_none() && read.channels.is_empty(), "two folders down is not the package");
        assert_eq!(read_package(&dir.path().join("a")).unwrap().channels.len(), 2);

        let two = tempfile::tempdir().unwrap();
        package(&two.path().join("mine"), false);
        package(&two.path().join("old"), false);
        let Err(SourceError::Malformed(why)) = read_package(two.path()) else { panic!("two packages read as one") };
        assert!(why.contains("more than one Discord package"), "{why}");
        // Something else called messages is not a package.
        assert!(root_of(&["old_messages/c1/messages.json".to_string()]).unwrap().is_none());
    }
}
