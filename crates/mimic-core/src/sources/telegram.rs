//! The `telegram` connector: chats from Telegram Desktop's export
//! (0.10.0-alpha.60).
//!
//! Telegram Desktop exports as JSON ("Machine-readable JSON") to a folder
//! holding `result.json`: either one chat (a chat's ⋮ → Export chat history),
//! with the chat's `name`, `type`, `id` and `messages` at the top, or the
//! whole account (Settings → Advanced → Export Telegram data), with
//! `personal_information` and `chats.list`, each a chat as above.
//!
//! What this reads, and what it leaves:
//!
//! * **Both sides.** A chat holds what everyone wrote, so — unlike a Discord
//!   package — it teaches Mimic how the user replies, and what someone wrote
//!   last can wait for a reply. Each writer is `telegram:<from_id>`
//!   (`user123`), as an account id.
//! * **Whose is whose** is the user's to say, as for WhatsApp: the check
//!   lists the writers (`names`). A whole-account export says whose account
//!   it is, so only that writer is listed, with the phone number on the
//!   account, which makes it the user at once when the number is theirs
//!   already.
//! * **Personal chats and private groups.** Saved Messages (notes to self),
//!   bots, channels and public groups are left out and counted: they are not
//!   conversations the user replies in, and a public group's hundreds of
//!   writers would each become a person.
//! * **Words the user wrote.** Service lines ("joined the group"), messages
//!   that are only a file, a photo or a sticker, and forwarded messages —
//!   someone else's words — are counted and left out.
//! * **Times.** `date_unixtime` is the moment, in newer exports; older ones
//!   have only `date`, on the exporting computer's clock, read as this
//!   computer's.

use std::io::Read;
use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use serde_json::Value;

use super::{
    validate_by_dry_run, AuthorRef, CommunicationSource, DiscoveredConversation, LocationKind, RawMessage, SourceError,
    SourceMetadata, SourceResult, ValidationReport, WriterAddress, WriterName,
};
use crate::db::repo_people::IdentifierInput;
use crate::db::IdentifierKind;

pub struct TelegramSource;

/// What a writer's `from_id` is prefixed with to make their address.
pub const PREFIX: &str = "telegram:";

/// The most read from an export: its messages, not its media.
const MAX_BYTES: u64 = 1024 * 1024 * 1024;

/// The kinds of chat read: between two people, and private groups.
const READ_KINDS: [&str; 3] = ["personal_chat", "private_group", "private_supergroup"];

/// One message, as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub id: String,
    /// `from_id` as the export writes it: `user123`.
    pub from_id: String,
    pub from: String,
    pub sent_at: Option<String>,
    pub body: String,
}

/// One chat, as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chat {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub messages: Vec<ChatMessage>,
}

/// What was read, and what was left out and why.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Export {
    /// The account a whole-account export is of: its `from_id`, name and
    /// phone number.
    pub owner: Option<(String, String, Option<String>)>,
    pub chats: Vec<Chat>,
    /// Chats left out: Saved Messages, bots, channels, public groups.
    pub chats_left_out: usize,
    /// Messages left out: service lines, files without words, forwards.
    pub without_words: usize,
    pub forwarded: usize,
}

/// A message's text: a string, or a list of strings and pieces with a
/// `text` (a link, bold, a mention).
fn text_of(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .map(|p| match p {
                Value::String(s) => s.as_str(),
                other => other.get("text").and_then(Value::as_str).unwrap_or(""),
            })
            .collect(),
        _ => String::new(),
    }
}

/// When a message was sent, as UTC RFC 3339: `date_unixtime` when the export
/// has it, else `date` on this computer's clock.
fn sent_at(message: &Value) -> Option<String> {
    let unix = message.get("date_unixtime").and_then(|v| match v {
        Value::String(s) => s.trim().parse::<i64>().ok(),
        Value::Number(n) => n.as_i64(),
        _ => None,
    });
    let at = match unix {
        Some(secs) => Utc.timestamp_opt(secs, 0).single()?,
        None => {
            let date = message.get("date")?.as_str()?;
            let naive = NaiveDateTime::parse_from_str(date.trim(), "%Y-%m-%dT%H:%M:%S").ok()?;
            Local.from_local_datetime(&naive).earliest()?.with_timezone(&Utc)
        }
    };
    Some(at.to_rfc3339_opts(SecondsFormat::Secs, true))
}

fn id_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// One chat from its JSON; `None` for a kind not read.
fn chat_of(value: &Value, export: &mut Export) -> Option<Chat> {
    let kind = value.get("type").and_then(Value::as_str).unwrap_or("").to_string();
    if !READ_KINDS.contains(&kind.as_str()) {
        export.chats_left_out += 1;
        return None;
    }
    let id = id_text(value.get("id"))?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map_or_else(|| format!("Telegram chat {id}"), str::to_string);
    let mut messages = Vec::new();
    for m in value.get("messages").and_then(Value::as_array).into_iter().flatten() {
        if m.get("type").and_then(Value::as_str) != Some("message") {
            export.without_words += 1;
            continue;
        }
        if m.get("forwarded_from").is_some_and(|f| !f.is_null()) {
            export.forwarded += 1;
            continue;
        }
        let body = text_of(m.get("text")).trim().to_string();
        let (Some(message_id), Some(from_id)) = (id_text(m.get("id")), id_text(m.get("from_id"))) else {
            export.without_words += 1;
            continue;
        };
        if body.is_empty() {
            export.without_words += 1;
            continue;
        }
        let from = m.get("from").and_then(Value::as_str).map(str::trim).filter(|f| !f.is_empty());
        messages.push(ChatMessage {
            id: message_id,
            from: from.unwrap_or("Someone on Telegram").to_string(),
            from_id,
            sent_at: sent_at(m),
            body,
        });
    }
    Some(Chat { id, name, kind, messages })
}

/// Read an export: a whole account's, or one chat's.
pub fn read_export(location: &Path) -> SourceResult<Export> {
    let mut bytes = Vec::new();
    std::fs::File::open(location)?.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(SourceError::Malformed("This file is too large to be a Telegram export's messages.".into()));
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
        SourceError::Malformed(
            "This isn't a Telegram export: choose the result.json Telegram Desktop wrote, in machine-readable JSON."
                .into(),
        )
    })?;
    let mut export = Export::default();
    if let Some(me) = value.get("personal_information") {
        if let Some(user) = id_text(me.get("user_id")) {
            let name = [me.get("first_name"), me.get("last_name")]
                .iter()
                .filter_map(|v| v.and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()))
                .collect::<Vec<_>>()
                .join(" ");
            let phone = me.get("phone_number").and_then(Value::as_str).map(str::trim).filter(|p| !p.is_empty());
            let name = if name.is_empty() { "Your Telegram account".to_string() } else { name };
            export.owner = Some((format!("user{user}"), name, phone.map(str::to_string)));
        }
    }
    let lists = [value.get("chats"), value.get("left_chats")];
    let listed: Vec<&Value> =
        lists.iter().filter_map(|l| l.and_then(|c| c.get("list")).and_then(Value::as_array)).flatten().collect();
    if listed.is_empty() && value.get("messages").is_some() {
        if let Some(chat) = chat_of(&value, &mut export) {
            export.chats.push(chat);
        }
    } else {
        for c in listed {
            if let Some(chat) = chat_of(c, &mut export) {
                export.chats.push(chat);
            }
        }
    }
    Ok(export)
}

/// A writer, as the import knows them: `telegram:<from_id>`, and for the
/// account's owner the phone number on it too.
fn author(from_id: &str, name: &str, owner: Option<&(String, String, Option<String>)>) -> AuthorRef {
    let mut identifiers = vec![IdentifierInput::new(IdentifierKind::AccountId, format!("{PREFIX}{from_id}"))];
    if let Some((_, _, Some(phone))) = owner.filter(|(id, _, _)| id == from_id) {
        identifiers.push(IdentifierInput::new(IdentifierKind::Phone, phone));
    }
    AuthorRef { display_name: name.to_string(), identifiers }
}

impl CommunicationSource for TelegramSource {
    fn metadata(&self) -> SourceMetadata {
        SourceMetadata {
            connector: "telegram",
            display_name: "Telegram export",
            channel: "chat",
            description: "The result.json Telegram Desktop writes when you export a chat, or all your data, as machine-readable JSON. Chats between two people and private groups; both sides, so I learn how you reply.",
            location_kind: LocationKind::File,
            extensions: &["json"],
        }
    }

    fn discover(&self, location: &Path) -> SourceResult<Vec<PathBuf>> {
        Ok(if location.is_file() { vec![location.to_path_buf()] } else { Vec::new() })
    }

    fn validate(&self, location: &Path) -> SourceResult<ValidationReport> {
        let mut report = validate_by_dry_run(self, location)?;
        let export = read_export(location)?;
        // Who wrote, most first; for a whole account's export, its owner only.
        let mut counts: Vec<(String, String, usize, bool)> = Vec::new();
        for chat in &export.chats {
            for m in &chat.messages {
                match counts.iter_mut().find(|(id, _, _, _)| *id == m.from_id) {
                    Some(seen) => seen.2 += 1,
                    None => counts.push((m.from_id.clone(), m.from.clone(), 1, false)),
                }
            }
            if chat.kind == "personal_chat" {
                for seen in counts.iter_mut().filter(|(_, name, _, _)| *name == chat.name) {
                    seen.3 = true;
                }
            }
        }
        counts.sort_by(|a, b| b.2.cmp(&a.2).then(a.1.cmp(&b.1)));
        if let Some((owner, _, _)) = &export.owner {
            counts.retain(|(id, _, _, _)| id == owner);
        }
        report.names = counts
            .into_iter()
            .map(|(from_id, name, messages, chat_named_after)| {
                let author = author(&from_id, &name, export.owner.as_ref());
                let id = &author.identifiers[0];
                WriterName {
                    name,
                    kind: id.kind.as_str().to_string(),
                    value: id.value.clone(),
                    normalized: id.normalized(),
                    messages,
                    chat_named_after,
                    also: author.identifiers[1..].iter().map(WriterAddress::of).collect(),
                }
            })
            .collect();
        if export.chats_left_out > 0 {
            report.warnings.push(format!(
                "{} left out: Saved Messages, bots, channels and public groups aren't conversations you reply in.",
                if export.chats_left_out == 1 {
                    "1 chat was".to_string()
                } else {
                    format!("{} chats were", export.chats_left_out)
                },
            ));
        }
        let left = export.without_words + export.forwarded;
        if left > 0 {
            report.warnings.push(format!(
                "{} left out: Telegram's own lines, files and stickers without words, and forwarded messages, which are someone else's words.",
                if left == 1 { "1 message was".to_string() } else { format!("{left} messages were") },
            ));
        }
        Ok(report)
    }

    fn import(
        &self,
        location: &Path,
        sink: &mut dyn FnMut(DiscoveredConversation) -> SourceResult<()>,
    ) -> SourceResult<()> {
        let export = read_export(location)?;
        // Whose account it is, when the export says: a chat's messages are
        // numbered per account (`import::account_of`, 0.10.0-alpha.77).
        let account =
            export.owner.as_ref().map(|(id, _, _)| serde_json::json!({ "telegramAccount": format!("telegram:{id}") }));
        for chat in export.chats {
            if chat.messages.is_empty() {
                continue;
            }
            let chat_id = chat.id.clone();
            let mut messages: Vec<RawMessage> = chat
                .messages
                .into_iter()
                .map(|m| RawMessage {
                    // A message's id is its place in its chat.
                    external_id: format!("{PREFIX}{chat_id}:{}", m.id),
                    author: author(&m.from_id, &m.from, export.owner.as_ref()),
                    sent_at: m.sent_at,
                    body: m.body,
                    metadata: account.clone().unwrap_or(Value::Null),
                })
                .collect();
            // In the order they were sent, ids breaking ties.
            messages.sort_by(|a, b| {
                (&a.sent_at, a.external_id.len(), &a.external_id).cmp(&(
                    &b.sent_at,
                    b.external_id.len(),
                    &b.external_id,
                ))
            });
            sink(DiscoveredConversation {
                external_id: format!("{PREFIX}chat:{chat_id}"),
                subject: Some(chat.name),
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

    fn write(dir: &tempfile::TempDir, json: &str) -> PathBuf {
        let path = dir.path().join("result.json");
        std::fs::write(&path, json).unwrap();
        path
    }

    const ONE_CHAT: &str = r#"{
      "name": "Ada Lovelace", "type": "personal_chat", "id": 4242,
      "messages": [
        { "id": 1, "type": "message", "date": "2026-02-03T09:14:00", "date_unixtime": "1770110040",
          "from": "Ada Lovelace", "from_id": "user4242", "text": "lunch on thursday?" },
        { "id": 2, "type": "message", "date": "2026-02-03T09:20:00", "date_unixtime": "1770110400",
          "from": "C", "from_id": "user7", "text": ["yes! ", {"type": "bold", "text": "noon"}, " at the usual"] },
        { "id": 3, "type": "service", "date": "2026-02-03T09:21:00", "actor": "Ada Lovelace", "action": "pin_message" },
        { "id": 4, "type": "message", "date": "2026-02-03T09:22:00", "from": "Ada Lovelace", "from_id": "user4242",
          "photo": "photos/photo_1.jpg", "text": "" },
        { "id": 5, "type": "message", "date": "2026-02-03T09:23:00", "from": "C", "from_id": "user7",
          "forwarded_from": "Someone", "text": "a joke someone else wrote" },
        { "id": 6, "type": "message", "date": "2026-02-03T09:25:00", "from": "Ada Lovelace", "from_id": "user4242",
          "text": "see you there" }
      ]
    }"#;

    #[test]
    fn one_chat_is_read_with_both_sides_and_only_words_people_wrote() {
        let dir = tempfile::tempdir().unwrap();
        let export = read_export(&write(&dir, ONE_CHAT)).unwrap();
        assert_eq!(export.owner, None);
        assert_eq!(export.chats.len(), 1);
        let chat = &export.chats[0];
        assert_eq!((chat.id.as_str(), chat.name.as_str()), ("4242", "Ada Lovelace"));
        let bodies: Vec<&str> = chat.messages.iter().map(|m| m.body.as_str()).collect();
        assert_eq!(bodies, ["lunch on thursday?", "yes! noon at the usual", "see you there"]);
        assert_eq!((export.without_words, export.forwarded), (2, 1), "the pin and the photo; the forward");
        assert_eq!(chat.messages[0].sent_at.as_deref(), Some("2026-02-03T09:14:00Z"), "date_unixtime is the moment");
        let local = Local
            .from_local_datetime(&NaiveDateTime::parse_from_str("2026-02-03T09:25:00", "%Y-%m-%dT%H:%M:%S").unwrap())
            .earliest()
            .unwrap()
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Secs, true);
        assert_eq!(chat.messages[2].sent_at.as_deref(), Some(local.as_str()), "date alone, on this computer's clock");
    }

    #[test]
    fn one_chat_asks_which_writer_is_the_user() {
        let dir = tempfile::tempdir().unwrap();
        let report = TelegramSource.validate(&write(&dir, ONE_CHAT)).unwrap();
        assert!(report.ok, "{report:?}");
        assert_eq!(report.messages, 3);
        let names: Vec<(&str, &str, usize, bool)> =
            report.names.iter().map(|w| (w.name.as_str(), w.value.as_str(), w.messages, w.chat_named_after)).collect();
        assert_eq!(
            names,
            [("Ada Lovelace", "telegram:user4242", 2, true), ("C", "telegram:user7", 1, false)],
            "most first; the chat is named after Ada"
        );
        assert_eq!(report.names[0].kind, "account_id");
        assert!(report.warnings.iter().any(|w| w.contains("3 messages were left out")), "{:?}", report.warnings);
    }

    #[test]
    fn a_whole_account_names_its_owner_and_leaves_out_what_isnt_a_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            &dir,
            r#"{
              "personal_information": { "user_id": 7, "first_name": "C", "last_name": "Haynes", "phone_number": "+1 808 555 0100" },
              "chats": { "list": [
                { "name": "Ada Lovelace", "type": "personal_chat", "id": 4242, "messages": [
                  { "id": 1, "type": "message", "date_unixtime": "1770110040", "from": "Ada Lovelace", "from_id": "user4242", "text": "lunch?" },
                  { "id": 2, "type": "message", "date_unixtime": "1770110400", "from": "C Haynes", "from_id": "user7", "text": "yes" } ] },
                { "name": "Saved Messages", "type": "saved_messages", "id": 7, "messages": [
                  { "id": 1, "type": "message", "date_unixtime": "1770110000", "from": "C Haynes", "from_id": "user7", "text": "note to self" } ] },
                { "name": "Weather", "type": "bot_chat", "id": 99, "messages": [] },
                { "name": "Family", "type": "private_group", "id": 55, "messages": [
                  { "id": 9, "type": "message", "date_unixtime": "1770111000", "from": "Charles", "from_id": "user8", "text": "dinner sunday" } ] }
              ] },
              "left_chats": { "list": [
                { "name": "Old crew", "type": "private_supergroup", "id": 66, "messages": [
                  { "id": 3, "type": "message", "date_unixtime": "1700000000", "from": "C Haynes", "from_id": "user7", "text": "bye all" } ] }
              ] }
            }"#,
        );
        let export = read_export(&path).unwrap();
        assert_eq!(export.owner, Some(("user7".into(), "C Haynes".into(), Some("+1 808 555 0100".into()))));
        let chats: Vec<&str> = export.chats.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(chats, ["Ada Lovelace", "Family", "Old crew"]);
        assert_eq!(export.chats_left_out, 2, "Saved Messages and the bot");

        let report = TelegramSource.validate(&path).unwrap();
        assert_eq!(report.names.len(), 1, "only the owner is asked about");
        assert_eq!(report.names[0].value, "telegram:user7");
        assert_eq!(report.names[0].also.len(), 1, "with the phone number on the account");
        assert_eq!(report.names[0].also[0].kind, "phone");

        let mut seen = Vec::new();
        TelegramSource
            .import(&path, &mut |c| {
                seen.push((
                    c.external_id.clone(),
                    c.messages.iter().map(|m| m.external_id.clone()).collect::<Vec<_>>(),
                ));
                Ok(())
            })
            .unwrap();
        assert_eq!(
            seen[0],
            ("telegram:chat:4242".to_string(), vec!["telegram:4242:1".to_string(), "telegram:4242:2".to_string()])
        );
        assert_eq!(seen.len(), 3);
    }

    #[test]
    fn a_file_that_isnt_an_export_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let err = read_export(&write(&dir, "not json")).unwrap_err();
        assert!(err.to_string().contains("result.json"), "{err}");
        let report = TelegramSource.validate(&write(&dir, r#"{"about": "nothing here"}"#)).unwrap();
        assert!(!report.ok);
    }
}
