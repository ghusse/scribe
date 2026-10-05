//! Corrector for any provider exposing OpenAI's `POST /chat/completions` (OpenAI, Mistral, Groq…).
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use scribe_core::pipeline::{Corrector, ProviderError};
use scribe_core::prompt::CorrectionPrompt;

use crate::http::{map_send_error, map_status};

pub struct OpenAiChatCorrector {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    /// `reasoning_effort`, only for models that accept it (see `catalog::effort_levels`).
    effort: Option<String>,
}

impl OpenAiChatCorrector {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        effort: Option<String>,
        timeout: Duration,
    ) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder().timeout(timeout).build().map_err(|e| ProviderError::Config(e.to_string()))?;
        Ok(Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            model: model.into(),
            effort,
        })
    }

    pub fn build_body(&self, prompt: &CorrectionPrompt) -> Value {
        let mut body = json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": prompt.system },
                { "role": "user", "content": prompt.user },
            ],
        });
        if let Some(effort) = &self.effort {
            body["reasoning_effort"] = json!(effort);
        }
        body
    }
}

#[async_trait]
impl Corrector for OpenAiChatCorrector {
    fn name(&self) -> String {
        self.model.clone()
    }

    async fn correct(&self, prompt: &CorrectionPrompt) -> Result<String, ProviderError> {
        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&self.build_body(prompt))
            .send()
            .await
            .map_err(map_send_error)?;
        let v: Value = map_status(resp).await?.json().await.map_err(|e| ProviderError::Malformed(e.to_string()))?;
        let choice = &v["choices"][0];
        if choice["finish_reason"] == "content_filter" || choice["message"]["refusal"].is_string() {
            return Err(ProviderError::Refusal);
        }
        match choice["message"]["content"].as_str() {
            Some(text) if !text.trim().is_empty() => Ok(text.to_string()),
            _ => Err(ProviderError::Malformed("aucun texte dans la réponse".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn prompt() -> CorrectionPrompt {
        CorrectionPrompt { system: "SYS".into(), user: "USER".into() }
    }

    fn corrector(server: &MockServer, effort: Option<&str>) -> OpenAiChatCorrector {
        OpenAiChatCorrector::new(format!("{}/v1/", server.uri()), "k", "gpt-6.1-sol", effort.map(String::from), Duration::from_secs(5))
            .unwrap()
    }

    fn reply(content: Value, finish: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{ "message": { "role": "assistant", "content": content }, "finish_reason": finish }]
        }))
    }

    #[tokio::test]
    async fn sends_system_and_user_messages_with_effort() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer k"))
            .respond_with(reply(json!("<output>Bonjour.</output>"), "stop"))
            .expect(1)
            .mount(&server)
            .await;
        let out = corrector(&server, Some("low")).correct(&prompt()).await.unwrap();
        assert_eq!(out, "<output>Bonjour.</output>");
        let body: Value = serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
        assert_eq!(body["model"], "gpt-6.1-sol");
        assert_eq!(body["messages"][0], json!({"role": "system", "content": "SYS"}));
        assert_eq!(body["messages"][1], json!({"role": "user", "content": "USER"}));
        assert_eq!(body["reasoning_effort"], "low");
    }

    #[tokio::test]
    async fn omits_effort_when_unsupported() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).respond_with(reply(json!("<output>x</output>"), "stop")).mount(&server).await;
        corrector(&server, None).correct(&prompt()).await.unwrap();
        let body: Value = serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
        assert!(body.get("reasoning_effort").is_none());
    }

    #[tokio::test]
    async fn maps_refusals_errors_and_empty_answers() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).respond_with(reply(Value::Null, "content_filter")).up_to_n_times(1).mount(&server).await;
        assert_eq!(corrector(&server, None).correct(&prompt()).await, Err(ProviderError::Refusal));
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(401)).up_to_n_times(1).mount(&server).await;
        assert_eq!(corrector(&server, None).correct(&prompt()).await, Err(ProviderError::Auth));
        Mock::given(method("POST")).respond_with(reply(json!("  "), "stop")).mount(&server).await;
        assert!(matches!(corrector(&server, None).correct(&prompt()).await, Err(ProviderError::Malformed(_))));
    }

    #[tokio::test]
    async fn name_is_the_model_and_a_slow_server_times_out() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
            .mount(&server)
            .await;
        let c = OpenAiChatCorrector::new(server.uri(), "k", "gpt-6.1-sol", None, Duration::from_millis(100)).unwrap();
        assert_eq!(c.name(), "gpt-6.1-sol");
        assert_eq!(c.correct(&prompt()).await, Err(ProviderError::Timeout));
    }
}
