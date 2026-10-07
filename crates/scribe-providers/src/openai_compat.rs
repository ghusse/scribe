use std::time::Duration;

use async_trait::async_trait;
use reqwest::multipart::{Form, Part};
use serde::Deserialize;

use scribe_core::pipeline::{ProviderError, Transcriber};
use scribe_core::prompt::transcriber_prompt;

use crate::http::{map_send_error, map_status, shared_client};

/// Any provider exposing OpenAI's `POST /audio/transcriptions` (OpenAI, Groq, Mistral…).
/// The language is never sent: detection stays automatic for mixed French/English dictation.
pub struct OpenAiCompatTranscriber {
    client: reqwest::Client,
    /// Per request: the client is shared by every provider.
    timeout: Duration,
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
        Ok(Self {
            client: shared_client()?,
            timeout,
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
            .timeout(self.timeout)
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


    #[tokio::test]
    async fn name_is_the_model_and_a_slow_server_times_out() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
            .mount(&server)
            .await;
        let t = OpenAiCompatTranscriber::new(server.uri(), "k", "whisper-large-v3", Duration::from_millis(100)).unwrap();
        assert_eq!(t.name(), "whisper-large-v3");
        assert_eq!(t.transcribe(b"RIFF", &[]).await, Err(ProviderError::Timeout));
    }
}
