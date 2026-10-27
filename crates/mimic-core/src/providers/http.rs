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
//!
//! A model that thinks before it answers (qwen3 through Ollama, say) is asked
//! not to (`reasoning_effort: "none"`, 0.10.0-alpha.84): through Ollama its
//! thinking otherwise spends the whole of a short reply's length, and the
//! reply comes back empty (measured on the owner's desktop, job 308: qwen3:14b,
//! 120 tokens, every one of them thinking). An endpoint that turns the field
//! down is asked again without it, and thinking written into the reply itself
//! (`<think>…</think>`) is taken out.

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

    /// One request to `/chat/completions`: its status, and what came back.
    fn post(&self, body: &Value) -> ProviderResult<(reqwest::StatusCode, Value)> {
        let resp = client()?
            .post(format!("{}/chat/completions", self.base_url.trim_end_matches('/')))
            .json(body)
            .send()
            .map_err(|e| ProviderError::Unreachable(short(&e.to_string())))?;
        let status = resp.status();
        let value: Value = resp.json().map_err(|e| ProviderError::Malformed(short(&e.to_string())))?;
        Ok((status, value))
    }
}

/// Asked of every local endpoint: answer without thinking first. Ollama
/// takes it for every model, thinking or not (job 308).
const NO_THINKING: &str = "none";

/// Said when a model thought and wrote no reply.
pub const THOUGHT_INSTEAD: &str =
    "the model spent its whole answer thinking and wrote no reply. A model that doesn't think first, such as llama3.2:3b, suits drafting better";

/// An endpoint that refuses `reasoning_effort` — OpenAI's for a model that
/// doesn't reason, or a server that knows no such field — says so in a 400
/// or 422 that names it, in its message or, as FastAPI does, in where the
/// error is (`loc`).
fn turned_down_thinking(status: reqwest::StatusCode, value: &Value) -> bool {
    (status.as_u16() == 400 || status.as_u16() == 422) && value.to_string().to_lowercase().contains("reasoning")
}

/// A reply with any thinking written into it taken out: `<think>…</think>`
/// and `<thinking>…</thinking>` wherever they are, and — when the reply
/// begins with one that is never closed — all of it. A tag never closed
/// further in is left as written: it may be the reply's own words.
pub fn without_thinking(text: &str) -> String {
    let mut rest = text;
    let mut out = String::new();
    loop {
        // The tags are ASCII, and ASCII lowercasing keeps every byte where
        // it was, so offsets in it are offsets in the reply (0.10.0-alpha.87;
        // full lowercasing moves them for "İ" and kept the thinking in).
        let lower = rest.to_ascii_lowercase();
        let found = ["<think>", "<thinking>"].iter().filter_map(|t| lower.find(t).map(|at| (at, *t))).min();
        let Some((at, tag)) = found else {
            out.push_str(rest);
            break;
        };
        let close = format!("</{}", &tag[1..]);
        match lower[at..].find(&close) {
            Some(end) => {
                out.push_str(&rest[..at]);
                rest = &rest[at + end + close.len()..];
            }
            None if out.trim().is_empty() && rest[..at].trim().is_empty() => break,
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    out.trim().to_string()
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
        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "max_tokens": request.max_output_tokens,
            "temperature": request.temperature,
            "stream": false,
            "reasoning_effort": NO_THINKING,
        });
        let (mut status, mut value) = self.post(&body)?;
        if turned_down_thinking(status, &value) {
            if let Some(fields) = body.as_object_mut() {
                fields.remove("reasoning_effort");
            }
            (status, value) = self.post(&body)?;
        }
        if !status.is_success() {
            return Err(ProviderError::Refused(format!("{} {}", status.as_u16(), error_message(&value))));
        }
        let message = &value["choices"][0]["message"];
        if !message.is_object() {
            return Err(ProviderError::Malformed("no message content in the response".into()));
        }
        // A model that only thought may send `null` for its words
        // (0.10.0-alpha.87): read as none, and said so below.
        let said = message["content"].as_str().unwrap_or_default();
        let text = without_thinking(said);
        if text.is_empty() {
            let thought = ["reasoning", "reasoning_content"]
                .iter()
                .any(|k| message[*k].as_str().is_some_and(|r| !r.trim().is_empty()))
                || said.contains("<think");
            return Err(ProviderError::Malformed(if thought {
                THOUGHT_INSTEAD.to_string()
            } else {
                "an empty reply".to_string()
            }));
        }
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
        .or_else(|| value["detail"].as_str())
        .or_else(|| value["detail"][0]["msg"].as_str())
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
    use crate::providers::{MemorySecrets, PromptMessage};
    use std::collections::BTreeMap;
    use std::io::{Read, Write};

    /// An endpoint on this computer that answers each request in turn with
    /// the next of `answers` (status, JSON) and hands over the body it was
    /// sent.
    fn endpoint(answers: Vec<(u16, Value)>) -> (LocalHttpProvider, std::sync::mpsc::Receiver<Value>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
        let (sent, bodies) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for (status, answer) in answers {
                let Ok((mut stream, _)) = listener.accept() else { return };
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                    head.push(byte[0]);
                }
                let length = String::from_utf8_lossy(&head)
                    .lines()
                    .find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().to_string()))
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(0);
                let mut body = vec![0u8; length];
                let _ = stream.read_exact(&mut body);
                let _ = sent.send(serde_json::from_slice(&body).unwrap_or(Value::Null));
                let answer = answer.to_string();
                let reply = format!(
                    "HTTP/1.1 {status} Answer\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                );
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        (LocalHttpProvider { base_url, model: "qwen3:14b".into() }, bodies)
    }

    fn said(content: &str, extra: Value) -> Value {
        let mut message = json!({"role": "assistant", "content": content});
        if let (Some(m), Some(e)) = (message.as_object_mut(), extra.as_object()) {
            m.extend(e.clone());
        }
        json!({"choices": [{"message": message, "finish_reason": "stop"}], "usage": {"prompt_tokens": 10, "completion_tokens": 3}})
    }

    fn ask() -> GenerationRequest {
        GenerationRequest {
            system: "draft it".into(),
            messages: vec![PromptMessage::user("you around later?")],
            max_output_tokens: 64,
            temperature: 0.7,
        }
    }

    #[test]
    fn a_local_model_is_asked_not_to_think_first() {
        let (p, bodies) = endpoint(vec![(200, said("yes from 6", json!({})))]);
        assert_eq!(p.generate(&ask()).unwrap().text, "yes from 6");
        let body = bodies.recv().unwrap();
        assert_eq!(body["reasoning_effort"], "none");
        assert_eq!(body["max_tokens"], 64);
    }

    #[test]
    fn an_endpoint_that_turns_the_field_down_is_asked_again_without_it() {
        let refused = json!({"error": {"message": "Unrecognized request argument supplied: reasoning_effort"}});
        let (p, bodies) = endpoint(vec![(400, refused), (200, said("yes from 6", json!({})))]);
        assert_eq!(p.generate(&ask()).unwrap().text, "yes from 6");
        assert_eq!(bodies.recv().unwrap()["reasoning_effort"], "none");
        let again = bodies.recv().unwrap();
        assert!(again.get("reasoning_effort").is_none(), "{again}");
        assert_eq!(again["messages"][1]["content"], "you around later?");

        // FastAPI names the field where the error is, not in its message.
        let extra = json!({"detail": [{"type": "extra_forbidden", "loc": ["body", "reasoning_effort"], "msg": "Extra inputs are not permitted"}]});
        let (p, bodies) = endpoint(vec![(422, extra), (200, said("yes from 6", json!({})))]);
        assert_eq!(p.generate(&ask()).unwrap().text, "yes from 6");
        bodies.recv().unwrap();
        assert!(bodies.recv().unwrap().get("reasoning_effort").is_none());

        // Any other refusal is the endpoint's answer, and is not asked again.
        let (p, bodies) = endpoint(vec![(400, json!({"error": {"message": "model not found"}}))]);
        let err = p.generate(&ask()).unwrap_err();
        assert!(matches!(err, ProviderError::Refused(_)), "{err}");
        assert!(err.to_string().contains("model not found"), "{err}");
        bodies.recv().unwrap();
        assert!(bodies.recv_timeout(std::time::Duration::from_millis(200)).is_err(), "asked once");
    }

    #[test]
    fn thinking_written_into_a_reply_is_taken_out() {
        let (p, _) = endpoint(vec![(200, said("<think>\nthey asked about later\n</think>\n\nyes from 6", json!({})))]);
        assert_eq!(p.generate(&ask()).unwrap().text, "yes from 6");

        assert_eq!(without_thinking("yes <THINK>hm</THINK>from 6"), "yes from 6");
        assert_eq!(without_thinking("<thinking>a</thinking>ok<think>b</think>!"), "ok!");
        assert_eq!(without_thinking("<think>never closed, all of it thinking"), "");
        assert_eq!(without_thinking("I think <b>so</b>"), "I think <b>so</b>");
        // Letters whose lowercase is longer don't keep the thinking in.
        assert_eq!(without_thinking("<think>\nhm\n</think>\n\nTamam, İstanbul'da 6'da"), "Tamam, İstanbul'da 6'da");
        // A tag never closed, further in, is the reply's own.
        assert_eq!(
            without_thinking("it puts <think> in front of what it says"),
            "it puts <think> in front of what it says"
        );
    }

    #[test]
    fn a_model_that_only_thought_says_so_rather_than_giving_an_empty_draft() {
        // What qwen3:14b through Ollama gave on the owner's desktop when
        // allowed to think: no words, its length spent on reasoning.
        let thought = json!({"choices": [{"message": {"role": "assistant", "content": "", "reasoning": "Okay, the user wants..."}, "finish_reason": "length"}]});
        let (p, _) = endpoint(vec![(200, thought)]);
        let err = p.generate(&ask()).unwrap_err();
        assert!(matches!(err, ProviderError::Malformed(_)), "{err}");
        assert!(err.to_string().contains(THOUGHT_INSTEAD), "{err}");

        let (p, _) = endpoint(vec![(200, said("   ", json!({})))]);
        let err = p.generate(&ask()).unwrap_err();
        assert!(err.to_string().contains("an empty reply"), "{err}");

        // vLLM's shape: no words at all, and the thinking beside them.
        let null = json!({"choices": [{"message": {"role": "assistant", "content": null, "reasoning_content": "hm"}, "finish_reason": "length"}]});
        let (p, _) = endpoint(vec![(200, null)]);
        let err = p.generate(&ask()).unwrap_err();
        assert!(err.to_string().contains(THOUGHT_INSTEAD), "{err}");
    }

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
        assert_eq!(
            error_message(&json!({"detail": "reasoning_effort is not allowed"})),
            "reasoning_effort is not allowed"
        );
        assert_eq!(
            error_message(&json!({"detail": [{"msg": "extra inputs are not permitted"}]})),
            "extra inputs are not permitted"
        );
        assert_eq!(error_message(&json!({})), "no reason given");
    }
}
