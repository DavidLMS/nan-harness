use crate::error::{ApiError, BridgeError};
use crate::timeouts::{
    INITIAL_RESPONSE_TIMEOUT, STREAM_INACTIVITY_TIMEOUT, with_initial_response_timeout,
};
use nan_harness_core::SecretValue;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub(crate) struct NanClient {
    client: reqwest::Client,
    chat_endpoint: String,
    search_endpoint: String,
    api_key: Arc<SecretValue>,
}

impl NanClient {
    pub(crate) fn new(
        provider_base_url: &str,
        api_key: Arc<SecretValue>,
    ) -> Result<Self, BridgeError> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            // `read_timeout` is reset after every body chunk. Together with
            // the explicit initial-response timeout below, this avoids both
            // header hangs and indefinitely stalled response bodies without
            // imposing a total deadline on healthy long-running streams.
            .read_timeout(STREAM_INACTIVITY_TIMEOUT)
            .build()
            .map_err(BridgeError::BuildClient)?;
        let base_url = provider_base_url.trim_end_matches('/');
        Ok(Self {
            client,
            chat_endpoint: format!("{base_url}/chat/completions"),
            search_endpoint: format!("{base_url}/search"),
            api_key,
        })
    }

    /// Sends a chat request to NaN, retrying transient transport failures and
    /// gateway errors before surfacing a transport or timeout error
    /// (`NH-BRIDGE-103`) to the caller.
    pub(crate) async fn send(&self, body: &Value) -> Result<reqwest::Response, ApiError> {
        const RETRY_DELAYS: [Duration; 3] = [
            Duration::from_millis(200),
            Duration::from_millis(500),
            Duration::from_secs(1),
        ];

        for delay in RETRY_DELAYS {
            match self.send_to(&self.chat_endpoint, body).await {
                Ok(response) if is_transient(response.status()) => tokio::time::sleep(delay).await,
                Err(error) if is_retryable(&error) => tokio::time::sleep(delay).await,
                result => return result,
            }
        }
        self.send_to(&self.chat_endpoint, body).await
    }

    pub(crate) async fn search(&self, body: &Value) -> Result<reqwest::Response, ApiError> {
        const RETRY_DELAYS: [Duration; 2] =
            [Duration::from_millis(200), Duration::from_millis(500)];

        for delay in RETRY_DELAYS {
            match self.send_to(&self.search_endpoint, body).await {
                Ok(response) if is_transient(response.status()) => tokio::time::sleep(delay).await,
                Err(error) if is_retryable(&error) => tokio::time::sleep(delay).await,
                result => return result,
            }
        }
        self.send_to(&self.search_endpoint, body).await
    }

    async fn send_to(&self, endpoint: &str, body: &Value) -> Result<reqwest::Response, ApiError> {
        let request = self.api_key.with_secret(|api_key| {
            self.client
                .post(endpoint)
                .header(CONTENT_TYPE, "application/json")
                .header(ACCEPT, "text/event-stream, application/json")
                .bearer_auth(api_key)
                .json(body)
        });
        with_initial_response_timeout(request.send(), INITIAL_RESPONSE_TIMEOUT).await
    }
}

fn is_transient(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 502..=504)
}

fn is_retryable(error: &ApiError) -> bool {
    matches!(
        error,
        ApiError::UpstreamTransport(_) | ApiError::UpstreamTimeout(_)
    )
}
