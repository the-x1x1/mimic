//! A deterministic provider used by tests and by the evaluation harness.
//!
//! It is not a stub that returns a fixed string: it echoes back a reply built
//! from the prompt it was given, which is what makes it useful. A test can
//! assert that the prompt assembler actually put the examples, the intent and
//! the length target where it claimed to, by reading them out of the answer.

use std::sync::Mutex;

use super::{GenerationRequest, GenerationResponse, ModelProvider, ProviderError, ProviderInfo, ProviderResult};

pub struct MockProvider {
    id: String,
    local: bool,
    /// Every request this provider has seen, so a test can inspect the prompt.
    pub seen: Mutex<Vec<GenerationRequest>>,
    /// When set, `generate` returns this instead of the echo.
    pub canned: Mutex<Option<String>>,
    /// When set, `generate` and `health` fail with it.
    pub fail_with: Mutex<Option<String>>,
}

impl Default for MockProvider {
    fn default() -> Self {
        Self::named("mock", true)
    }
}

impl MockProvider {
    pub fn named(id: &str, local: bool) -> Self {
        Self {
            id: id.to_string(),
            local,
            seen: Mutex::new(Vec::new()),
            canned: Mutex::new(None),
            fail_with: Mutex::new(None),
        }
    }

    pub fn answering(text: &str) -> Self {
        let p = Self::default();
        *p.canned.lock().unwrap() = Some(text.to_string());
        p
    }

    pub fn failing(message: &str) -> Self {
        let p = Self::default();
        *p.fail_with.lock().unwrap() = Some(message.to_string());
        p
    }

    pub fn last_request(&self) -> Option<GenerationRequest> {
        self.seen.lock().unwrap().last().cloned()
    }

    pub fn call_count(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

impl ModelProvider for MockProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: self.id.clone(),
            display_name: "Test provider".into(),
            local: self.local,
            model: "mock-1".into(),
            description: "A deterministic provider used by Mimic's own tests. Never shown in the app.".into(),
            requires_credential: false,
        }
    }

    fn generate(&self, request: &GenerationRequest) -> ProviderResult<GenerationResponse> {
        if let Some(msg) = self.fail_with.lock().unwrap().clone() {
            return Err(ProviderError::Unreachable(msg));
        }
        self.seen.lock().unwrap().push(request.clone());
        let text = match self.canned.lock().unwrap().clone() {
            Some(t) => t,
            None => request
                .messages
                .last()
                .map(|m| format!("echo: {}", m.content.lines().next().unwrap_or_default()))
                .unwrap_or_else(|| "echo:".into()),
        };
        Ok(GenerationResponse {
            text,
            provider: self.id.clone(),
            model: "mock-1".into(),
            input_tokens: None,
            output_tokens: None,
        })
    }

    fn health(&self) -> ProviderResult<()> {
        match self.fail_with.lock().unwrap().clone() {
            Some(msg) => Err(ProviderError::Unreachable(msg)),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::PromptMessage;
    use super::*;

    fn request(last: &str) -> GenerationRequest {
        GenerationRequest {
            system: "be yourself".into(),
            messages: vec![PromptMessage::user(last)],
            max_output_tokens: 256,
            temperature: 0.7,
        }
    }

    #[test]
    fn the_mock_records_what_it_was_asked_and_echoes_it_back() {
        let p = MockProvider::default();
        assert_eq!(p.call_count(), 0);
        let r = p.generate(&request("say hello\nand more")).unwrap();
        assert_eq!(r.text, "echo: say hello");
        assert_eq!(p.call_count(), 1);
        assert_eq!(p.last_request().unwrap().system, "be yourself");
    }

    #[test]
    fn a_canned_answer_replaces_the_echo_and_a_failure_replaces_both() {
        let p = MockProvider::answering("Tuesday works.");
        assert_eq!(p.generate(&request("x")).unwrap().text, "Tuesday works.");
        let p = MockProvider::failing("connection refused");
        assert!(p.health().is_err());
        assert!(p.generate(&request("x")).is_err());
        assert_eq!(p.call_count(), 0, "a failed call records nothing");
    }
}
