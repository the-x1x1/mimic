//! Row types. Field names mirror columns; JSON columns are `serde_json::Value`
//! so the frontend receives structured data, not double-encoded strings.
//!
//! Every type here is `camelCase` on the wire and has a matching zod schema in
//! `packages/contracts`. The contract fixtures under `fixtures/` are parsed by
//! both sides, so a change on one side that is not made on the other fails a
//! test rather than rendering `undefined`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Parse a nullable JSON text column; malformed JSON becomes `None` rather
/// than failing a whole list query.
pub(crate) fn json_col(raw: Option<String>) -> Option<Value> {
    raw.and_then(|s| serde_json::from_str(&s).ok())
}

/// Parse a non-null JSON text column, falling back to `null`.
pub(crate) fn json_col_or_default(raw: String) -> Value {
    serde_json::from_str(&raw).unwrap_or(Value::Null)
}

/// Parse a JSON object column, falling back to `{}`.
pub(crate) fn json_obj(raw: String) -> Value {
    match serde_json::from_str(&raw) {
        Ok(v @ Value::Object(_)) => v,
        _ => Value::Object(serde_json::Map::new()),
    }
}

// ----------------------------------------------------------------- generic

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub status: String,
    pub payload: Value,
    pub progress_current: i64,
    pub progress_total: i64,
    pub phase: Option<String>,
    pub resumable: bool,
    pub created_at: String,
    pub started_at: Option<String>,
    pub heartbeat_at: Option<String>,
    pub completed_at: Option<String>,
    pub result: Option<Value>,
    pub error: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventRow {
    pub id: i64,
    pub level: String,
    pub category: String,
    pub event_type: String,
    pub entity_type: Option<String>,
    pub entity_id: Option<String>,
    pub payload_json: String,
    pub created_at: String,
}

pub struct NewEvent<'a> {
    pub level: &'a str,
    pub category: &'a str,
    pub event_type: &'a str,
    pub entity_type: Option<&'a str>,
    pub entity_id: Option<&'a str>,
    pub payload: Value,
}

impl<'a> NewEvent<'a> {
    pub fn info(category: &'a str, event_type: &'a str, payload: Value) -> Self {
        Self { level: "info", category, event_type, entity_type: None, entity_id: None, payload }
    }
    pub fn warn(category: &'a str, event_type: &'a str, payload: Value) -> Self {
        Self { level: "warn", category, event_type, entity_type: None, entity_id: None, payload }
    }
    pub fn error(category: &'a str, event_type: &'a str, payload: Value) -> Self {
        Self { level: "error", category, event_type, entity_type: None, entity_id: None, payload }
    }
    pub fn entity(mut self, entity_type: &'a str, entity_id: &'a str) -> Self {
        self.entity_type = Some(entity_type);
        self.entity_id = Some(entity_id);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    pub current_version: String,
    pub latest_seen_version: Option<String>,
    pub staged_version: Option<String>,
    pub channel: String,
    pub last_checked_at: Option<String>,
    pub last_update_result: Option<String>,
    pub update_error: Option<String>,
}

// ---------------------------------------------------------------- identity

/// How someone is addressed. `normalized_value` is what matching uses:
/// lowercased for emails and handles, digits-only for phone numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentifierKind {
    Email,
    Phone,
    Handle,
    DisplayName,
    AccountId,
}

impl IdentifierKind {
    pub fn as_str(self) -> &'static str {
        match self {
            IdentifierKind::Email => "email",
            IdentifierKind::Phone => "phone",
            IdentifierKind::Handle => "handle",
            IdentifierKind::DisplayName => "display_name",
            IdentifierKind::AccountId => "account_id",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "email" => IdentifierKind::Email,
            "phone" => IdentifierKind::Phone,
            "handle" => IdentifierKind::Handle,
            "display_name" => IdentifierKind::DisplayName,
            "account_id" => IdentifierKind::AccountId,
            _ => return None,
        })
    }

    /// The comparison form. Email and handle fold case; a phone number keeps
    /// only its digits so `+1 (555) 010-9999` and `5550109999` are one person.
    pub fn normalize(self, value: &str) -> String {
        match self {
            IdentifierKind::Email | IdentifierKind::Handle => value.trim().to_lowercase(),
            IdentifierKind::Phone => {
                let digits: String = value.chars().filter(char::is_ascii_digit).collect();
                // Strip a leading country code only when it leaves a plausible number.
                if digits.len() == 11 && digits.starts_with('1') {
                    digits[1..].to_string()
                } else {
                    digits
                }
            }
            IdentifierKind::DisplayName => value.trim().to_lowercase(),
            IdentifierKind::AccountId => value.trim().to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identifier {
    pub id: String,
    pub kind: String,
    pub value: String,
    pub normalized_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserIdentity {
    pub id: String,
    pub display_name: String,
    pub identifiers: Vec<Identifier>,
    pub created_at: String,
    pub updated_at: String,
}

// ------------------------------------------------------------------ source

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    pub connector: String,
    pub name: String,
    pub channel: String,
    pub location: Option<String>,
    pub config: Value,
    pub status: String,
    pub created_at: String,
    pub last_imported_at: Option<String>,
    pub message_count: i64,
    pub last_error: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSource {
    pub connector: String,
    pub name: String,
    pub channel: String,
    pub location: Option<String>,
    #[serde(default)]
    pub config: Value,
}

// ------------------------------------------------------------- participant

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Participant {
    pub id: String,
    pub display_name: String,
    pub is_self: bool,
    /// User-declared, e.g. "colleague", "close friend". Never inferred.
    pub relationship: Option<String>,
    pub notes: Option<String>,
    pub identifiers: Vec<Identifier>,
    pub created_at: String,
    pub updated_at: String,
}

/// List-view counts, computed with one grouped query rather than per row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantSummary {
    pub participant: Participant,
    pub message_count: i64,
    pub sent_by_user: i64,
    pub conversation_count: i64,
    pub channels: Vec<String>,
    pub first_message_at: Option<String>,
    pub last_message_at: Option<String>,
    pub has_relationship_profile: bool,
}

// ------------------------------------------------------------ conversation

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    pub source_id: String,
    pub external_id: String,
    pub channel: String,
    pub subject: Option<String>,
    pub is_group: bool,
    pub started_at: Option<String>,
    pub last_message_at: Option<String>,
    pub message_count: i64,
    pub created_at: String,
}

// ---------------------------------------------------------------- messages

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Written by the user. The only messages a voice profile is built from.
    Self_,
    /// Written by someone else.
    Other,
    /// The author could not be matched to the user or to a participant.
    /// These are kept — they are conversational context — but never treated
    /// as evidence of how the user writes.
    Unknown,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Self_ => "self",
            Direction::Other => "other",
            Direction::Unknown => "unknown",
        }
    }
    pub fn parse(s: &str) -> Direction {
        match s {
            "self" => Direction::Self_,
            "other" => Direction::Other,
            _ => Direction::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub source_id: String,
    pub participant_id: Option<String>,
    pub external_id: String,
    pub direction: String,
    pub channel: String,
    pub sent_at: Option<String>,
    pub sequence_index: i64,
    pub body: String,
    pub word_count: i64,
    pub char_count: i64,
    pub reply_to_message_id: Option<String>,
    pub response_latency_seconds: Option<i64>,
    pub metadata: Value,
}

// ------------------------------------------------------------------- voice

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceLayer {
    Global,
    Channel,
    Relationship,
    Situational,
}

impl VoiceLayer {
    pub fn as_str(self) -> &'static str {
        match self {
            VoiceLayer::Global => "global",
            VoiceLayer::Channel => "channel",
            VoiceLayer::Relationship => "relationship",
            VoiceLayer::Situational => "situational",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "global" => VoiceLayer::Global,
            "channel" => VoiceLayer::Channel,
            "relationship" => VoiceLayer::Relationship,
            "situational" => VoiceLayer::Situational,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceProfileRow {
    pub id: String,
    pub layer: String,
    pub scope_key: String,
    pub participant_id: Option<String>,
    pub metrics: Value,
    pub qualitative: Value,
    pub sample_size: i64,
    pub analysis_version: String,
    pub computed_at: String,
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoicePreference {
    pub id: String,
    pub layer: String,
    pub scope_key: String,
    pub key: String,
    pub value: Value,
    pub note: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepresentativeExample {
    pub id: String,
    pub message_id: String,
    pub layer: String,
    pub scope_key: String,
    pub participant_id: Option<String>,
    pub reason: String,
    pub score: f64,
    pub body: String,
    pub sent_at: Option<String>,
}

// ------------------------------------------------------------------ drafts

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub id: String,
    pub participant_id: Option<String>,
    pub conversation_id: Option<String>,
    pub channel: String,
    pub situation_id: Option<String>,
    pub incoming_message: Option<String>,
    pub intent: Option<String>,
    pub generated_text: String,
    pub final_text: Option<String>,
    pub provider: String,
    pub model: String,
    pub context: Value,
    pub prompt_hash: String,
    pub evidence: Value,
    pub created_at: String,
    pub resolved_at: Option<String>,
    pub outcome: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftFeedback {
    pub id: String,
    pub draft_id: String,
    pub kind: String,
    pub weight: f64,
    pub diff: Value,
    pub note: Option<String>,
    pub created_at: String,
    pub applied_to_analysis_version: Option<String>,
}

// ---------------------------------------------------------------- analysis

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisRun {
    pub id: String,
    pub kind: String,
    pub analysis_version: String,
    pub scope: Value,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub status: String,
    pub messages_considered: i64,
    pub profiles_written: i64,
    pub error: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phone_numbers_normalize_to_one_form() {
        let k = IdentifierKind::Phone;
        assert_eq!(k.normalize("+1 (555) 010-9999"), "5550109999");
        assert_eq!(k.normalize("555-010-9999"), "5550109999");
        assert_eq!(k.normalize("15550109999"), "5550109999");
        // A short number keeps its leading 1 rather than being mangled.
        assert_eq!(k.normalize("1800"), "1800");
        // An international number that is not 11 digits is left intact.
        assert_eq!(k.normalize("+44 20 7946 0018"), "442079460018");
    }

    #[test]
    fn emails_and_handles_fold_case_account_ids_do_not() {
        assert_eq!(IdentifierKind::Email.normalize("  Ada@Example.COM "), "ada@example.com");
        assert_eq!(IdentifierKind::Handle.normalize("@AdaL"), "@adal");
        assert_eq!(IdentifierKind::AccountId.normalize(" U01AB "), "U01AB");
    }

    #[test]
    fn identifier_kind_round_trips() {
        for k in [
            IdentifierKind::Email,
            IdentifierKind::Phone,
            IdentifierKind::Handle,
            IdentifierKind::DisplayName,
            IdentifierKind::AccountId,
        ] {
            assert_eq!(IdentifierKind::parse(k.as_str()), Some(k));
        }
        assert!(IdentifierKind::parse("nope").is_none());
    }

    #[test]
    fn unknown_direction_is_the_safe_default() {
        assert_eq!(Direction::parse("self"), Direction::Self_);
        assert_eq!(Direction::parse("other"), Direction::Other);
        assert_eq!(Direction::parse("garbage"), Direction::Unknown);
        assert_eq!(Direction::Self_.as_str(), "self");
    }

    #[test]
    fn voice_layers_round_trip() {
        for l in [VoiceLayer::Global, VoiceLayer::Channel, VoiceLayer::Relationship, VoiceLayer::Situational] {
            assert_eq!(VoiceLayer::parse(l.as_str()), Some(l));
        }
        assert_eq!(VoiceLayer::parse("nope"), None);
    }
}
