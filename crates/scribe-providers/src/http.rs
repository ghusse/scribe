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
