//! The `meta` connector: Facebook Messenger and Instagram messages, from
//! Meta's "Download your information" in JSON (0.10.0-alpha.61).
//!
//! Both put each conversation in a folder of its own under `messages/` —
//! `inbox/`, `archived_threads/`, `e2ee_cutover/` (and, left out,
//! `message_requests/` and `filtered_threads/`: strangers and spam) — as
//! `message_1.json`, `message_2.json` and so on, newest first, each with the
//! conversation's `participants`, `title` and `messages`. Newer downloads put
//! `messages/` under `your_activity_across_facebook/` or
//! `your_instagram_activity/`; the download is read zipped (one part at a
//! time: a big one comes in several) or unzipped.
//!
//! What this reads, and what it leaves:
//!
//! * **Both sides**, so Mimic learns how the user replies; a group's every
//!   member writes in it.
//! * **People by name**, as WhatsApp's: Meta's download names who wrote each
//!   message and nothing else, so a writer is `meta:<name>`, and the check
//!   lists them, most first, for the user to say which is theirs.
//! * **Words people wrote.** Calls, people joining or leaving, messages that
//!   are only a photo, a video, a sticker, a file or a voice message — and
//!   the line Meta writes for one ("Ada sent an attachment.") — Instagram's
//!   likes and reactions written as messages, and unsent messages are
//!   counted and left out.
//! * **Meta's text as written.** The download writes each character's UTF-8
//!   bytes as characters of their own ("â€™" for "’"); they are put back
//!   together, and text that isn't so written is left as it is.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, TimeZone, Utc};
use serde_json::Value;

use super::{
    validate_by_dry_run, AuthorRef, CommunicationSource, DiscoveredConversation, LocationKind, RawMessage, SourceError,
    SourceMetadata, SourceResult, ValidationReport, WriterName,
};
use crate::db::repo_people::IdentifierInput;
use crate::db::IdentifierKind;

pub struct MetaSource;

/// What a writer's name is prefixed with to make their address.
pub const PREFIX: &str = "meta:";

/// The most read from any one file of a download: its messages, not its media.
const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// The folders under `messages/` read, and those left out.
const READ_FOLDERS: [&str; 3] = ["inbox", "archived_threads", "e2ee_cutover"];
const LEFT_FOLDERS: [&str; 2] = ["message_requests", "filtered_threads"];

/// What a message that is only media carries, any of which with no words of
/// the writer's own makes it a message without words.
const MEDIA: [&str; 7] = ["photos", "videos", "gifs", "audio_files", "files", "sticker", "share"];

/// Meta's line for a message that was only media, in English: "Ada sent an
/// attachment." Left out when the message carries the media.
const MEDIA_LINES: [&str; 8] = [
    " sent an attachment.",
    " sent a photo.",
    " sent a video.",
    " sent a GIF.",
    " sent a sticker.",
    " sent a voice message.",
    " sent a link.",
    " shared a story.",
];

/// The address the import gives a writer.
pub fn handle_of(name: &str) -> String {
    format!("{PREFIX}{name}")
}

/// Text as Meta's download writes it, put back together: each UTF-8 byte
/// written as a character of its own. Text with a character past one byte,
/// or whose bytes aren't UTF-8, was written as it reads, and is kept.
pub fn unmangled(text: &str) -> String {
    if text.is_ascii() || text.chars().any(|c| c as u32 > 0xFF) {
        return text.to_string();
    }
    let bytes: Vec<u8> = text.chars().map(|c| c as u32 as u8).collect();
    String::from_utf8(bytes).unwrap_or_else(|_| text.to_string())
}

/// One message, as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadMessage {
    pub from: String,
    pub at_ms: i64,
    pub body: String,
}

/// One conversation, from all of its files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    /// Its folder's name: `adalovelace_10158642`.
    pub id: String,
    pub title: String,
    pub participants: Vec<String>,
    pub messages: Vec<ThreadMessage>,
}

/// What was read, and what was left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Download {
    pub threads: Vec<Thread>,
    /// Conversations from strangers and spam.
    pub threads_left_out: usize,
    /// Calls, joins, media without words, unsent messages.
    pub without_words: usize,
}

/// The files of a download, from its zip or its folder.
enum Files {
    Zip(zip::ZipArchive<std::fs::File>),
    Folder(PathBuf),
}

impl Files {
    fn open(location: &Path) -> SourceResult<(Files, Vec<String>)> {
        if location.is_dir() {
            let mut names = Vec::new();
            // Deep enough for `your_activity_across_facebook/messages/inbox/
            // <thread>/message_1.json` inside a folder of its own.
            let mut stack = vec![(location.to_path_buf(), 0)];
            while let Some((dir, depth)) = stack.pop() {
                for entry in std::fs::read_dir(&dir)? {
                    let path = entry?.path();
                    if path.is_dir() {
                        if depth < 6 {
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
            .map_err(|e| SourceError::Malformed(format!("This isn't a zip Meta could have made: {e}")))?;
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

/// `(folder, thread)` of a conversation's file — `messages/inbox/ada_1/
/// message_1.json` gives ("inbox", "ada_1") — wherever `messages/` is.
fn thread_file(name: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = name.split('/').collect();
    let n = parts.len();
    if n < 4 {
        return None;
    }
    let (messages, folder, thread, file) = (parts[n - 4], parts[n - 3], parts[n - 2], parts[n - 1]);
    let numbered = file
        .strip_prefix("message_")
        .and_then(|f| f.strip_suffix(".json"))
        .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()));
    (messages.eq_ignore_ascii_case("messages") && numbered && !thread.is_empty())
        .then(|| (folder.to_ascii_lowercase(), thread.to_string()))
}

fn text(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(unmangled).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// One message's words, or `None` for one without: a call, a join, media
/// alone, Meta's line for it, an unsent message.
fn words_of(m: &Value, from: &str) -> Option<String> {
    let kind = m.get("type").and_then(Value::as_str).unwrap_or("Generic");
    // A call is a call whether or not the download says its type.
    if matches!(kind, "Call" | "Subscribe" | "Unsubscribe") || m.get("call_duration").is_some() {
        return None;
    }
    if m.get("is_unsent").or_else(|| m.get("isUnsent")).and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let body = text(m.get("content")).or_else(|| text(m.get("text")))?;
    // Instagram writes a like or a reaction as a message of its own.
    let lower = body.to_lowercase();
    if lower == "liked a message" || (lower.starts_with("reacted ") && lower.ends_with(" to your message")) {
        return None;
    }
    let has_media = MEDIA.iter().any(|k| m.get(*k).is_some_and(|v| !v.is_null()));
    if has_media && MEDIA_LINES.iter().any(|line| body == format!("{from}{line}") || body == format!("You{line}")) {
        return None;
    }
    Some(body)
}

/// Read a download's conversations, each from all of its files, oldest
/// message first.
pub fn read_download(location: &Path) -> SourceResult<Download> {
    let (mut files, names) = Files::open(location)?;
    let mut download = Download::default();
    // Every file of a thread, by folder and thread.
    let mut by_thread: Vec<((String, String), Vec<String>)> = Vec::new();
    for name in names {
        let Some(key) = thread_file(&name) else { continue };
        match by_thread.iter_mut().find(|(k, _)| *k == key) {
            Some((_, listed)) => listed.push(name),
            None => by_thread.push((key, vec![name])),
        }
    }
    for ((folder, thread), file_names) in by_thread {
        if LEFT_FOLDERS.contains(&folder.as_str()) {
            download.threads_left_out += 1;
            continue;
        }
        if !READ_FOLDERS.contains(&folder.as_str()) {
            continue;
        }
        let mut title = None;
        let mut participants: Vec<String> = Vec::new();
        let mut messages = Vec::new();
        for name in file_names {
            let value: Value = serde_json::from_slice(&files.read(&name)?)
                .map_err(|e| SourceError::Malformed(format!("{name} could not be read: {e}")))?;
            if title.is_none() {
                title = text(value.get("title")).or_else(|| text(value.get("threadName")));
            }
            for p in value.get("participants").and_then(Value::as_array).into_iter().flatten() {
                let name = text(p.get("name")).or_else(|| text(Some(p)));
                if let Some(name) = name.filter(|n| !participants.contains(n)) {
                    participants.push(name);
                }
            }
            for m in value.get("messages").and_then(Value::as_array).into_iter().flatten() {
                let from = text(m.get("sender_name")).or_else(|| text(m.get("senderName")));
                let at_ms = m.get("timestamp_ms").or_else(|| m.get("timestamp")).and_then(Value::as_i64);
                let (Some(from), Some(at_ms)) = (from, at_ms) else {
                    download.without_words += 1;
                    continue;
                };
                match words_of(m, &from) {
                    Some(body) => messages.push(ThreadMessage { from, at_ms, body }),
                    None => download.without_words += 1,
                }
            }
        }
        messages.sort_by_key(|m| m.at_ms);
        download.threads.push(Thread {
            title: title.unwrap_or_else(|| participants.join(", ")),
            id: thread,
            participants,
            messages,
        });
    }
    Ok(download)
}

fn author(name: &str) -> AuthorRef {
    AuthorRef {
        display_name: name.to_string(),
        identifiers: vec![IdentifierInput::new(IdentifierKind::Handle, handle_of(name))],
    }
}

/// A message's id: its conversation, its moment, and a digest of who wrote
/// it and what, so the same message read from two parts of a download, or
/// imported again, is known.
fn message_id(thread: &str, m: &ThreadMessage) -> String {
    let digest = crate::ids::sha256_hex(format!("{}\n{}", m.from, m.body).as_bytes());
    format!("{PREFIX}{thread}:{}:{}", m.at_ms, &digest[..12])
}

impl CommunicationSource for MetaSource {
    fn metadata(&self) -> SourceMetadata {
        SourceMetadata {
            connector: "meta",
            display_name: "Messenger or Instagram messages",
            channel: "chat",
            description: "Meta's \"Download your information\" from Facebook or Instagram, in JSON: the zip (or one part of it) or the folder it unzips to. Both sides of each conversation, so I learn how you reply.",
            location_kind: LocationKind::FileOrFolder,
            extensions: &["zip"],
        }
    }

    fn discover(&self, location: &Path) -> SourceResult<Vec<PathBuf>> {
        Ok(if location.exists() { vec![location.to_path_buf()] } else { Vec::new() })
    }

    fn validate(&self, location: &Path) -> SourceResult<ValidationReport> {
        let mut report = validate_by_dry_run(self, location)?;
        let download = read_download(location)?;
        if download.threads.is_empty() {
            report.ok = false;
            report.blockers = vec![
                "I can't find any messages here. Choose the download Meta sent in JSON — not HTML — or the folder it unzipped to; a download in several parts has its messages in one or more of them.".into(),
            ];
            report.warnings.clear();
            return Ok(report);
        }
        let mut counts: HashMap<String, (usize, bool)> = HashMap::new();
        for t in &download.threads {
            for m in &t.messages {
                counts.entry(m.from.clone()).or_default().0 += 1;
            }
            // Between two, the conversation is named after the other.
            if t.participants.len() == 2 {
                if let Some(seen) = counts.get_mut(&t.title) {
                    seen.1 = true;
                }
            }
        }
        let mut names: Vec<(String, usize, bool)> = counts.into_iter().map(|(n, (c, after))| (n, c, after)).collect();
        names.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        report.names = names
            .into_iter()
            .map(|(name, messages, chat_named_after)| {
                let id = &author(&name).identifiers[0];
                WriterName {
                    kind: id.kind.as_str().to_string(),
                    value: id.value.clone(),
                    normalized: id.normalized(),
                    name,
                    messages,
                    chat_named_after,
                    also: Vec::new(),
                }
            })
            .collect();
        if download.threads_left_out > 0 {
            report.warnings.push(format!(
                "{} from message requests and filtered messages {} left out.",
                if download.threads_left_out == 1 {
                    "1 conversation".to_string()
                } else {
                    format!("{} conversations", download.threads_left_out)
                },
                if download.threads_left_out == 1 { "was" } else { "were" },
            ));
        }
        if download.without_words > 0 {
            report.warnings.push(format!(
                "{} left out: calls, people joining or leaving, photos, videos, stickers and files without words, and unsent messages.",
                if download.without_words == 1 { "1 message was".to_string() } else { format!("{} messages were", download.without_words) },
            ));
        }
        Ok(report)
    }

    fn import(
        &self,
        location: &Path,
        sink: &mut dyn FnMut(DiscoveredConversation) -> SourceResult<()>,
    ) -> SourceResult<()> {
        for thread in read_download(location)?.threads {
            if thread.messages.is_empty() {
                continue;
            }
            let messages = thread
                .messages
                .iter()
                .map(|m| RawMessage {
                    external_id: message_id(&thread.id, m),
                    author: author(&m.from),
                    sent_at: Utc
                        .timestamp_millis_opt(m.at_ms)
                        .single()
                        .map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true)),
                    body: m.body.clone(),
                    metadata: Value::Null,
                })
                .collect();
            sink(DiscoveredConversation {
                external_id: format!("{PREFIX}thread:{}", thread.id),
                subject: Some(thread.title),
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

    fn put(root: &Path, rel: &str, json: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, json).unwrap();
    }

    #[test]
    fn metas_text_is_put_back_together_and_other_text_left_as_it_is() {
        assert_eq!(unmangled("it\u{00e2}\u{0080}\u{0099}s"), "it’s");
        assert_eq!(unmangled("caf\u{00c3}\u{00a9}"), "café");
        assert_eq!(unmangled("café"), "café", "written as it reads: not UTF-8 bytes");
        assert_eq!(unmangled("plain"), "plain");
        assert_eq!(unmangled("😀 ok"), "😀 ok");
    }

    #[test]
    fn a_download_is_read_across_its_files_oldest_first_with_only_words_people_wrote() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("facebook-c-2026");
        put(
            &root,
            "your_activity_across_facebook/messages/inbox/adalovelace_1/message_1.json",
            r#"{ "participants": [{"name": "Ada Lovelace"}, {"name": "C Haynes"}], "title": "Ada Lovelace",
                 "messages": [
                   {"sender_name": "Ada Lovelace", "timestamp_ms": 1770110800000, "content": "Reacted \u00f0\u009f\u0098\u0082 to your message"},
                   {"sender_name": "C Haynes", "timestamp_ms": 1770110750000, "content": "C Haynes called you.", "call_duration": 4},
                   {"sender_name": "Ada Lovelace", "timestamp_ms": 1770110700000, "content": "see you there"},
                   {"sender_name": "C Haynes", "timestamp_ms": 1770110400000, "content": "yes! noon at the usual â\u0080\u0099round"},
                   {"sender_name": "Ada Lovelace", "timestamp_ms": 1770110300000, "content": "Ada Lovelace sent a photo.", "photos": [{"uri": "x.jpg"}]},
                   {"sender_name": "C Haynes", "timestamp_ms": 1770110200000, "type": "Call", "content": "C Haynes called you.", "call_duration": 10}
                 ] }"#,
        );
        put(
            &root,
            "your_activity_across_facebook/messages/inbox/adalovelace_1/message_2.json",
            r#"{ "participants": [{"name": "Ada Lovelace"}, {"name": "C Haynes"}], "title": "Ada Lovelace",
                 "messages": [ {"sender_name": "Ada Lovelace", "timestamp_ms": 1770110040000, "content": "lunch on thursday?"} ] }"#,
        );
        put(
            &root,
            "your_activity_across_facebook/messages/message_requests/stranger_9/message_1.json",
            r#"{ "participants": [{"name": "Stranger"}, {"name": "C Haynes"}], "title": "Stranger",
                 "messages": [ {"sender_name": "Stranger", "timestamp_ms": 1770110000000, "content": "hi"} ] }"#,
        );
        let download = read_download(&root).unwrap();
        assert_eq!(download.threads.len(), 1);
        assert_eq!(download.threads_left_out, 1);
        let t = &download.threads[0];
        assert_eq!((t.id.as_str(), t.title.as_str()), ("adalovelace_1", "Ada Lovelace"));
        let said: Vec<(&str, &str)> = t.messages.iter().map(|m| (m.from.as_str(), m.body.as_str())).collect();
        assert_eq!(
            said,
            [
                ("Ada Lovelace", "lunch on thursday?"),
                ("C Haynes", "yes! noon at the usual ’round"),
                ("Ada Lovelace", "see you there")
            ]
        );
        assert_eq!(download.without_words, 4, "the photo, two calls, the reaction");

        let report = MetaSource.validate(&root).unwrap();
        assert!(report.ok, "{report:?}");
        let names: Vec<(&str, &str, usize, bool)> =
            report.names.iter().map(|w| (w.name.as_str(), w.value.as_str(), w.messages, w.chat_named_after)).collect();
        assert_eq!(names, [("Ada Lovelace", "meta:Ada Lovelace", 2, true), ("C Haynes", "meta:C Haynes", 1, false)]);

        let mut seen = Vec::new();
        MetaSource
            .import(&root, &mut |c| {
                seen.push((c.external_id.clone(), c.messages[0].sent_at.clone(), c.messages.len()));
                Ok(())
            })
            .unwrap();
        assert_eq!(seen, [("meta:thread:adalovelace_1".to_string(), Some("2026-02-03T09:14:00Z".to_string()), 3)]);
    }

    #[test]
    fn the_same_message_read_twice_has_the_same_id() {
        let m = ThreadMessage { from: "Ada".into(), at_ms: 1, body: "hi".into() };
        assert_eq!(message_id("t", &m), message_id("t", &m.clone()));
        assert_ne!(message_id("t", &m), message_id("t", &ThreadMessage { body: "hey".into(), ..m.clone() }));
    }

    #[test]
    fn a_download_without_messages_says_where_they_are() {
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "profile_information/profile_information.json", "{}");
        let report = MetaSource.validate(dir.path()).unwrap();
        assert!(!report.ok);
        assert!(report.blockers[0].contains("JSON — not HTML"), "{:?}", report.blockers);
    }
}
