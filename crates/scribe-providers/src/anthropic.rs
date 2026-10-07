//! Anthropic Messages API corrector (raw HTTP: there is no official Rust SDK).
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use scribe_core::pipeline::{Corrector, ProviderError};
use scribe_core::prompt::CorrectionPrompt;

use crate::http::{map_send_error, map_status, shared_client};

const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

pub struct AnthropicCorrector {
    client: reqwest::Client,
    /// Per request: the client is shared by every provider.
    timeout: Duration,
    base_url: String,
    api_key: String,
    model: String,
    /// Sent only when set and the model accepts it.
    effort: Option<String>,
}

fn supports_effort(model: &str) -> bool {
    !model.starts_with("claude-haiku") && !model.starts_with("claude-3")
}

fn supports_server_fallback(model: &str) -> bool {
    ["claude-opus-5", "claude-sonnet-5-5", "claude-fable-5"].iter().any(|p| model.starts_with(p))
}

impl AnthropicCorrector {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        effort: Option<String>,
        timeout: Duration,
    ) -> Result<Self, ProviderError> {
        Ok(Self {
            client: shared_client()?,
            timeout,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            model: model.into(),
            effort,
        })
    }

    pub fn build_body(&self, prompt: &CorrectionPrompt) -> Value {
        let mut body = json!({
            "model": self.model,
            "max_tokens": 8192,
            "system": [{ "type": "text", "text": prompt.system, "cache_control": { "type": "ephemeral" } }],
            "messages": [{ "role": "user", "content": prompt.user }],
        });
        if let Some(effort) = self.effort.as_deref().filter(|_| supports_effort(&self.model)) {
            body["output_config"] = json!({ "effort": effort });
        }
        if supports_server_fallback(&self.model) {
            body["fallbacks"] = json!("default");
        }
        body
    }
}

#[async_trait]
impl Corrector for AnthropicCorrector {
    fn name(&self) -> String {
        self.model.clone()
    }

    async fn correct(&self, prompt: &CorrectionPrompt) -> Result<String, ProviderError> {
        let mut req = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .timeout(self.timeout)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&self.build_body(prompt));
        if supports_server_fallback(&self.model) {
            req = req.header("anthropic-beta", FALLBACK_BETA);
        }
        let resp = map_status(req.send().await.map_err(map_send_error)?).await?;
        let v: Value = resp.json().await.map_err(|e| ProviderError::Malformed(e.to_string()))?;
        if v["stop_reason"] == "refusal" {
            return Err(ProviderError::Refusal);
        }
        let text: String = v["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        if text.trim().is_empty() {
            return Err(ProviderError::Malformed("aucun bloc texte dans la réponse".into()));
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn prompt() -> CorrectionPrompt {
        CorrectionPrompt { system: "SYS".into(), user: "USER".into() }
    }

    fn corrector(server: &MockServer, model: &str) -> AnthropicCorrector {
        AnthropicCorrector::new(server.uri(), "k", model, Some("low".into()), Duration::from_secs(5)).unwrap()
    }

    async fn last_body(server: &MockServer) -> serde_json::Value {
        let reqs = server.received_requests().await.unwrap();
        serde_json::from_slice(&reqs.last().unwrap().body).unwrap()
    }

    #[tokio::test]
    async fn sends_cached_system_and_returns_text_blocks_only() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "k"))
            .and(header("anthropic-version", "2023-06-01"))
            .and(header("anthropic-beta", "server-side-fallback-2026-07-01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "thinking", "thinking": ""}, {"type": "text", "text": "<output>Bonjour.</output>"}],
                "stop_reason": "end_turn"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let out = corrector(&server, "claude-opus-5-5").correct(&prompt()).await.unwrap();
        assert_eq!(out, "<output>Bonjour.</output>");
        let body = last_body(&server).await;
        assert_eq!(body["model"], "claude-opus-5-5");
        assert_eq!(body["system"][0]["text"], "SYS");
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(body["messages"][0], json!({"role": "user", "content": "USER"}));
        assert_eq!(body["output_config"]["effort"], "low");
        assert_eq!(body["fallbacks"], "default");
        assert!(body.get("thinking").is_none());
    }

    #[tokio::test]
    async fn haiku_gets_no_effort_and_no_fallbacks() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "text", "text": "<output>x</output>"}], "stop_reason": "end_turn"
            })))
            .mount(&server)
            .await;
        corrector(&server, "claude-haiku-4-5").correct(&prompt()).await.unwrap();
        let body = last_body(&server).await;
        assert!(body.get("output_config").is_none());
        assert!(body.get("fallbacks").is_none());
        let req = &server.received_requests().await.unwrap()[0];
        assert!(req.headers.get("anthropic-beta").is_none());
    }

    #[tokio::test]
    async fn refusal_and_errors_are_mapped() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"content": [], "stop_reason": "refusal"})))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        assert_eq!(corrector(&server, "claude-opus-5-5").correct(&prompt()).await, Err(ProviderError::Refusal));
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(401)).up_to_n_times(1).mount(&server).await;
        assert_eq!(corrector(&server, "claude-opus-5-5").correct(&prompt()).await, Err(ProviderError::Auth));
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"content": [{"type": "thinking", "thinking": ""}], "stop_reason": "end_turn"})))
            .mount(&server)
            .await;
        assert!(matches!(corrector(&server, "claude-opus-5-5").correct(&prompt()).await, Err(ProviderError::Malformed(_))));
    }

    #[tokio::test]
    async fn name_is_the_model_and_a_slow_server_times_out() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
            .mount(&server)
            .await;
        let c = AnthropicCorrector::new(server.uri(), "k", "claude-haiku-4-5", None, Duration::from_millis(100)).unwrap();
        assert_eq!(c.name(), "claude-haiku-4-5");
        assert_eq!(c.correct(&prompt()).await, Err(ProviderError::Timeout));
    }
}
