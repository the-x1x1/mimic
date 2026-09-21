//! Two real providers, both over HTTP.
//!
//! * `local` speaks the OpenAI chat-completions shape, which is what Ollama,
//!   LM Studio and llama.cpp's server all expose. Pointed at `127.0.0.1` by
//!   default, so nothing leaves the machine.
//! * `anthropic` speaks the Claude Messages API, for users who would rather
//!   have the quality than the locality, having been told which it is.
//!
//! Neither logs a prompt or a completion. Errors carry the provider's status
//! and message, never the request body.

use serde_json::{json, Value};

use super::{
    GenerationRequest, GenerationResponse, ModelProvider, ProviderError, ProviderInfo, ProviderResult, SecretStore,
};

const TIMEOUT_SECS: u64 = 120;

fn client() -> ProviderResult<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .build()
        .map_err(|e| ProviderError::Config(e.to_string()))
}

/// An OpenAI-compatible endpoint on this machine.
pub struct LocalHttpProvider {
    pub base_url: String,
    pub model: String,
}

impl Default for LocalHttpProvider {
    fn default() -> Self {
        Self { base_url: "http://127.0.0.1:11434/v1".into(), model: "llama3.2:3b".into() }
    }
}

impl LocalHttpProvider {
    /// True when the configured endpoint is a loopback address. A user who
    /// points "local" at someone else's server should not be told their
    /// messages stay on their computer.
    pub fn is_loopback(&self) -> bool {
        let authority = self.base_url.split("//").nth(1).and_then(|rest| rest.split('/').next()).unwrap_or_default();
        // A bracketed IPv6 authority keeps its colons; everything else splits
        // on the port separator.
        let host = match authority.strip_prefix('[').and_then(|r| r.split_once(']')) {
            Some((inner, _)) => inner,
            None => authority.split(':').next().unwrap_or_default(),
        };
        host == "127.0.0.1" || host == "localhost" || host == "::1"
    }
}

impl ModelProvider for LocalHttpProvider {
    fn info(&self) -> ProviderInfo {
        let local = self.is_loopback();
        ProviderInfo {
            id: "local".into(),
            display_name: "Local model".into(),
            local,
            model: self.model.clone(),
            description: if local {
                "A model running on this computer, through Ollama, LM Studio or anything else that speaks the OpenAI API. Nothing you write leaves the machine.".into()
            } else {
                format!("An OpenAI-compatible endpoint at {}. This is not on your computer, so your messages are sent to it.", self.base_url)
            },
            requires_credential: false,
        }
    }

    fn generate(&self, request: &GenerationRequest) -> ProviderResult<GenerationResponse> {
        let mut messages = vec![json!({"role": "system", "content": request.system})];
        messages.extend(request.messages.iter().map(|m| json!({"role": m.role, "content": m.content})));
        let body = json!({
            "model": self.model,
            "messages": messages,
            "max_tokens": request.max_output_tokens,
            "temperature": request.temperature,
            "stream": false,
        });
        let resp = client()?
            .post(format!("{}/chat/completions", self.base_url.trim_end_matches('/')))
            .json(&body)
            .send()
            .map_err(|e| ProviderError::Unreachable(short(&e.to_string())))?;
        let status = resp.status();
        let value: Value = resp.json().map_err(|e| ProviderError::Malformed(short(&e.to_string())))?;
        if !status.is_success() {
            return Err(ProviderError::Refused(format!("{} {}", status.as_u16(), error_message(&value))));
        }
        let text = value["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| ProviderError::Malformed("no message content in the response".into()))?
            .trim()
            .to_string();
        Ok(GenerationResponse {
            text,
            provider: "local".into(),
            model: self.model.clone(),
            input_tokens: value["usage"]["prompt_tokens"].as_u64().map(|n| n as u32),
            output_tokens: value["usage"]["completion_tokens"].as_u64().map(|n| n as u32),
        })
    }

    fn health(&self) -> ProviderResult<()> {
        let resp = client()?
            .get(format!("{}/models", self.base_url.trim_end_matches('/')))
            .send()
            .map_err(|e| ProviderError::Unreachable(short(&e.to_string())))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(ProviderError::Refused(format!("{} from {}", resp.status().as_u16(), self.base_url)))
        }
    }
}

/// The Claude Messages API.
pub struct AnthropicProvider {
    pub model: String,
    pub api_key: Option<String>,
}

impl AnthropicProvider {
    pub const SECRET_KEY: &'static str = "provider.anthropic.apiKey";

    pub fn from_secrets(model: &str, secrets: &dyn SecretStore) -> Self {
        Self { model: model.to_string(), api_key: secrets.get(Self::SECRET_KEY) }
    }

    fn key(&self) -> ProviderResult<&str> {
        self.api_key
            .as_deref()
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| ProviderError::Config("add an API key in Settings before using this provider".into()))
    }
}

impl ModelProvider for AnthropicProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "anthropic".into(),
            display_name: "Claude".into(),
            local: false,
            model: self.model.clone(),
            description:
                "Anthropic's hosted models. The message you are replying to, your intent and a handful of your own past messages are sent to Anthropic for each draft."
                    .into(),
            requires_credential: true,
        }
    }

    fn generate(&self, request: &GenerationRequest) -> ProviderResult<GenerationResponse> {
        let body = json!({
            "model": self.model,
            "system": request.system,
            "messages": request.messages.iter().map(|m| json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
            "max_tokens": request.max_output_tokens,
            "temperature": request.temperature,
        });
        let resp = client()?
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", self.key()?)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .map_err(|e| ProviderError::Unreachable(short(&e.to_string())))?;
        let status = resp.status();
        let value: Value = resp.json().map_err(|e| ProviderError::Malformed(short(&e.to_string())))?;
        if !status.is_success() {
            return Err(ProviderError::Refused(format!("{} {}", status.as_u16(), error_message(&value))));
        }
        let text = value["content"]
            .as_array()
            .and_then(|blocks| blocks.iter().find_map(|b| b["text"].as_str()))
            .ok_or_else(|| ProviderError::Malformed("no text block in the response".into()))?
            .trim()
            .to_string();
        Ok(GenerationResponse {
            text,
            provider: "anthropic".into(),
            model: self.model.clone(),
            input_tokens: value["usage"]["input_tokens"].as_u64().map(|n| n as u32),
            output_tokens: value["usage"]["output_tokens"].as_u64().map(|n| n as u32),
        })
    }

    fn health(&self) -> ProviderResult<()> {
        // Presence of a key is all that can be checked without spending one.
        self.key().map(|_| ())
    }
}

/// Pull a human-readable reason out of whatever error shape came back.
fn error_message(value: &Value) -> String {
    value["error"]["message"]
        .as_str()
        .or_else(|| value["error"].as_str())
        .or_else(|| value["message"].as_str())
        .unwrap_or("no reason given")
        .to_string()
}

/// Keep an error to one line, and never let a prompt echo into it.
fn short(message: &str) -> String {
    let first = message.lines().next().unwrap_or(message);
    if first.chars().count() > 200 {
        format!("{}…", first.chars().take(200).collect::<String>())
    } else {
        first.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::MemorySecrets;
    use std::collections::BTreeMap;

    #[test]
    fn a_local_provider_only_claims_to_be_local_when_it_is() {
        let p = LocalHttpProvider::default();
        assert!(p.is_loopback());
        assert!(p.info().local);
        assert!(p.info().description.contains("leaves the machine"));

        let remote = LocalHttpProvider { base_url: "http://192.168.1.50:11434/v1".into(), ..Default::default() };
        assert!(!remote.is_loopback());
        assert!(!remote.info().local, "an endpoint on another box is not local");
        assert!(remote.info().description.contains("sent to it"));

        for url in ["http://localhost:1234/v1", "http://[::1]:1234/v1"] {
            assert!(LocalHttpProvider { base_url: url.into(), ..Default::default() }.is_loopback(), "{url}");
        }
    }

    #[test]
    fn anthropic_asks_for_a_key_before_it_asks_for_anything_else() {
        let none = AnthropicProvider { model: "claude".into(), api_key: None };
        let err = none.health().unwrap_err();
        assert!(err.to_string().contains("API key"), "{err}");
        let blank = AnthropicProvider { model: "claude".into(), api_key: Some("   ".into()) };
        assert!(blank.health().is_err(), "whitespace is not a key");

        let store = MemorySecrets(BTreeMap::from([(AnthropicProvider::SECRET_KEY.to_string(), "sk-x".to_string())]));
        let p = AnthropicProvider::from_secrets("claude-x", &store);
        assert!(p.health().is_ok());
        assert!(p.info().requires_credential);
        assert!(!p.info().local, "a hosted provider must never report itself as local");
    }

    /// The first-run state on most machines: the default endpoint is right and
    /// nothing is listening on it. The UI shows a badge and disables the draft
    /// button on the strength of this answer, so it has to be an error rather
    /// than an optimistic success.
    #[test]
    fn a_local_endpoint_with_nothing_listening_is_unreachable() {
        // Port 1 needs privileges to bind, so nothing of the user's is there.
        let p = LocalHttpProvider { base_url: "http://127.0.0.1:1/v1".into(), ..Default::default() };
        let err = p.health().unwrap_err();
        assert!(matches!(err, ProviderError::Unreachable(_)), "{err}");
        let message = err.to_string();
        assert_eq!(message.lines().count(), 1, "one line: {message}");
        assert!(p.info().local, "an unreachable local endpoint is still a local one");
    }

    #[test]
    fn provider_errors_stay_one_line_and_carry_no_body() {
        let long = "a".repeat(400);
        assert_eq!(short(&long).chars().count(), 201, "200 characters plus an ellipsis");
        assert_eq!(short("first line\nsecond line"), "first line");
        assert_eq!(error_message(&json!({"error": {"message": "overloaded"}})), "overloaded");
        assert_eq!(error_message(&json!({"message": "nope"})), "nope");
        assert_eq!(error_message(&json!({})), "no reason given");
    }
}
