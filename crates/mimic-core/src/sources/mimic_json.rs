//! The `mimic_json` connector: the documented generic export format.
//!
//! This is the format every other exporter can be converted into, and the one
//! the importer's own tests use. It is deliberately boring:
//!
//! ```json
//! {
//!   "channel": "chat",
//!   "conversations": [
//!     {
//!       "id": "thread-1",
//!       "subject": "Lunch",
//!       "messages": [
//!         { "id": "m1", "sentAt": "2026-02-03T09:14:00Z",
//!           "from": { "name": "Ada", "email": "ada@example.com" },
//!           "body": "Does Tuesday work?" }
//!       ]
//!     }
//!   ]
//! }
//! ```
//!
//! `from` may carry any of `email`, `phone`, `handle` or `accountId`; at least
//! one is required, because a name alone cannot be matched to an identity.
//! See docs/IMPORT_PIPELINE.md for the full specification.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use super::normalize::clean_body;
use super::{
    validate_by_dry_run, AuthorRef, CommunicationSource, DiscoveredConversation, LocationKind, RawMessage, SourceError,
    SourceMetadata, SourceResult, ValidationReport,
};
use crate::db::repo_people::IdentifierInput;
use crate::db::IdentifierKind;

pub struct MimicJsonSource;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Export {
    #[serde(default = "default_channel")]
    channel: String,
    #[serde(default)]
    conversations: Vec<ExportConversation>,
}

fn default_channel() -> String {
    "other".into()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportConversation {
    id: String,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    messages: Vec<ExportMessage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportMessage {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    sent_at: Option<String>,
    from: ExportAuthor,
    body: String,
    #[serde(default)]
    metadata: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportAuthor {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    phone: Option<String>,
    #[serde(default)]
    handle: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
}

impl ExportAuthor {
    fn to_ref(&self) -> AuthorRef {
        let mut identifiers = Vec::new();
        for (kind, value) in [
            (IdentifierKind::Email, &self.email),
            (IdentifierKind::Phone, &self.phone),
            (IdentifierKind::Handle, &self.handle),
            (IdentifierKind::AccountId, &self.account_id),
        ] {
            if let Some(v) = value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                identifiers.push(IdentifierInput::new(kind, v));
            }
        }
        AuthorRef { display_name: self.name.clone().unwrap_or_default(), identifiers }
    }
}

impl CommunicationSource for MimicJsonSource {
    fn metadata(&self) -> SourceMetadata {
        SourceMetadata {
            connector: "mimic_json",
            display_name: "Mimic export (JSON)",
            channel: "other",
            description: "A conversation export in Mimic's own JSON format. Use this to bring in anything Mimic has no dedicated connector for.",
            location_kind: LocationKind::File,
            extensions: &["json"],
        }
    }

    fn discover(&self, location: &Path) -> SourceResult<Vec<PathBuf>> {
        if location.is_file() {
            Ok(vec![location.to_path_buf()])
        } else {
            Ok(Vec::new())
        }
    }

    fn validate(&self, location: &Path) -> SourceResult<ValidationReport> {
        let mut report = validate_by_dry_run(self, location)?;
        // A name with no address is the one shape this format cannot recover
        // from, so it is called out rather than left as a silent 'unknown'.
        let text = std::fs::read_to_string(location)?;
        let export: Export = serde_json::from_str(&text).map_err(|e| SourceError::Malformed(e.to_string()))?;
        let nameless =
            export.conversations.iter().flat_map(|c| &c.messages).filter(|m| !m.from.to_ref().is_usable()).count();
        if nameless > 0 {
            report.warnings.push(format!(
                "{nameless} messages have an author with no email, phone, handle or account id. They will import as 'unknown' and will not be used to learn your voice."
            ));
        }
        if !crate::db::channel_is_known(&export.channel) {
            report.blockers.push(format!(
                "\"{}\" is not a channel Mimic knows. Use email, sms, chat, forum or other.",
                export.channel
            ));
            report.ok = false;
        }
        Ok(report)
    }

    fn import(
        &self,
        location: &Path,
        sink: &mut dyn FnMut(DiscoveredConversation) -> SourceResult<()>,
    ) -> SourceResult<()> {
        let text = std::fs::read_to_string(location)?;
        let export: Export = serde_json::from_str(&text).map_err(|e| SourceError::Malformed(e.to_string()))?;
        for convo in export.conversations {
            let channel = convo.channel.clone().unwrap_or_else(|| export.channel.clone());
            let mut messages = Vec::with_capacity(convo.messages.len());
            for (i, m) in convo.messages.iter().enumerate() {
                let body = clean_body(&m.body);
                if body.trim().is_empty() {
                    continue;
                }
                messages.push(RawMessage {
                    // Position is a stable fallback id: the same file
                    // re-imported produces the same ids, so nothing doubles.
                    external_id: m.id.clone().unwrap_or_else(|| format!("{}#{i}", convo.id)),
                    author: m.from.to_ref(),
                    sent_at: m.sent_at.clone().filter(|s| !s.trim().is_empty()),
                    body,
                    metadata: match &m.metadata {
                        Value::Object(map) => {
                            let mut map = map.clone();
                            // `automated` is Mimic's reading of email headers,
                            // and a file in this format has none to read. Left
                            // in, it would let a file claim a person was a
                            // machine, in words that say the headers did.
                            map.remove("automated");
                            Value::Object(map)
                        }
                        _ => Value::Null,
                    },
                });
            }
            if messages.is_empty() {
                continue;
            }
            sink(DiscoveredConversation { external_id: convo.id, subject: convo.subject, channel, messages })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(text: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("export.json");
        std::fs::write(&path, text).unwrap();
        (dir, path)
    }

    const SAMPLE: &str = r#"{
      "channel": "chat",
      "conversations": [
        { "id": "t1", "subject": "Lunch", "messages": [
          { "id": "m1", "sentAt": "2026-02-03T09:14:00Z",
            "from": {"name": "Ada", "handle": "@ada"}, "body": "Does Tuesday work?" },
          { "id": "m2", "sentAt": "2026-02-03T09:20:00Z",
            "from": {"name": "C", "handle": "@c"}, "body": "yeah tuesday's good\n\n> Does Tuesday work?" }
        ]},
        { "id": "t2", "messages": [
          { "from": {"name": "Nobody"}, "body": "who am I" }
        ]}
      ]
    }"#;

    #[test]
    fn conversations_and_authors_are_read() {
        let (_d, path) = write(SAMPLE);
        let mut got = Vec::new();
        MimicJsonSource
            .import(&path, &mut |c| {
                got.push(c);
                Ok(())
            })
            .unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].external_id, "t1");
        assert_eq!(got[0].channel, "chat", "a conversation inherits the export channel");
        assert_eq!(got[0].messages.len(), 2);
        assert_eq!(got[0].messages[1].body, "yeah tuesday's good", "quoted text is stripped on the way in");
        assert_eq!(got[0].messages[0].author.identifiers[0].value, "@ada");
        assert!(!got[1].messages[0].author.is_usable(), "a name with no address cannot be matched");
    }

    #[test]
    fn a_file_cannot_say_a_message_was_sent_by_a_machine() {
        let (_d, path) = write(
            r#"{"channel": "email", "conversations": [{"id": "t", "messages": [
                {"from": {"name": "Ada", "email": "ada@example.com"}, "body": "hello",
                 "metadata": {"automated": true, "subject": "hi"}}
            ]}]}"#,
        );
        let mut got = Vec::new();
        MimicJsonSource
            .import(&path, &mut |c| {
                got.push(c);
                Ok(())
            })
            .unwrap();
        let meta = &got[0].messages[0].metadata;
        assert!(meta.get("automated").is_none());
        assert_eq!(meta["subject"], "hi", "everything else in it is kept as it was");
    }

    #[test]
    fn a_message_with_no_id_gets_a_stable_one() {
        let (_d, path) = write(SAMPLE);
        let read = |p: &Path| {
            let mut ids = Vec::new();
            MimicJsonSource
                .import(p, &mut |c| {
                    ids.extend(c.messages.iter().map(|m| m.external_id.clone()));
                    Ok(())
                })
                .unwrap();
            ids
        };
        assert_eq!(read(&path), vec!["m1", "m2", "t2#0"]);
        assert_eq!(read(&path), read(&path), "re-reading produces identical ids");
    }

    #[test]
    fn validation_reports_shape_and_the_problems_worth_naming() {
        let (_d, path) = write(SAMPLE);
        let r = MimicJsonSource.validate(&path).unwrap();
        assert!(r.ok);
        assert_eq!((r.conversations, r.messages), (2, 3));
        assert_eq!(r.earliest.as_deref(), Some("2026-02-03T09:14:00Z"));
        assert!(r.warnings.iter().any(|w| w.contains("no email, phone, handle or account id")));
        assert!(r.warnings.iter().any(|w| w.contains("Only 3 messages")));
    }

    #[test]
    fn an_unknown_channel_blocks_the_import() {
        let (_d, path) = write(
            r#"{"channel":"telepathy","conversations":[{"id":"t","messages":[
            {"from":{"handle":"@a"},"body":"hi"}]}]}"#,
        );
        let r = MimicJsonSource.validate(&path).unwrap();
        assert!(!r.ok);
        assert!(r.blockers[0].contains("telepathy"));
    }

    #[test]
    fn an_empty_file_blocks_and_malformed_json_errors() {
        let (_d, path) = write(r#"{"channel":"chat","conversations":[]}"#);
        let r = MimicJsonSource.validate(&path).unwrap();
        assert!(!r.ok);
        assert_eq!(r.blockers, vec!["No messages could be read from this file."]);

        let (_d2, bad) = write("{not json");
        assert!(matches!(MimicJsonSource.validate(&bad), Err(SourceError::Malformed(_))));
    }

    #[test]
    fn the_sink_can_abort_an_import() {
        let (_d, path) = write(SAMPLE);
        let mut seen = 0;
        let err = MimicJsonSource
            .import(&path, &mut |_| {
                seen += 1;
                Err(SourceError::Aborted("canceled".into()))
            })
            .unwrap_err();
        assert!(matches!(err, SourceError::Aborted(_)));
        assert_eq!(seen, 1, "the connector stops at the first refusal");
    }

    #[test]
    fn discover_reports_nothing_for_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(MimicJsonSource.discover(dir.path()).unwrap().is_empty());
    }
}
