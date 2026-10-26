//! The `sms` connector: text messages from an Android phone, from the XML
//! file "SMS Backup & Restore" writes (0.10.0-alpha.62).
//!
//! The backup is one `<smses>` element of `<sms … />` and `<mms>…</mms>`
//! elements, every value in an attribute: `address`, `date` (milliseconds),
//! `type` (1 received, 2 sent; drafts, outbox and failed are left out),
//! `body`, `contact_name`; an MMS keeps its words in `<part ct="text/plain"
//! text="…"/>` and who sent it in `<addr type="137">`, its other people in
//! `address` (`+15550100~+15550101`), and `msg_box` for received or sent.
//!
//! * **Both sides**, one conversation per number, or per group of numbers.
//! * **Whose messages are whose** the backup says by `type`, not by any
//!   number of the user's: every message the phone sent comes from
//!   `sms:this-phone`, which the check lists alone, for the user to say is
//!   theirs. Everyone else is their number, under the name the phone saved.
//! * **Words, not media.** An MMS that is only a picture is counted and left
//!   out; its pictures, often most of the file, are read past.
//! * **The file's own quirks.** "null" is no value; an emoji written as two
//!   numeric references (`&#55357;&#56832;`), one per half, is put together.

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

pub struct SmsSource;

/// Who sent what the phone sent: the user, once they say so.
pub const THIS_PHONE: &str = "sms:this-phone";

/// The most read from a backup. Pictures sent by MMS can make one far
/// larger; a backup made without them is what this reads.
const MAX_BYTES: u64 = 1024 * 1024 * 1024;

/// One element's name and attributes, entities decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Element {
    name: String,
    attrs: HashMap<String, String>,
    /// For an `mms`, its `part`s and `addr`s.
    children: Vec<Element>,
}

impl Element {
    fn get(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(String::as_str).map(str::trim).filter(|v| !v.is_empty() && *v != "null")
    }
}

/// A value's entities as the characters they stand for, and a character
/// written as two references, one for each half of its UTF-16, put together.
fn decoded(raw: &str) -> String {
    let mut units: Vec<u16> = Vec::new();
    let mut out = String::with_capacity(raw.len());
    let flush = |units: &mut Vec<u16>, out: &mut String| {
        if !units.is_empty() {
            out.push_str(&String::from_utf16_lossy(units));
            units.clear();
        }
    };
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        let before = &rest[..amp];
        if !before.is_empty() {
            flush(&mut units, &mut out);
            out.push_str(before);
        }
        let after = &rest[amp + 1..];
        let Some(semi) = after.find(';').filter(|&s| s <= 10) else {
            flush(&mut units, &mut out);
            out.push('&');
            rest = after;
            continue;
        };
        let entity = &after[..semi];
        let number = if let Some(hex) = entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")) {
            u32::from_str_radix(hex, 16).ok()
        } else {
            entity.strip_prefix('#').and_then(|d| d.parse::<u32>().ok())
        };
        match (number, entity) {
            (Some(n), _) if n <= 0xFFFF => units.push(n as u16),
            (Some(n), _) => {
                flush(&mut units, &mut out);
                out.push(char::from_u32(n).unwrap_or('\u{FFFD}'));
            }
            (None, named) => {
                flush(&mut units, &mut out);
                match named {
                    "amp" => out.push('&'),
                    "lt" => out.push('<'),
                    "gt" => out.push('>'),
                    "quot" => out.push('"'),
                    "apos" => out.push('\''),
                    _ => {
                        out.push('&');
                        out.push_str(named);
                        out.push(';');
                    }
                }
            }
        }
        rest = &after[semi + 1..];
    }
    flush(&mut units, &mut out);
    out.push_str(rest);
    out
}

/// The `sms` and `mms` elements of a backup, in order. Quoted values may
/// hold anything, `>` included; what isn't an element is read past.
fn elements(text: &str) -> Vec<Element> {
    let bytes = text.as_bytes();
    let mut out: Vec<Element> = Vec::new();
    // The `mms` being read, while inside one.
    let mut open: Option<Element> = None;
    let mut i = 0;
    while let Some(lt) = text[i..].find('<').map(|p| i + p) {
        i = lt + 1;
        if text[i..].starts_with('/') {
            let end = text[i..].find('>').map_or(text.len(), |p| i + p + 1);
            if text[i + 1..end.saturating_sub(1).max(i + 1)].trim() == "mms" {
                if let Some(mms) = open.take() {
                    out.push(mms);
                }
            }
            i = end;
            continue;
        }
        if text[i..].starts_with('?') || text[i..].starts_with('!') {
            i = text[i..].find('>').map_or(text.len(), |p| i + p + 1);
            continue;
        }
        let name_end =
            text[i..].find(|c: char| c.is_whitespace() || c == '>' || c == '/').map_or(text.len(), |p| i + p);
        let name = text[i..name_end].to_string();
        let mut attrs = HashMap::new();
        let mut j = name_end;
        let mut closed = false;
        loop {
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= bytes.len() {
                break;
            }
            match bytes[j] {
                b'>' => {
                    j += 1;
                    break;
                }
                b'/' => {
                    closed = true;
                    j += 1;
                    continue;
                }
                _ => {}
            }
            let Some(eq) = text[j..].find('=').map(|p| j + p) else { break };
            let key = text[j..eq].trim().to_string();
            let mut k = eq + 1;
            while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                k += 1;
            }
            let Some(&quote) = bytes.get(k).filter(|&&q| q == b'"' || q == b'\'') else { break };
            let Some(close) = text[k + 1..].find(quote as char).map(|p| k + 1 + p) else { break };
            // Pictures are read past, not decoded.
            if key != "data" {
                attrs.insert(key, decoded(&text[k + 1..close]));
            }
            j = close + 1;
        }
        i = j;
        let element = Element { name: name.clone(), attrs, children: Vec::new() };
        match name.as_str() {
            "sms" => out.push(element),
            "mms" if closed => out.push(element),
            "mms" => open = Some(element),
            "part" | "addr" => {
                if let Some(mms) = open.as_mut() {
                    mms.children.push(element);
                }
            }
            _ => {}
        }
    }
    out
}

/// One message, as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    /// Who wrote it: `None` for the phone itself.
    pub from: Option<(String, String)>,
    pub at_ms: i64,
    pub body: String,
}

/// One conversation: with a number, or a group of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    /// The other numbers, as matched, sorted.
    pub key: String,
    pub name: String,
    pub messages: Vec<Text>,
}

/// What was read, and what was left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Backup {
    pub threads: Vec<Thread>,
    pub sent: usize,
    /// Drafts, unsent and failed messages, and pictures without words.
    pub left_out: usize,
}

fn number_key(number: &str) -> String {
    IdentifierInput::new(IdentifierKind::Phone, number).normalized()
}

/// Read a backup.
pub fn read_backup(location: &Path) -> SourceResult<Backup> {
    let mut bytes = Vec::new();
    std::fs::File::open(location)?.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(SourceError::Malformed(
            "This backup is too large to read: make one without MMS pictures (in SMS Backup & Restore, leave out media) and choose that.".into(),
        ));
    }
    let text = String::from_utf8_lossy(&bytes);
    if !text.contains("<smses") {
        return Err(SourceError::Malformed(
            "This isn't a backup from SMS Backup & Restore: choose the .xml file it made, starting sms-.".into(),
        ));
    }
    let mut backup = Backup::default();
    let mut threads: Vec<Thread> = Vec::new();
    for e in elements(&text) {
        let Some(at_ms) = e.get("date").and_then(|d| d.parse::<i64>().ok()) else {
            backup.left_out += 1;
            continue;
        };
        let (kind, others, body, sender) = if e.name == "sms" {
            let Some(address) = e.get("address") else {
                backup.left_out += 1;
                continue;
            };
            (e.get("type"), vec![address.to_string()], e.get("body").map(str::to_string), Some(address.to_string()))
        } else {
            let others: Vec<String> = e
                .get("address")
                .map(|a| a.split('~').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect())
                .unwrap_or_default();
            let words: Vec<String> = e
                .children
                .iter()
                .filter(|c| c.name == "part" && c.get("ct") == Some("text/plain"))
                .filter_map(|c| c.get("text").map(str::to_string))
                .collect();
            let from = e
                .children
                .iter()
                .find(|c| c.name == "addr" && c.get("type") == Some("137"))
                .and_then(|c| c.get("address"))
                .map(str::to_string);
            let body = (!words.is_empty()).then(|| words.join("\n"));
            (e.get("msg_box"), others, body, from)
        };
        let sent = match kind {
            Some("1") => false,
            Some("2") => true,
            // Drafts, the outbox, failed and queued.
            _ => {
                backup.left_out += 1;
                continue;
            }
        };
        let Some(body) = body.map(|b| b.trim().to_string()).filter(|b| !b.is_empty()) else {
            backup.left_out += 1;
            continue;
        };
        if others.is_empty() {
            backup.left_out += 1;
            continue;
        }
        let mut keys: Vec<String> = others.iter().map(|n| number_key(n)).filter(|k| !k.is_empty()).collect();
        keys.sort();
        keys.dedup();
        let key = keys.join("~");
        let names: Vec<String> = e
            .get("contact_name")
            .filter(|n| *n != "(Unknown)")
            .map(|n| n.split(',').map(|p| p.trim().to_string()).collect())
            .unwrap_or_default();
        let name_for = |number: &str| -> String {
            let at = others.iter().position(|n| number_key(n) == number_key(number));
            at.and_then(|p| names.get(p).cloned()).filter(|n| !n.is_empty()).unwrap_or_else(|| number.to_string())
        };
        let from = if sent {
            backup.sent += 1;
            None
        } else {
            let number = sender.unwrap_or_else(|| others[0].clone());
            Some((name_for(&number), number))
        };
        let message = Text { from, at_ms, body };
        match threads.iter_mut().find(|t| t.key == key) {
            Some(t) => t.messages.push(message),
            None => threads.push(Thread {
                name: if names.is_empty() { others.join(", ") } else { names.join(", ") },
                key,
                messages: vec![message],
            }),
        }
    }
    for t in &mut threads {
        t.messages.sort_by_key(|m| m.at_ms);
    }
    backup.threads = threads;
    Ok(backup)
}

fn author(from: &Option<(String, String)>) -> AuthorRef {
    match from {
        None => AuthorRef {
            display_name: "This phone".into(),
            identifiers: vec![IdentifierInput::new(IdentifierKind::Handle, THIS_PHONE)],
        },
        Some((name, number)) => AuthorRef {
            display_name: name.clone(),
            identifiers: vec![IdentifierInput::new(IdentifierKind::Phone, number)],
        },
    }
}

impl CommunicationSource for SmsSource {
    fn metadata(&self) -> SourceMetadata {
        SourceMetadata {
            connector: "sms",
            display_name: "Text messages (Android backup)",
            channel: "sms",
            description: "The .xml file SMS Backup & Restore makes on an Android phone. Both sides of each conversation, so I learn how you text back.",
            location_kind: LocationKind::File,
            extensions: &["xml"],
        }
    }

    fn discover(&self, location: &Path) -> SourceResult<Vec<PathBuf>> {
        Ok(if location.is_file() { vec![location.to_path_buf()] } else { Vec::new() })
    }

    fn validate(&self, location: &Path) -> SourceResult<ValidationReport> {
        let mut report = validate_by_dry_run(self, location)?;
        let backup = read_backup(location)?;
        // Only the phone itself is asked about: everyone else is a number.
        if backup.sent > 0 {
            let id = IdentifierInput::new(IdentifierKind::Handle, THIS_PHONE);
            report.names = vec![WriterName {
                name: "Messages this phone sent".into(),
                kind: id.kind.as_str().to_string(),
                value: id.value.clone(),
                normalized: id.normalized(),
                messages: backup.sent,
                chat_named_after: false,
                also: Vec::new(),
            }];
        }
        if backup.left_out > 0 {
            report.warnings.push(format!(
                "{} left out: drafts, messages that didn't send, and pictures without words.",
                if backup.left_out == 1 {
                    "1 message was".to_string()
                } else {
                    format!("{} messages were", backup.left_out)
                },
            ));
        }
        Ok(report)
    }

    fn import(
        &self,
        location: &Path,
        sink: &mut dyn FnMut(DiscoveredConversation) -> SourceResult<()>,
    ) -> SourceResult<()> {
        for thread in read_backup(location)?.threads {
            let messages = thread
                .messages
                .iter()
                .map(|m| {
                    let who = m.from.as_ref().map_or(THIS_PHONE.to_string(), |(_, n)| number_key(n));
                    let digest = crate::ids::sha256_hex(format!("{who}\n{}", m.body).as_bytes());
                    RawMessage {
                        external_id: format!("sms:{}:{}:{}", thread.key, m.at_ms, &digest[..12]),
                        author: author(&m.from),
                        sent_at: Utc
                            .timestamp_millis_opt(m.at_ms)
                            .single()
                            .map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true)),
                        body: m.body.clone(),
                        metadata: Value::Null,
                    }
                })
                .collect();
            sink(DiscoveredConversation {
                external_id: format!("sms:{}", thread.key),
                subject: Some(thread.name),
                channel: "sms".into(),
                messages,
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACKUP: &str = r#"<?xml version='1.0' encoding='UTF-8' standalone='yes' ?>
<!--File Created By SMS Backup & Restore v10.18.003-->
<smses count="7" backup_set="x" backup_date="1770200000000" type="full">
  <sms protocol="0" address="+1 (555) 010-0100" date="1770110040000" type="1" subject="null" body="lunch on thursday? &gt;_&lt; &#55357;&#56832;" toa="null" sc_toa="null" service_center="null" read="1" status="-1" locked="0" date_sent="1770110039000" sub_id="1" readable_date="Feb 3, 2026 9:14:00 AM" contact_name="Ada Lovelace" />
  <sms protocol="0" address="+15550100100" date="1770110400000" type="2" subject="null" body="yes! &quot;noon&quot; works" read="1" status="-1" locked="0" date_sent="0" contact_name="Ada Lovelace" />
  <sms protocol="0" address="+15550100100" date="1770110500000" type="3" body="a draft" contact_name="Ada Lovelace" />
  <sms protocol="0" address="+15550100100" date="1770110600000" type="5" body="didn't send" contact_name="Ada Lovelace" />
  <mms date="1770111000000" ct_t="application/vnd.wap.multipart.related" msg_box="1" address="+15550100100~+15550100101" contact_name="Ada Lovelace, Charles Babbage">
    <parts>
      <part seq="-1" ct="application/smil" text="&lt;smil&gt;&lt;/smil&gt;" />
      <part seq="0" ct="text/plain" text="dinner sunday?" />
    </parts>
    <addrs>
      <addr address="+15550100101" type="137" charset="106" />
      <addr address="+15550100100" type="151" charset="106" />
    </addrs>
  </mms>
  <mms date="1770111100000" msg_box="1" address="+15550100100~+15550100101" contact_name="Ada Lovelace, Charles Babbage">
    <parts><part seq="0" ct="image/jpeg" name="a.jpg" data="/9j/4AAQSkZJRgABAQ>==" /></parts>
    <addrs><addr address="+15550100100" type="137" /></addrs>
  </mms>
  <mms date="1770111200000" msg_box="2" address="+15550100100~+15550100101" contact_name="Ada Lovelace, Charles Babbage">
    <parts><part seq="0" ct="text/plain" text="sunday works" /></parts>
    <addrs><addr address="insert-address-token" type="137" /></addrs>
  </mms>
</smses>"#;

    fn write(dir: &tempfile::TempDir) -> PathBuf {
        let path = dir.path().join("sms-20260204.xml");
        std::fs::write(&path, BACKUP).unwrap();
        path
    }

    #[test]
    fn entities_and_an_emoji_in_two_halves_read_as_written() {
        assert_eq!(decoded("a &amp; b &lt;c&gt; &quot;d&quot; &apos;e&apos;"), "a & b <c> \"d\" 'e'");
        assert_eq!(decoded("&#55357;&#56832; hi"), "😀 hi");
        assert_eq!(decoded("&#x1F600;&#10;x"), "😀\nx");
        assert_eq!(decoded("fish & chips"), "fish & chips", "a bare & is itself");
    }

    #[test]
    fn a_backup_is_read_both_sides_one_conversation_per_number_or_group() {
        let dir = tempfile::tempdir().unwrap();
        let backup = read_backup(&write(&dir)).unwrap();
        assert_eq!(backup.threads.len(), 2, "Ada; Ada and Charles");
        let ada = &backup.threads[0];
        assert_eq!(ada.name, "Ada Lovelace");
        let said: Vec<(Option<&str>, &str)> =
            ada.messages.iter().map(|m| (m.from.as_ref().map(|(n, _)| n.as_str()), m.body.as_str())).collect();
        assert_eq!(said, [(Some("Ada Lovelace"), "lunch on thursday? >_< 😀"), (None, "yes! \"noon\" works")]);
        let group = &backup.threads[1];
        assert_eq!(group.name, "Ada Lovelace, Charles Babbage");
        let said: Vec<(Option<&str>, &str)> =
            group.messages.iter().map(|m| (m.from.as_ref().map(|(n, _)| n.as_str()), m.body.as_str())).collect();
        assert_eq!(said, [(Some("Charles Babbage"), "dinner sunday?"), (None, "sunday works")]);
        assert_eq!((backup.sent, backup.left_out), (2, 3), "the draft, the failed one, the picture");
    }

    #[test]
    fn only_the_phone_itself_is_asked_about() {
        let dir = tempfile::tempdir().unwrap();
        let report = SmsSource.validate(&write(&dir)).unwrap();
        assert!(report.ok, "{report:?}");
        assert_eq!(report.messages, 4);
        assert_eq!(report.names.len(), 1);
        assert_eq!((report.names[0].value.as_str(), report.names[0].messages), (THIS_PHONE, 2));
        let mut seen = Vec::new();
        SmsSource
            .import(&write(&dir), &mut |c| {
                seen.push((c.channel.clone(), c.messages[0].sent_at.clone()));
                Ok(())
            })
            .unwrap();
        assert_eq!(seen[0], ("sms".to_string(), Some("2026-02-03T09:14:00Z".to_string())));
    }

    #[test]
    fn a_file_that_isnt_a_backup_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("other.xml");
        std::fs::write(&path, "<notes><note/></notes>").unwrap();
        assert!(read_backup(&path).unwrap_err().to_string().contains("SMS Backup & Restore"));
    }
}
