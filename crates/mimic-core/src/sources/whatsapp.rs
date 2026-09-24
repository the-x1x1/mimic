//! The `whatsapp` connector: a chat exported from WhatsApp with "Export chat".
//!
//! WhatsApp exports one chat at a time: a text file named after the chat
//! ("WhatsApp Chat with Ada.txt") on Android, or a zip holding `_chat.txt`
//! ("WhatsApp Chat - Ada.zip") from an iPhone, and from Android when media is
//! included. Every message starts on a line of its own with the date, the
//! time and who wrote it; a message of several lines runs on until the next
//! such line:
//!
//! ```text
//! 12/31/20, 9:41 PM - Ada: Drinks on Friday?        (Android)
//! [31/12/2020, 21:41:05] Ada: Drinks on Friday?     (iPhone)
//! 31.12.20, 21:41 - Ada: Drinks on Friday?          (Android, German)
//! ```
//!
//! What this reads, and what it cannot:
//!
//! * **Who wrote it is a name**, as saved on the phone the chat was exported
//!   from — no address. So a writer is known by the handle `whatsapp:<name>`
//!   (a writer shown as a phone number, someone not saved, by that number).
//!   The user's own messages are theirs once they have said which name is
//!   theirs — the handle is added as one of their addresses, as any other
//!   address is — and until then they are someone else's.
//! * **Dates carry no order and no time zone.** Whether `3/4/21` is the 3rd
//!   of April or the 4th of March is read from the chat itself: a day past 12
//!   settles it, and so does which reading keeps the messages in order; when
//!   neither does, it is read day first and the check says so. Times are read
//!   in this computer's time zone.
//! * **Only words.** A photo, a voice message, a sticker, a missed call, a
//!   shared location, a poll or a deleted message has no words of the user's
//!   in it, and is counted and left out. Lines WhatsApp writes itself ("Ada
//!   added Bob", "Messages and calls are end-to-end encrypted") have no
//!   writer and are left out too.
//! * **Nothing marks a message**, so each is known by a digest of its date,
//!   writer and words: exporting the chat again and importing it adds only
//!   what is new.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime, SecondsFormat, TimeZone, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    validate_by_dry_run, AuthorRef, CommunicationSource, DiscoveredConversation, LocationKind, RawMessage, SourceError,
    SourceMetadata, SourceResult, ValidationReport, WriterName,
};
use crate::db::repo_people::IdentifierInput;
use crate::db::IdentifierKind;

pub struct WhatsAppSource;

/// What a writer's name is prefixed with to make their handle.
pub const HANDLE_PREFIX: &str = "whatsapp:";

/// The largest chat text read: a chat's words, not its photos.
const MAX_TEXT_BYTES: u64 = 512 * 1024 * 1024;

/// The handle a writer is known by: their name as WhatsApp shows it.
pub fn handle_of(name: &str) -> String {
    format!("{HANDLE_PREFIX}{name}")
}

/// Which of a date's three numbers is the day, and which the month.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateOrder {
    /// `2021-01-13`
    YearFirst,
    /// `13/01/21`
    DayFirst,
    /// `01/13/21`
    MonthFirst,
}

/// How the dates of a chat were read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateReading {
    /// The chat itself says which order its dates are in.
    Read(DateOrder),
    /// Either order fits every date and keeps the messages in order; this
    /// one was taken.
    Guessed(DateOrder),
    /// No order makes every date a date.
    Unreadable,
}

/// A date and time as a chat writes them: three numbers, not yet known to
/// be which of day, month and year, and a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct When {
    numbers: (u32, u32, u32),
    /// The first number has four digits: the year comes first.
    year_first: bool,
    /// The last number has two digits.
    short_year: bool,
    time: NaiveTime,
}

impl When {
    fn read(self, order: DateOrder) -> Option<NaiveDateTime> {
        let (a, b, c) = self.numbers;
        let year = |y: u32| if self.short_year { 2000 + y } else { y };
        let (y, m, d) = match order {
            DateOrder::YearFirst => (a, b, c),
            DateOrder::DayFirst => (year(c), b, a),
            DateOrder::MonthFirst => (year(c), a, b),
        };
        Some(NaiveDate::from_ymd_opt(y as i32, m, d)?.and_time(self.time))
    }
}

/// One line that starts a message: its date and time as written, and the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp<'a> {
    when: When,
    rest: &'a str,
}

/// One message of a chat, as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub writer: String,
    pub body: String,
    /// When it was sent, in the phone's time, if the dates could be read.
    pub at: Option<NaiveDateTime>,
    /// A digest of the date as written, the writer and the words, with a
    /// count for a message said twice in the same minute: the same every time
    /// the same chat is read, whatever its dates turn out to mean.
    pub key: String,
}

/// A chat, as read from one export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chat {
    /// Who the chat is with, or the group's name, from the file's name.
    pub title: String,
    pub messages: Vec<ChatMessage>,
    pub dates: DateReading,
    /// Photos, voice messages, stickers, calls, locations, polls and the like.
    pub attachments: usize,
    pub deleted: usize,
    /// Lines WhatsApp wrote itself.
    pub notices: usize,
}

fn number(s: &str, max_digits: usize) -> Option<(u32, usize, &str)> {
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > max_digits {
        return None;
    }
    Some((s[..digits].parse().ok()?, digits, &s[digits..]))
}

/// A space of any width.
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\u{a0}' | '\u{202f}')
}

/// `token` at the start of `s`, ignoring case and taking any space for a
/// space: what follows it.
fn eat<'a>(s: &'a str, token: &str) -> Option<&'a str> {
    let mut chars = s.char_indices();
    for want in token.chars() {
        let (_, got) = chars.next()?;
        let got = if is_space(got) { ' ' } else { got.to_ascii_lowercase() };
        if got != want {
            return None;
        }
    }
    Some(chars.next().map_or("", |(i, _)| &s[i..]))
}

/// `AM`, `pm`, `a.m.`, `p. m.`, after a space of any width: whether it is
/// after noon, and what follows it.
fn meridiem(s: &str) -> (Option<bool>, &str) {
    let t = s.trim_start_matches(is_space);
    for (token, pm) in [("a. m.", false), ("p. m.", true), ("a.m.", false), ("p.m.", true), ("am", false), ("pm", true)]
    {
        if let Some(rest) = eat(t, token) {
            return (Some(pm), rest);
        }
    }
    (None, s)
}

/// The date, time and the rest of a line that starts a message, or None for a
/// line that carries on the one before.
fn stamp(line: &str) -> Option<Stamp<'_>> {
    let s = line.trim_start_matches(['\u{200e}', '\u{200f}', '\u{feff}']);
    let (bracketed, s) = match s.strip_prefix('[') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    let (a, a_digits, s) = number(s, 4)?;
    let sep = s.chars().next().filter(|c| matches!(c, '/' | '.' | '-'))?;
    let (b, _, s) = number(&s[1..], 2)?;
    let s = s.strip_prefix(sep)?;
    let (c, c_digits, s) = number(s, 4)?;
    // A year has two digits or four, and comes first or last, not both.
    let year_first = a_digits == 4;
    if a_digits == 3 || c_digits == 3 || (year_first && c_digits == 4) || (!year_first && c_digits == 1) {
        return None;
    }
    let s = s.strip_prefix(',').unwrap_or(s);
    let s = s.strip_prefix(' ')?;
    let (hour, _, s) = number(s, 2)?;
    let s = s.strip_prefix(':')?;
    let (minute, minute_digits, s) = number(s, 2)?;
    if minute_digits != 2 {
        return None;
    }
    let (second, s) = match s.strip_prefix(':') {
        Some(rest) => match number(rest, 2)? {
            (second, 2, rest) => (second, rest),
            _ => return None,
        },
        None => (0, s),
    };
    let (pm, s) = meridiem(s);
    let hour = match pm {
        None => hour,
        Some(_) if hour == 0 || hour > 12 => return None,
        Some(pm) => hour % 12 + if pm { 12 } else { 0 },
    };
    let time = NaiveTime::from_hms_opt(hour, minute, second)?;
    let rest = if bracketed {
        s.strip_prefix(']')?.strip_prefix(' ')?
    } else {
        s.strip_prefix(" - ").or_else(|| s.strip_prefix(" \u{2013} "))?
    };
    Some(Stamp { when: When { numbers: (a, b, c), year_first, short_year: c_digits <= 2, time }, rest })
}

/// Who wrote a message and what they wrote, or None for a line WhatsApp
/// wrote itself ("Ada added Bob"), which has no writer before a colon.
fn writer(rest: &str) -> Option<(&str, &str)> {
    let rest = rest.trim_start_matches('\u{200e}');
    let (name, body) = match rest.find(": ") {
        Some(at) => (&rest[..at], &rest[at + 2..]),
        None => (rest.strip_suffix(':')?, ""),
    };
    let name = name.trim().trim_matches(['\u{200e}', '\u{202a}', '\u{202c}']).trim();
    // A notice quoting something ("Ada changed the subject to "Plans: Friday"",
    // „…“ in German, «…» in French, 「…」 in Japanese) is not a writer. An
    // apostrophe is: "O’Brien".
    const QUOTES: [char; 13] = [
        '"', '\u{201c}', '\u{201d}', '\u{201e}', '\u{201a}', '\u{ab}', '\u{bb}', '\u{2039}', '\u{203a}', '\u{300c}',
        '\u{300d}', '\u{300e}', '\u{300f}',
    ];
    if name.is_empty() || name.chars().count() > 60 || name.contains(QUOTES) {
        return None;
    }
    Some((name, body))
}

/// Which order a chat's dates are in, from the dates themselves.
fn date_reading(dates: &[When]) -> DateReading {
    if dates.is_empty() {
        return DateReading::Unreadable;
    }
    if dates.iter().all(|w| w.year_first) {
        return if dates.iter().all(|w| w.read(DateOrder::YearFirst).is_some()) {
            DateReading::Read(DateOrder::YearFirst)
        } else {
            DateReading::Unreadable
        };
    }
    // Read this way, how often a message would come before the one it
    // follows; None when a date would not be a date at all.
    let out_of_order = |order: DateOrder| -> Option<usize> {
        let mut last: Option<NaiveDateTime> = None;
        let mut count = 0;
        for w in dates {
            let at = w.read(order)?;
            if last.is_some_and(|l| at < l) {
                count += 1;
            }
            last = Some(at);
        }
        Some(count)
    };
    match (out_of_order(DateOrder::DayFirst), out_of_order(DateOrder::MonthFirst)) {
        (None, None) => DateReading::Unreadable,
        (Some(_), None) => DateReading::Read(DateOrder::DayFirst),
        (None, Some(_)) => DateReading::Read(DateOrder::MonthFirst),
        (Some(d), Some(m)) if d < m => DateReading::Read(DateOrder::DayFirst),
        (Some(d), Some(m)) if m < d => DateReading::Read(DateOrder::MonthFirst),
        _ => DateReading::Guessed(DateOrder::DayFirst),
    }
}

/// What a message's body is, once WhatsApp's own marks are taken off.
enum Said {
    Words(String),
    Attachment,
    Deleted,
}

/// What WhatsApp puts after a message edited, in the languages whose
/// exports are read here.
const EDITED: &[&str] = &[
    "<This message was edited>",
    "<Diese Nachricht wurde bearbeitet>",
    "<Se editó este mensaje.>",
    "<Ce message a été modifié>",
    "<Mensagem editada>",
    "<Questo messaggio è stato modificato>",
    "<Dit bericht is bewerkt>",
];

/// What WhatsApp writes in place of a message that was deleted.
const DELETED: &[&str] = &[
    "this message was deleted",
    "you deleted this message",
    "diese nachricht wurde gelöscht",
    "du hast diese nachricht gelöscht",
    "se eliminó este mensaje",
    "eliminaste este mensaje",
    "ce message a été supprimé",
    "vous avez supprimé ce message",
    "mensagem apagada",
    "você apagou esta mensagem",
    "questo messaggio è stato eliminato",
    "hai eliminato questo messaggio",
    "dit bericht is verwijderd",
    "je hebt dit bericht verwijderd",
];

/// What WhatsApp writes in place of something with no words: a photo, a
/// voice message, a call. (On Android, any `<…>` alone is one of these, in
/// whatever language: `<Media omitted>`, `<Medien ausgeschlossen>`.)
const ATTACHMENTS: &[&str] = &[
    "image omitted",
    "video omitted",
    "audio omitted",
    "sticker omitted",
    "gif omitted",
    "document omitted",
    "contact card omitted",
    "missed voice call",
    "missed video call",
    "waiting for this message. this may take a while",
    "null",
];

fn said(body: &str) -> Said {
    let mut text = body.trim().trim_start_matches('\u{200e}').trim().to_string();
    for edited in EDITED {
        if let Some(rest) = text.strip_suffix(edited) {
            text = rest.trim_end().trim_end_matches('\u{200e}').trim_end().to_string();
        }
    }
    // A file sent with a caption, on Android: the file on the first line.
    if let Some((first, caption)) = text.split_once('\n') {
        if first.trim_end().ends_with("(file attached)") {
            let caption = caption.trim();
            return if caption.is_empty() { Said::Attachment } else { Said::Words(caption.to_string()) };
        }
    }
    let lower = text.to_lowercase();
    let lower = lower.trim_end_matches('.');
    let placeholder =
        text.starts_with('<') && text.ends_with('>') && !text.contains('\n') && text.chars().count() <= 60;
    if placeholder
        || ATTACHMENTS.contains(&lower)
        || lower.starts_with("<attached:")
        || lower.ends_with("(file attached)")
        || lower.ends_with("document omitted")
        || lower.starts_with("location: https://")
        || text.starts_with("POLL:")
    {
        return Said::Attachment;
    }
    if DELETED.contains(&lower) {
        return Said::Deleted;
    }
    if text.is_empty() {
        Said::Attachment
    } else {
        Said::Words(text)
    }
}

/// Who a chat is with, from its file's name in any of the usual languages:
/// "WhatsApp Chat with Ada", "WhatsApp Chat - Ada", "WhatsApp-Chat mit Ada",
/// "Chat de WhatsApp con Ada", "Discussion WhatsApp avec Ada". Anything else
/// is its own name. A second download's " (1)" is left off.
pub fn chat_title(stem: &str) -> String {
    let mut s = stem.trim();
    if let Some(open) = s.rfind(" (") {
        let inside = &s[open + 2..];
        if let Some(n) = inside.strip_suffix(')') {
            if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) {
                s = s[..open].trim_end();
            }
        }
    }
    // ASCII case folding keeps every byte where it was.
    let lower = s.to_ascii_lowercase();
    if let Some(at) = lower.find("whatsapp") {
        let after = &lower[at + "whatsapp".len()..];
        let found =
            [" with ", " - ", " \u{2013} ", " mit ", " con ", " avec ", " com ", " met ", " med ", " z ", " ile "]
                .iter()
                .filter_map(|sep| after.find(sep).map(|i| (i, sep.len())))
                .min();
        if let Some((i, len)) = found {
            let title = s[at + "whatsapp".len() + i + len..].trim();
            if !title.is_empty() {
                return title.to_string();
            }
        }
    }
    s.to_string()
}

/// A writer as the importer knows them: a phone number when that is all the
/// phone had, and otherwise the handle made from their name.
pub fn author(name: &str) -> AuthorRef {
    let name = name.trim_matches(['\u{202a}', '\u{202c}', '\u{200e}']).trim();
    let digits = name.bytes().filter(u8::is_ascii_digit).count();
    let phone = digits >= 7
        && name.starts_with(|c: char| c == '+' || c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_digit() || " +-().\u{a0}".contains(c));
    let id = if phone {
        IdentifierInput::new(IdentifierKind::Phone, name)
    } else {
        IdentifierInput::new(IdentifierKind::Handle, handle_of(name))
    };
    AuthorRef { display_name: name.to_string(), identifiers: vec![id] }
}

/// Read one chat's text. `title` goes into every message's key, so the
/// same words at the same minute in two chats are two messages.
pub fn read_chat(text: &str, title: String) -> Chat {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    struct Entry {
        when: When,
        /// None for a line WhatsApp wrote itself.
        writer: Option<String>,
        body: String,
    }
    let mut entries: Vec<Entry> = Vec::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        match stamp(line) {
            Some(s) => {
                let (writer, body) = match writer(s.rest) {
                    Some((name, body)) => (Some(name.to_string()), body.to_string()),
                    None => (None, String::new()),
                };
                entries.push(Entry { when: s.when, writer, body });
            }
            None => {
                if let Some(last) = entries.last_mut() {
                    last.body.push('\n');
                    last.body.push_str(line);
                }
            }
        }
    }
    let dates: Vec<When> = entries.iter().map(|e| e.when).collect();
    let reading = date_reading(&dates);
    let order = match reading {
        DateReading::Read(o) | DateReading::Guessed(o) => Some(o),
        DateReading::Unreadable => None,
    };
    let mut chat = Chat { title, messages: Vec::new(), dates: reading, attachments: 0, deleted: 0, notices: 0 };
    let mut seen: HashMap<String, usize> = HashMap::new();
    for e in entries {
        let Some(name) = e.writer else {
            chat.notices += 1;
            continue;
        };
        // On an iPhone, what WhatsApp writes itself into a chat — that it is
        // encrypted, who joined a group — comes after a left-to-right mark,
        // under the group's name or someone's.
        let marked = e.body.starts_with('\u{200e}');
        let body = match said(&e.body) {
            Said::Words(_) if marked => {
                chat.notices += 1;
                continue;
            }
            Said::Words(words) => words,
            Said::Attachment => {
                chat.attachments += 1;
                continue;
            }
            Said::Deleted => {
                chat.deleted += 1;
                continue;
            }
        };
        let (a, b, c) = e.when.numbers;
        let written = format!("{a}.{b}.{c} {}", e.when.time.format("%H:%M:%S"));
        let digest =
            hex::encode(Sha256::digest(format!("{}\u{1f}{written}\u{1f}{name}\u{1f}{body}", chat.title).as_bytes()));
        let n = seen.entry(digest.clone()).or_default();
        *n += 1;
        let key = format!("{}#{n}", &digest[..32]);
        let at = order.and_then(|o| e.when.read(o));
        chat.messages.push(ChatMessage { writer: name, body, at, key });
    }
    chat
}

/// When a message was sent, from the phone's time read in this computer's
/// time zone.
fn sent_at(at: NaiveDateTime) -> Option<String> {
    // A time in the hour the clocks skip is read as the hour after it.
    let local = Local
        .from_local_datetime(&at)
        .earliest()
        .or_else(|| Local.from_local_datetime(&(at + chrono::Duration::hours(1))).earliest())?;
    Some(local.with_timezone(&Utc).to_rfc3339_opts(SecondsFormat::Secs, true))
}

/// Every line of `bytes` that starts a message.
fn stamped_lines(bytes: &[u8]) -> usize {
    String::from_utf8_lossy(bytes).split('\n').filter(|l| stamp(l.strip_suffix('\r').unwrap_or(l)).is_some()).count()
}

/// Up to `MAX_TEXT_BYTES` of `reader`, or an error past that.
fn read_capped(reader: impl Read) -> SourceResult<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(MAX_TEXT_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_TEXT_BYTES {
        return Err(SourceError::Malformed("This is too large to be one chat's words.".into()));
    }
    Ok(bytes)
}

/// The chat in a zip: `_chat.txt` from an iPhone; otherwise a text file
/// named for WhatsApp; otherwise the text file most of whose lines start
/// messages — never one in a folder, and never a document sent in the chat
/// just because its name sorts first.
fn chat_in_zip(path: &Path) -> SourceResult<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| SourceError::Malformed(format!("This isn't a zip WhatsApp could have made: {e}")))?;
    let mut texts: Vec<String> = archive
        .file_names()
        .filter(|n| !n.contains('/') && n.to_ascii_lowercase().ends_with(".txt"))
        .map(str::to_string)
        .collect();
    texts.sort();
    let named = texts
        .iter()
        .find(|n| n.ends_with("_chat.txt"))
        .or_else(|| texts.iter().find(|n| n.to_ascii_lowercase().contains("whatsapp")))
        .cloned();
    let mut read = |name: &str| -> SourceResult<Vec<u8>> {
        let entry = archive.by_name(name).map_err(|e| SourceError::Malformed(e.to_string()))?;
        read_capped(entry)
    };
    if let Some(name) = named {
        return read(&name);
    }
    let mut best: Option<(usize, Vec<u8>)> = None;
    for name in &texts {
        let bytes = read(name)?;
        let lines = stamped_lines(&bytes);
        if lines > 0 && best.as_ref().is_none_or(|(most, _)| lines > *most) {
            best = Some((lines, bytes));
        }
    }
    best.map(|(_, bytes)| bytes)
        .ok_or_else(|| SourceError::Malformed("There's no chat in this zip: WhatsApp puts it in a .txt file.".into()))
}

/// The text of one export, and who the chat is with.
fn load(path: &Path) -> SourceResult<Chat> {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("WhatsApp chat");
    let is_zip = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    let bytes = if is_zip { chat_in_zip(path)? } else { read_capped(std::fs::File::open(path)?)? };
    Ok(read_chat(&String::from_utf8_lossy(&bytes), chat_title(stem)))
}

fn is_export(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("txt") || e.eq_ignore_ascii_case("zip"))
}

impl CommunicationSource for WhatsAppSource {
    fn metadata(&self) -> SourceMetadata {
        SourceMetadata {
            connector: "whatsapp",
            display_name: "WhatsApp chat",
            channel: "chat",
            description: "A chat exported from WhatsApp with Export chat: the .txt file, or the .zip from an iPhone or with media. One chat at a time; photos and voice messages are left out.",
            location_kind: LocationKind::File,
            extensions: &["txt", "zip"],
        }
    }

    fn discover(&self, location: &Path) -> SourceResult<Vec<PathBuf>> {
        if location.is_file() {
            return Ok(vec![location.to_path_buf()]);
        }
        if !location.is_dir() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(location)? {
            let path = entry?.path();
            if is_export(&path) {
                out.push(path);
            }
        }
        out.sort();
        Ok(out)
    }

    fn validate(&self, location: &Path) -> SourceResult<ValidationReport> {
        let mut report = validate_by_dry_run(self, location)?;
        let mut names: HashMap<String, (AuthorRef, usize)> = HashMap::new();
        let mut titles: Vec<String> = Vec::new();
        let (mut attachments, mut deleted, mut guessed, mut unreadable) = (0, 0, false, false);
        for path in self.discover(location)? {
            let chat = load(&path)?;
            for m in &chat.messages {
                let writer = author(&m.writer);
                names.entry(writer.display_name.clone()).or_insert((writer, 0)).1 += 1;
            }
            titles.push(chat.title.clone());
            attachments += chat.attachments;
            deleted += chat.deleted;
            guessed |= matches!(chat.dates, DateReading::Guessed(_));
            unreadable |= chat.dates == DateReading::Unreadable && !chat.messages.is_empty();
        }
        let mut names: Vec<WriterName> = names
            .into_iter()
            .map(|(name, (writer, messages))| {
                let id = &writer.identifiers[0];
                WriterName {
                    chat_named_after: titles.contains(&name),
                    name,
                    kind: id.kind.as_str().to_string(),
                    value: id.value.clone(),
                    normalized: id.normalized(),
                    messages,
                    also: Vec::new(),
                }
            })
            .collect();
        names.sort_by(|a, b| b.messages.cmp(&a.messages).then(a.name.cmp(&b.name)));
        report.names = names;
        if guessed {
            report.warnings.push(
                "Every date in this chat could be day first or month first, so I've read them day first — 3/4 as the 3rd of April."
                    .into(),
            );
        }
        if unreadable {
            report.warnings.push(
                "I couldn't read the dates in this chat, so its messages will have no times: how quickly you reply can't be learned from them, and none will look old.".into(),
            );
        }
        if attachments + deleted > 0 {
            let n = attachments + deleted;
            report.warnings.push(format!(
                "{} — photos, voice messages, calls{} — {} no words of yours, so {} left out.",
                if n == 1 { "1 message".to_string() } else { format!("{n} messages") },
                if deleted > 0 { ", deleted messages" } else { "" },
                if n == 1 { "has" } else { "have" },
                if n == 1 { "it's" } else { "they're" },
            ));
        }
        Ok(report)
    }

    fn import(
        &self,
        location: &Path,
        sink: &mut dyn FnMut(DiscoveredConversation) -> SourceResult<()>,
    ) -> SourceResult<()> {
        for path in self.discover(location)? {
            let chat = load(&path)?;
            if chat.messages.is_empty() {
                continue;
            }
            let messages = chat
                .messages
                .iter()
                .map(|m| RawMessage {
                    external_id: format!("{HANDLE_PREFIX}{}", m.key),
                    author: author(&m.writer),
                    sent_at: m.at.and_then(sent_at),
                    body: m.body.clone(),
                    metadata: Value::Null,
                })
                .collect();
            sink(DiscoveredConversation {
                external_id: format!("{HANDLE_PREFIX}{}", chat.title),
                subject: Some(chat.title.clone()),
                channel: "chat".into(),
                messages,
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;
    use std::io::Write;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> Option<NaiveDateTime> {
        Some(NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 0).unwrap())
    }

    const ANDROID: &str = "\u{feff}12/31/20, 9:41\u{202f}PM - Messages and calls are end-to-end encrypted. No one outside of this chat, not even WhatsApp, can read or listen to them.\r
12/31/20, 9:41\u{202f}PM - Ada Lovelace: Drinks on Friday?\r
12/31/20, 9:43\u{202f}PM - C: yes!\r
the usual place\r
1/2/21, 8:05\u{202f}AM - Ada Lovelace: <Media omitted>\r
1/2/21, 8:06\u{202f}AM - Ada Lovelace: This message was deleted\r
1/2/21, 8:07\u{202f}AM - C: sorry, running late <This message was edited>\r
1/2/21, 8:08\u{202f}AM - Ada Lovelace changed the subject to \"Friday: drinks\"\r
1/13/21, 12:15\u{202f}PM - +44 7700 900123: who is this?\r
";

    #[test]
    fn an_android_chat_is_read_message_by_message_with_its_writer_and_time() {
        let chat = read_chat(ANDROID, chat_title("WhatsApp Chat with Ada Lovelace"));
        assert_eq!(chat.title, "Ada Lovelace");
        assert_eq!(chat.dates, DateReading::Read(DateOrder::MonthFirst), "the 13th settles it");
        let read: Vec<(&str, &str, Option<NaiveDateTime>)> =
            chat.messages.iter().map(|m| (m.writer.as_str(), m.body.as_str(), m.at)).collect();
        assert_eq!(
            read,
            [
                ("Ada Lovelace", "Drinks on Friday?", at(2020, 12, 31, 21, 41)),
                ("C", "yes!\nthe usual place", at(2020, 12, 31, 21, 43)),
                ("C", "sorry, running late", at(2021, 1, 2, 8, 7)),
                ("+44 7700 900123", "who is this?", at(2021, 1, 13, 12, 15)),
            ]
        );
        assert_eq!((chat.attachments, chat.deleted, chat.notices), (1, 1, 2));
    }

    #[test]
    fn an_iphone_chat_comes_in_a_zip_and_is_read_the_same_way() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("WhatsApp Chat - Book club (1).zip");
        let text = "[31/12/2020, 21:41:05] Book club: \u{200e}Messages and calls are end-to-end encrypted.\n\
                    \u{200e}[31/12/2020, 21:41:06] Ada: \u{200e}image omitted\n\
                    [31/12/2020, 21:42:00] Ada: Chapter 3 by Friday?\n\
                    [01/01/2021, 09:00:00] C: I'll try\n\
                    [01/01/2021, 09:01:00] Bob: \u{200e}<attached: 00000012-PHOTO-2021-01-01.jpg>\n";
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("00000012-PHOTO-2021-01-01.jpg", options).unwrap();
        zip.write_all(&[0xff, 0xd8, 0xff]).unwrap();
        zip.start_file("_chat.txt", options).unwrap();
        zip.write_all(text.as_bytes()).unwrap();
        zip.finish().unwrap();

        let chat = load(&path).unwrap();
        assert_eq!(chat.title, "Book club");
        // The encryption notice comes under the group's name, but it is
        // WhatsApp's own line, not the group writing.
        let read: Vec<(&str, &str)> = chat.messages.iter().map(|m| (m.writer.as_str(), m.body.as_str())).collect();
        assert_eq!(read, [("Ada", "Chapter 3 by Friday?"), ("C", "I'll try")]);
        assert_eq!(chat.messages[1].at, at(2021, 1, 1, 9, 0));
        assert_eq!((chat.attachments, chat.notices), (2, 1));
        assert!(matches!(load(&dir.path().join("missing.zip")), Err(SourceError::Io(_))));
    }

    #[test]
    fn whether_a_date_is_day_or_month_first_is_read_from_the_dates_themselves() {
        let order = |lines: &[&str]| read_chat(&lines.join("\n"), "t".into()).dates;
        // A day past twelve settles it, either way round.
        assert_eq!(order(&["13/01/21, 21:41 - A: x"]), DateReading::Read(DateOrder::DayFirst));
        assert_eq!(order(&["01/13/21, 21:41 - A: x"]), DateReading::Read(DateOrder::MonthFirst));
        // Otherwise the reading that keeps the messages in order.
        // 12 January then 1 February, not 1 December then 2 January.
        assert_eq!(
            order(&["12/01/21, 10:00 - A: x", "01/02/21, 10:00 - A: x"]),
            DateReading::Read(DateOrder::DayFirst)
        );
        assert_eq!(
            order(&["01/12/21, 10:00 - A: x", "02/01/21, 10:00 - A: x"]),
            DateReading::Read(DateOrder::MonthFirst)
        );
        // Both in order: a guess.
        assert_eq!(
            order(&["01/02/21, 10:00 - A: x", "02/02/21, 10:00 - A: x", "03/02/21, 10:00 - A: x"]),
            DateReading::Guessed(DateOrder::DayFirst)
        );
        // Neither: day first, and said to be a guess.
        assert_eq!(order(&["05/06/21, 10:00 - A: x"]), DateReading::Guessed(DateOrder::DayFirst));
        // The year first, and German dots with a 24-hour clock.
        assert_eq!(order(&["2021-01-13, 21:41 - A: x"]), DateReading::Read(DateOrder::YearFirst));
        let german = read_chat("13.01.21, 21:41 - Ada: Hallo", "t".into());
        assert_eq!(german.messages[0].at, at(2021, 1, 13, 21, 41));
        // No order makes 13/13 a date.
        assert_eq!(order(&["13/13/21, 10:00 - A: x"]), DateReading::Unreadable);
        assert_eq!(read_chat("13/13/21, 10:00 - A: x", "t".into()).messages[0].at, None);
    }

    #[test]
    fn times_are_read_in_every_way_whatsapp_writes_them() {
        for (line, hour, minute) in [
            ("1/2/21, 9:41 PM - A: x", 21, 41),
            ("1/2/21, 12:05 AM - A: x", 0, 5),
            ("1/2/21, 12:05 pm - A: x", 12, 5),
            ("1/2/21, 9:41\u{a0}p.\u{a0}m. - A: x", 21, 41),
            ("1/2/21, 9:41 p. m. - A: x", 21, 41),
            ("[1/2/21, 9:41:07 AM] A: x", 9, 41),
            ("1/2/21, 21:41 \u{2013} A: x", 21, 41),
        ] {
            let s = stamp(line).unwrap_or_else(|| panic!("{line:?} is a message"));
            assert_eq!((s.when.time.hour(), s.when.time.minute()), (hour, minute), "{line:?}");
        }
        for line in ["the usual place", "12:30 works", "1/2/21 is fine", "13:61 - A: x", "1/2/21, 13:05 PM - A: x"] {
            assert_eq!(stamp(line), None, "{line:?} carries on the message before");
        }
    }

    #[test]
    fn the_chat_is_named_from_the_file_in_the_usual_languages() {
        for (stem, title) in [
            ("WhatsApp Chat with Ada", "Ada"),
            ("WhatsApp Chat - Book club", "Book club"),
            ("WhatsApp Chat with Ada (2)", "Ada"),
            ("WhatsApp-Chat mit Ada", "Ada"),
            ("Chat de WhatsApp con Ada", "Ada"),
            ("Discussion WhatsApp avec Ada", "Ada"),
            ("Conversa do WhatsApp com Ada", "Ada"),
            ("_chat", "_chat"),
            ("Ada and me (Paris)", "Ada and me (Paris)"),
        ] {
            assert_eq!(chat_title(stem), title, "{stem:?}");
        }
    }

    #[test]
    fn the_same_chat_read_again_is_known_message_by_message() {
        let text = "1/2/21, 10:00 - A: ok\n1/2/21, 10:00 - A: ok\n1/2/21, 10:01 - B: fine";
        let first = read_chat(text, "t".into());
        let again = read_chat(&format!("{text}\n1/2/21, 10:05 - B: new"), "t".into());
        let keys = |c: &Chat| c.messages.iter().map(|m| m.key.clone()).collect::<Vec<_>>();
        assert_eq!(keys(&first)[..], keys(&again)[..3], "what was there is known again");
        assert_ne!(keys(&first)[0], keys(&first)[1], "the same words twice in a minute are two messages");
    }

    #[test]
    fn a_writer_shown_as_a_number_is_a_phone_and_anyone_else_a_whatsapp_name() {
        let ada = author("Ada Lovelace");
        assert_eq!(ada.identifiers, [IdentifierInput::new(IdentifierKind::Handle, "whatsapp:Ada Lovelace")]);
        assert_eq!(ada.identifiers[0].normalized(), "whatsapp:ada lovelace");
        let unsaved = author("\u{202a}+44 7700 900123\u{202c}");
        assert_eq!(unsaved.identifiers, [IdentifierInput::new(IdentifierKind::Phone, "+44 7700 900123")]);
        assert_eq!(unsaved.display_name, "+44 7700 900123");
        assert_eq!(author("Agent 007").identifiers[0].kind, IdentifierKind::Handle);
    }

    #[test]
    fn what_has_no_words_is_counted_and_left_out() {
        let text = "1/2/21, 10:00 - A: IMG-20210102-WA0001.jpg (file attached)\nlook at this\n\
                    1/2/21, 10:01 - A: VID-20210102-WA0002.mp4 (file attached)\n\
                    1/2/21, 10:02 - A: location: https://maps.google.com/?q=1,2\n\
                    1/2/21, 10:03 - A: POLL:\nWhere?\nOPTION: Pub (2 votes)\n\
                    1/2/21, 10:04 - A: Missed voice call\n\
                    1/2/21, 10:05 - A: You deleted this message\n\
                    1/2/21, 10:06 - A:\nwords on the next line";
        let chat = read_chat(text, "t".into());
        let bodies: Vec<&str> = chat.messages.iter().map(|m| m.body.as_str()).collect();
        assert_eq!(bodies, ["look at this", "words on the next line"], "a caption is words");
        assert_eq!((chat.attachments, chat.deleted), (4, 1));
    }

    #[test]
    fn the_check_names_who_wrote_and_what_was_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("WhatsApp Chat with Ada.txt");
        std::fs::write(
            &path,
            "05/06/21, 10:00 - Ada: hi\n05/06/21, 10:01 - C: hello\n05/06/21, 10:02 - C: <Media omitted>\n05/06/21, 10:03 - C: how are you\n05/06/21, 10:04 - \u{202a}+44 7700 900123\u{202c}: who's this",
        )
        .unwrap();
        let report = WhatsAppSource.validate(&path).unwrap();
        assert!(report.ok);
        assert_eq!(report.messages, 4);
        let names: Vec<(&str, &str, &str, usize, bool)> = report
            .names
            .iter()
            .map(|w| (w.name.as_str(), w.kind.as_str(), w.normalized.as_str(), w.messages, w.chat_named_after))
            .collect();
        assert_eq!(
            names,
            [
                ("C", "handle", "whatsapp:c", 2, false),
                ("+44 7700 900123", "phone", "447700900123", 1, false),
                ("Ada", "handle", "whatsapp:ada", 1, true),
            ],
            "each as the import will know them; Ada is who the chat is named after"
        );
        assert!(report.warnings.iter().any(|w| w.contains("read them day first")), "{:?}", report.warnings);
        assert!(report.warnings.iter().any(|w| w.starts_with("1 message — photos")), "{:?}", report.warnings);
        let empty = dir.path().join("notes.txt");
        std::fs::write(&empty, "just some notes\nnot a chat").unwrap();
        assert!(!WhatsAppSource.validate(&empty).unwrap().ok, "a file with no messages is refused");
    }

    #[test]
    fn whatsapps_own_words_are_known_in_the_usual_languages() {
        let text = "13.01.21, 21:41 - Ada: <Medien ausgeschlossen>\n\
                    13.01.21, 21:42 - Ada: Diese Nachricht wurde gelöscht\n\
                    13.01.21, 21:43 - C: bin gleich da <Diese Nachricht wurde bearbeitet>\n\
                    13.01.21, 21:44 - Ada hat die Gruppe \u{201e}Trip: Paris\u{201c} erstellt\n\
                    13.01.21, 21:45 - Ada a nommé le groupe \u{ab} Plans : vendredi \u{bb}\n\
                    13.01.21, 21:46 - O\u{2019}Brien: see you there";
        let chat = read_chat(text, "t".into());
        let read: Vec<(&str, &str)> = chat.messages.iter().map(|m| (m.writer.as_str(), m.body.as_str())).collect();
        assert_eq!(read, [("C", "bin gleich da"), ("O\u{2019}Brien", "see you there")]);
        assert_eq!((chat.attachments, chat.deleted, chat.notices), (1, 1, 2));
    }

    #[test]
    fn the_chat_in_a_zip_is_the_chat_and_not_a_document_sent_in_it() {
        let dir = tempfile::tempdir().unwrap();
        let chat = "1/2/21, 10:00 - Ada: the agenda is attached\n1/2/21, 10:01 - C: thanks";
        let zip_of = |name: &str, files: &[(&str, &str)]| -> PathBuf {
            let path = dir.path().join(name);
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            for (file, text) in files {
                zip.start_file(*file, options).unwrap();
                zip.write_all(text.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
            path
        };
        // Android, with media: the chat is named for WhatsApp, and a document
        // whose name sorts first is not it.
        let android = zip_of(
            "WhatsApp Chat with Ada.zip",
            &[("Agenda.txt", "Plan: 1/2/21, 10:00 - start"), ("WhatsApp Chat with Ada.txt", chat)],
        );
        assert_eq!(load(&android).unwrap().messages.len(), 2);
        // Named for nothing: the text most of whose lines start messages, and
        // never one in a folder.
        let plain = zip_of(
            "export.zip",
            &[("Agenda.txt", "Plan: meet at ten"), ("chat.txt", chat), ("media/1/2/21, 10:00 - x.txt", chat)],
        );
        assert_eq!(load(&plain).unwrap().messages.len(), 2);
        let none = zip_of("photos.zip", &[("Agenda.txt", "Plan: meet at ten")]);
        assert!(matches!(load(&none), Err(SourceError::Malformed(_))));
    }
}
