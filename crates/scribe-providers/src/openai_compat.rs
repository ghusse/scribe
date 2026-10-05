use std::time::Duration;

use async_trait::async_trait;
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};

use scribe_core::pipeline::{ProviderError, Transcriber};
use scribe_core::prompt::transcriber_prompt;

use crate::http::{map_send_error, map_status};

#[derive(Debug, Clone, Copy, Serialize)]
pub struct SttPreset {
    pub id: &'static str,
    pub label: &'static str,
    pub base_url: &'static str,
    pub default_model: &'static str,
}

pub const STT_PRESETS: &[SttPreset] = &[
    SttPreset { id: "openai", label: "OpenAI", base_url: "https://api.openai.com/v1", default_model: "gpt-4o-transcribe" },
    SttPreset { id: "groq", label: "Groq", base_url: "https://api.groq.com/openai/v1", default_model: "whisper-large-v3-turbo" },
    SttPreset { id: "mistral", label: "Mistral", base_url: "https://api.mistral.ai/v1", default_model: "voxtral-mini-latest" },
];

pub fn stt_preset(id: &str) -> Option<&'static SttPreset> {
    STT_PRESETS.iter().find(|p| p.id == id)
}

/// Any provider exposing OpenAI's `POST /audio/transcriptions` (OpenAI, Groq, Mistral…).
/// The language is never sent: detection stays automatic for mixed French/English dictation.
pub struct OpenAiCompatTranscriber {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAiCompatTranscriber {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder().timeout(timeout).build().map_err(|e| ProviderError::Config(e.to_string()))?;
        Ok(Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            model: model.into(),
        })
    }
}

#[derive(Deserialize)]
struct TranscriptionResponse {
    text: String,
}

#[async_trait]
impl Transcriber for OpenAiCompatTranscriber {
    fn name(&self) -> String {
        self.model.clone()
    }

    async fn transcribe(&self, wav: &[u8], hints: &[String]) -> Result<String, ProviderError> {
        let part = Part::bytes(wav.to_vec())
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| ProviderError::Config(e.to_string()))?;
        let mut form = Form::new().part("file", part).text("model", self.model.clone()).text("response_format", "json");
        if !hints.is_empty() {
            form = form.text("prompt", transcriber_prompt(hints));
        }
        let resp = self
            .client
            .post(format!("{}/audio/transcriptions", self.base_url))
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await
            .map_err(map_send_error)?;
        let resp = map_status(resp).await?;
        let body: TranscriptionResponse = resp.json().await.map_err(|e| ProviderError::Malformed(e.to_string()))?;
        Ok(body.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client(server: &MockServer) -> OpenAiCompatTranscriber {
        OpenAiCompatTranscriber::new(format!("{}/v1/", server.uri()), "k", "gpt-4o-transcribe", Duration::from_secs(5)).unwrap()
    }

    #[tokio::test]
    async fn sends_multipart_with_model_and_hints() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .and(header("authorization", "Bearer k"))
            .and(body_string_contains("gpt-4o-transcribe"))
            .and(body_string_contains("Kubernetes, Tauri"))
            .and(body_string_contains("audio.wav"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": "bonjour"})))
            .expect(1)
            .mount(&server)
            .await;
        let text = client(&server).transcribe(b"RIFF", &["Kubernetes".into(), "Tauri".into()]).await.unwrap();
        assert_eq!(text, "bonjour");
    }

    #[tokio::test]
    async fn omits_prompt_and_language_without_hints() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": "ok"})))
            .mount(&server)
            .await;
        client(&server).transcribe(b"RIFF", &[]).await.unwrap();
        let body = String::from_utf8_lossy(&server.received_requests().await.unwrap()[0].body).to_string();
        assert!(!body.contains("name=\"prompt\""));
        assert!(!body.contains("name=\"language\""));
    }

    #[tokio::test]
    async fn maps_http_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(401)).up_to_n_times(1).mount(&server).await;
        assert_eq!(client(&server).transcribe(b"x", &[]).await, Err(ProviderError::Auth));
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(503).set_body_string("down")).mount(&server).await;
        assert_eq!(
            client(&server).transcribe(b"x", &[]).await,
            Err(ProviderError::Http { status: 503, body: "down".into() })
        );
    }

    #[tokio::test]
    async fn malformed_json_is_reported() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_body_string("nope")).mount(&server).await;
        assert!(matches!(client(&server).transcribe(b"x", &[]).await, Err(ProviderError::Malformed(_))));
    }

    #[test]
    fn presets_are_available() {
        assert_eq!(stt_preset("openai").unwrap().default_model, "gpt-4o-transcribe");
        assert_eq!(stt_preset("groq").unwrap().base_url, "https://api.groq.com/openai/v1");
        assert!(stt_preset("nope").is_none());
    }
}
