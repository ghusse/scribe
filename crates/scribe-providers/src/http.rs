use scribe_core::pipeline::ProviderError;

pub fn map_send_error(e: reqwest::Error) -> ProviderError {
    if e.is_timeout() { ProviderError::Timeout } else { ProviderError::Network(e.to_string()) }
}

pub async fn map_status(resp: reqwest::Response) -> Result<reqwest::Response, ProviderError> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    Err(match status.as_u16() {
        401 | 403 => ProviderError::Auth,
        s => ProviderError::Http { status: s, body: body.chars().take(500).collect() },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn send(client: &reqwest::Client, url: &str) -> Result<reqwest::Response, ProviderError> {
        let resp = client.post(url).send().await.map_err(map_send_error)?;
        map_status(resp).await
    }

    async fn respond(template: ResponseTemplate) -> Result<reqwest::Response, ProviderError> {
        let server = MockServer::start().await;
        Mock::given(method("POST")).respond_with(template).mount(&server).await;
        send(&reqwest::Client::new(), &server.uri()).await
    }

    #[tokio::test]
    async fn a_slow_server_is_a_retryable_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
            .mount(&server)
            .await;
        let client = reqwest::Client::builder().timeout(Duration::from_millis(100)).build().unwrap();
        let err = send(&client, &server.uri()).await.unwrap_err();
        assert_eq!(err, ProviderError::Timeout);
        assert!(err.is_retryable());
    }

    #[tokio::test]
    async fn an_unreachable_server_is_a_retryable_network_error() {
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let err = send(&reqwest::Client::new(), &format!("http://127.0.0.1:{port}/")).await.unwrap_err();
        assert!(matches!(err, ProviderError::Network(_)), "{err:?}");
        assert!(err.is_retryable());
    }

    #[tokio::test]
    async fn success_passes_the_response_through() {
        let resp = respond(ResponseTemplate::new(200).set_body_string("ok")).await.unwrap();
        assert_eq!(resp.text().await.unwrap(), "ok");
    }

    #[tokio::test]
    async fn rejected_keys_are_auth_errors() {
        for status in [401, 403] {
            let err = respond(ResponseTemplate::new(status).set_body_string("invalid key")).await.unwrap_err();
            assert_eq!(err, ProviderError::Auth, "{status}");
            assert!(!err.is_retryable());
        }
    }

    #[tokio::test]
    async fn rate_limits_and_server_errors_are_retryable_client_errors_are_not() {
        for (status, retryable) in [(429, true), (500, true), (502, true), (400, false), (404, false), (413, false)] {
            let err = respond(ResponseTemplate::new(status).set_body_string("détail")).await.unwrap_err();
            assert_eq!(err, ProviderError::Http { status, body: "détail".into() });
            assert_eq!(err.is_retryable(), retryable, "{status}");
        }
    }

    #[tokio::test]
    async fn error_bodies_are_truncated_to_500_characters() {
        let err = respond(ResponseTemplate::new(500).set_body_string("é".repeat(600))).await.unwrap_err();
        assert_eq!(err, ProviderError::Http { status: 500, body: "é".repeat(500) });
    }
}
