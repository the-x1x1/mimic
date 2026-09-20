//! Model providers.
//!
//! Generation is the one step that may involve a language model, and the one
//! step where the user's words can leave the machine. So this boundary is
//! narrow on purpose:
//!
//! * One trait, one method. A provider takes an assembled prompt and returns
//!   text. It never sees the database, never decides what goes in the prompt,
//!   and never logs message content.
//! * Credentials never touch the database. A provider is handed them by the
//!   shell through `SecretStore`, backed by the OS credential store.
//! * `local: true` providers are the default and are labelled as such in the
//!   UI, because "this leaves your computer" is the single most important
//!   thing a user can know about a provider.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub mod http;
pub mod mock;

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("{0}")]
    Config(String),
    #[error("provider is not reachable: {0}")]
    Unreachable(String),
    #[error("provider refused the request: {0}")]
    Refused(String),
    #[error("provider returned something unusable: {0}")]
    Malformed(String),
    #[error("no provider named {0:?} is configured")]
    Unknown(String),
}

pub type ProviderResult<T> = Result<T, ProviderError>;

/// What the shell knows about a configured provider. Note the absence of any
/// credential field: those live in `SecretStore`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    pub display_name: String,
    /// True when the request never leaves this machine.
    pub local: bool,
    pub model: String,
    /// One sentence for the Settings screen, in the user's terms.
    pub description: String,
    pub requires_credential: bool,
}

/// An assembled prompt. `system` is the voice instruction; `messages` is the
/// conversation, oldest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationRequest {
    pub system: String,
    pub messages: Vec<PromptMessage>,
    pub max_output_tokens: u32,
    pub temperature: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptMessage {
    /// `user` or `assistant`.
    pub role: String,
    pub content: String,
}

impl PromptMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into() }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: "assistant".into(), content: content.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationResponse {
    pub text: String,
    pub provider: String,
    pub model: String,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

/// Implemented by every provider.
pub trait ModelProvider: Send + Sync {
    fn info(&self) -> ProviderInfo;

    /// Generate one reply. Implementations must not log `request` contents.
    fn generate(&self, request: &GenerationRequest) -> ProviderResult<GenerationResponse>;

    /// Cheap reachability check for the Settings screen. Must not send message
    /// content anywhere.
    fn health(&self) -> ProviderResult<()>;
}

/// Where credentials come from. The Tauri shell implements this over the OS
/// credential store; tests implement it over a map.
pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
}

/// An in-memory store, for tests and for a session where the user typed a key
/// without saving it.
#[derive(Debug, Default)]
pub struct MemorySecrets(pub BTreeMap<String, String>);

impl SecretStore for MemorySecrets {
    fn get(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
}

/// The providers this build can offer, and which one is selected.
pub struct ProviderRegistry {
    providers: Vec<Arc<dyn ModelProvider>>,
}

impl ProviderRegistry {
    pub fn new(providers: Vec<Arc<dyn ModelProvider>>) -> Self {
        Self { providers }
    }

    pub fn list(&self) -> Vec<ProviderInfo> {
        self.providers.iter().map(|p| p.info()).collect()
    }

    pub fn get(&self, id: &str) -> ProviderResult<Arc<dyn ModelProvider>> {
        self.providers.iter().find(|p| p.info().id == id).cloned().ok_or_else(|| ProviderError::Unknown(id.to_string()))
    }

    /// The provider to use when the user has not chosen: the first local one,
    /// falling back to the first of any kind. Local wins because sending a
    /// person's private messages to a third party is not a sensible default.
    pub fn default_id(&self) -> Option<String> {
        self.providers.iter().find(|p| p.info().local).or_else(|| self.providers.first()).map(|p| p.info().id)
    }
}

#[cfg(test)]
mod tests {
    use super::mock::MockProvider;
    use super::*;

    fn remote(id: &'static str) -> Arc<dyn ModelProvider> {
        Arc::new(MockProvider::named(id, false))
    }

    #[test]
    fn the_default_provider_is_a_local_one_when_there_is_one() {
        let reg = ProviderRegistry::new(vec![remote("cloud"), Arc::new(MockProvider::named("local", true))]);
        assert_eq!(reg.default_id().as_deref(), Some("local"), "a cloud provider is never the default by accident");
        let reg = ProviderRegistry::new(vec![remote("cloud")]);
        assert_eq!(reg.default_id().as_deref(), Some("cloud"));
        assert_eq!(ProviderRegistry::new(vec![]).default_id(), None);
    }

    #[test]
    fn an_unknown_provider_is_named_in_the_error() {
        let reg = ProviderRegistry::new(vec![remote("cloud")]);
        assert!(reg.get("cloud").is_ok());
        let err = match reg.get("nope") {
            Err(e) => e,
            Ok(_) => panic!("an unknown provider must not resolve"),
        };
        assert!(err.to_string().contains("nope"), "{err}");
        assert_eq!(reg.list().len(), 1);
    }

    #[test]
    fn secrets_come_from_the_store_and_not_from_the_registry() {
        let store = MemorySecrets(BTreeMap::from([("provider.cloud.apiKey".to_string(), "k".to_string())]));
        assert_eq!(store.get("provider.cloud.apiKey").as_deref(), Some("k"));
        assert_eq!(store.get("provider.cloud.password"), None);
    }
}
