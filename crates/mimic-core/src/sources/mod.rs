//! Communication sources.
//!
//! A connector's whole job is to turn some export format into the canonical
//! model: conversations made of messages, each with an author that can be
//! matched to an identity. Nothing downstream knows what an mbox is.
//!
//! Connectors stream. `import` hands one conversation at a time to a sink
//! rather than returning a `Vec`, so a 2 GB mailbox is bounded by the largest
//! single thread in it and not by the file size.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::repo_people::IdentifierInput;

pub mod automated;
pub mod imap;
pub mod mbox;
pub mod mime;
pub mod mimic_json;
pub mod normalize;
pub mod oauth;

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Malformed(String),
    #[error("unsupported connector {0:?}")]
    UnknownConnector(String),
    #[error("{0}")]
    Aborted(String),
}

pub type SourceResult<T> = Result<T, SourceError>;

/// Where a connector expects to be pointed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LocationKind {
    File,
    Folder,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceMetadata {
    /// Stable key stored in `sources.connector`.
    pub connector: &'static str,
    pub display_name: &'static str,
    /// Default channel for messages from this connector.
    pub channel: &'static str,
    pub description: &'static str,
    pub location_kind: LocationKind,
    /// File extensions the picker should offer, lowercase and without the dot.
    pub extensions: &'static [&'static str],
}

/// One author as an export describes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorRef {
    pub display_name: String,
    pub identifiers: Vec<IdentifierInput>,
}

impl AuthorRef {
    pub fn is_usable(&self) -> bool {
        self.identifiers.iter().any(|i| !i.normalized().is_empty())
    }
}

#[derive(Debug, Clone)]
pub struct RawMessage {
    /// Stable within the source. A connector that has no natural id must
    /// derive a deterministic one, so a re-import does not duplicate.
    pub external_id: String,
    pub author: AuthorRef,
    pub sent_at: Option<String>,
    pub body: String,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct DiscoveredConversation {
    pub external_id: String,
    pub subject: Option<String>,
    pub channel: String,
    pub messages: Vec<RawMessage>,
}

/// What `validate` reports before the user commits to an import. Blockers stop
/// the import; warnings do not.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationReport {
    pub ok: bool,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
    pub conversations: usize,
    pub messages: usize,
    /// Addresses seen most often, so the user can confirm which are theirs.
    pub frequent_identifiers: Vec<(String, usize)>,
    pub earliest: Option<String>,
    pub latest: Option<String>,
}

/// Implemented by every connector.
pub trait CommunicationSource: Send + Sync {
    fn metadata(&self) -> SourceMetadata;

    /// Files this connector can read at `location`. For a file-kind connector
    /// this is the file itself when it is readable, and empty otherwise.
    fn discover(&self, location: &Path) -> SourceResult<Vec<PathBuf>>;

    /// Read enough to report shape and problems without importing.
    fn validate(&self, location: &Path) -> SourceResult<ValidationReport>;

    /// Stream every conversation to `sink`. The sink returning `Err` aborts
    /// the import — that is how cancellation reaches a connector.
    fn import(
        &self,
        location: &Path,
        sink: &mut dyn FnMut(DiscoveredConversation) -> SourceResult<()>,
    ) -> SourceResult<()>;
}

/// Every connector this build ships.
pub fn all() -> Vec<Box<dyn CommunicationSource>> {
    vec![Box::new(mimic_json::MimicJsonSource), Box::new(mbox::MboxSource)]
}

pub fn by_connector(connector: &str) -> SourceResult<Box<dyn CommunicationSource>> {
    all()
        .into_iter()
        .find(|s| s.metadata().connector == connector)
        .ok_or_else(|| SourceError::UnknownConnector(connector.to_string()))
}

/// Shared helper: build a validation report by walking a connector's own
/// `import`, so `validate` can never disagree with what the import will do.
pub fn validate_by_dry_run(source: &dyn CommunicationSource, location: &Path) -> SourceResult<ValidationReport> {
    use std::collections::HashMap;
    let mut report = ValidationReport::default();
    let mut seen: HashMap<String, usize> = HashMap::new();
    source.import(location, &mut |convo| {
        report.conversations += 1;
        for m in &convo.messages {
            report.messages += 1;
            for id in &m.author.identifiers {
                if !id.normalized().is_empty() {
                    *seen.entry(id.value.clone()).or_default() += 1;
                }
            }
            if let Some(at) = &m.sent_at {
                if report.earliest.as_deref().is_none_or(|e| at.as_str() < e) {
                    report.earliest = Some(at.clone());
                }
                if report.latest.as_deref().is_none_or(|l| at.as_str() > l) {
                    report.latest = Some(at.clone());
                }
            }
        }
        Ok(())
    })?;
    let mut frequent: Vec<(String, usize)> = seen.into_iter().collect();
    frequent.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    frequent.truncate(10);
    report.frequent_identifiers = frequent;

    if report.messages == 0 {
        report.blockers.push("No messages could be read from this file.".into());
    }
    if report.earliest.is_none() {
        report.warnings.push(
            "No timestamps were found. Messages will import, but response timing cannot be learned from them.".into(),
        );
    }
    if report.messages > 0 && report.messages < 50 {
        report.warnings.push(format!(
            "Only {} messages. That is enough to import, but a voice profile needs a few hundred of your own messages to say anything useful.",
            report.messages
        ));
    }
    report.ok = report.blockers.is_empty();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_connector_has_a_distinct_key_and_a_known_channel() {
        let mut keys = Vec::new();
        for s in all() {
            let m = s.metadata();
            assert!(!keys.contains(&m.connector), "duplicate connector key {}", m.connector);
            assert!(crate::db::channel_is_known(m.channel), "{} has an unknown channel", m.connector);
            assert!(!m.description.is_empty());
            keys.push(m.connector);
        }
        assert!(by_connector("nope").is_err());
        assert!(by_connector("mbox").is_ok());
    }
}
