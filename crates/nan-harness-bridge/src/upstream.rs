use crate::error::{ApiError, BridgeError};
use crate::timeouts::{
    INITIAL_RESPONSE_TIMEOUT, STREAM_INACTIVITY_TIMEOUT, with_initial_response_timeout,
};
use crate::upstream_capture::{record_json, record_payload, record_response_metadata};
use async_stream::stream;
use bytes::Bytes;
use futures_util::{Stream, StreamExt as _};
use nan_harness_coordinator::{
    AttemptOutcome, CaptureLeg, CaptureRequest, CaptureSink, CoordinatorClient, EndpointKind,
    RequestLease, RetryDirective,
};
use nan_harness_core::SecretValue;
use reqwest::header::{ACCEPT, CONTENT_TYPE, RETRY_AFTER};
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

const REQUEST_BUDGET: Duration = Duration::from_secs(45);
const MAX_ATTEMPTS: u8 = 3;

#[derive(Clone)]
pub(crate) struct NanClient {
    client: reqwest::Client,
    chat_endpoint: String,
    search_endpoint: String,
    api_key: Arc<SecretValue>,
    coordinator: Option<CoordinatorClient>,
    capture: CaptureSink,
    next_request_id: Arc<AtomicU64>,
}

pub(crate) struct UpstreamResponse {
    response: reqwest::Response,
    lease: Option<RequestLease>,
    capture: Option<CaptureRequest>,
}

pub(crate) enum UpstreamAttempt {
    Complete(reqwest::Response),
    Retry {
        outcome: AttemptOutcome,
        retry_after: Option<Duration>,
    },
    Failed(ApiError),
}

impl NanClient {
    pub(crate) fn new(
        provider_base_url: &str,
        api_key: Arc<SecretValue>,
        launch_id: &str,
    ) -> Result<Self, BridgeError> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(STREAM_INACTIVITY_TIMEOUT)
            .build()
            .map_err(BridgeError::BuildClient)?;
        let base_url = provider_base_url.trim_end_matches('/');
        let coordinator = CoordinatorClient::new(provider_base_url, &api_key, launch_id);
        Ok(Self {
            client,
            chat_endpoint: format!("{base_url}/chat/completions"),
            search_endpoint: format!("{base_url}/search"),
            api_key,
            coordinator,
            capture: CaptureSink::new(launch_id),
            next_request_id: Arc::new(AtomicU64::new(1)),
        })
    }

    pub(crate) async fn send(
        &self,
        body: &Value,
        harness_body: &[u8],
    ) -> Result<UpstreamResponse, ApiError> {
        let model = body.get("model").and_then(Value::as_str);
        self.send_with_policy(
            &self.chat_endpoint,
            body,
            harness_body,
            EndpointKind::Inference,
            model,
        )
        .await
    }

    pub(crate) async fn search(&self, body: &Value) -> Result<UpstreamResponse, ApiError> {
        let harness_body = serde_json::to_vec(body).unwrap_or_default();
        self.send_with_policy(
            &self.search_endpoint,
            body,
            &harness_body,
            EndpointKind::Search,
            None,
        )
        .await
    }

    async fn send_with_policy(
        &self,
        endpoint: &str,
        body: &Value,
        harness_body: &[u8],
        endpoint_kind: EndpointKind,
        model: Option<&str>,
    ) -> Result<UpstreamResponse, ApiError> {
        let started = Instant::now();
        let request_id = format!(
            "request_{}_{}",
            std::process::id(),
            self.next_request_id.fetch_add(1, Ordering::Relaxed)
        );
        let capture = self.capture.begin_request(request_id);
        if let Some(capture) = &capture {
            capture.record(CaptureLeg::HarnessRequest, harness_body);
        }
        record_json(
            capture.as_ref(),
            CaptureLeg::ProviderRequest,
            &serde_json::json!({
                "method": "POST",
                "url": endpoint,
                "headers": {
                    "accept": "text/event-stream, application/json",
                    "content-type": "application/json",
                    "authorization": "[REDACTED]",
                }
            }),
        );
        record_json(capture.as_ref(), CaptureLeg::ProviderRequest, body);
        for attempt in 1..=MAX_ATTEMPTS {
            let remaining = REQUEST_BUDGET.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(initial_timeout());
            }
            let mut lease = match &self.coordinator {
                Some(coordinator) => coordinator.acquire(endpoint_kind, model, remaining).await,
                None => None,
            };
            if started.elapsed() >= REQUEST_BUDGET {
                return Err(initial_timeout());
            }
            let result = self.send_to(endpoint, body, remaining).await;
            match classify_attempt(result, attempt == MAX_ATTEMPTS, capture.as_ref()).await {
                UpstreamAttempt::Retry {
                    outcome,
                    retry_after,
                } => {
                    let delay = retry_delay(&mut lease, outcome, retry_after, attempt).await;
                    if !sleep_within_budget(delay, started).await {
                        return Err(initial_timeout());
                    }
                }
                UpstreamAttempt::Complete(response) => {
                    if !response.status().is_success()
                        && let Some(lease) = &mut lease
                    {
                        let _ = lease.observe(AttemptOutcome::Terminal, None).await;
                    }
                    return Ok(UpstreamResponse {
                        response,
                        lease,
                        capture,
                    });
                }
                UpstreamAttempt::Failed(error) => {
                    observe_terminal_error(&mut lease, &error).await;
                    return Err(error);
                }
            }
        }
        unreachable!("bounded retry loop always returns on its final attempt")
    }

    async fn send_to(
        &self,
        endpoint: &str,
        body: &Value,
        remaining: Duration,
    ) -> Result<reqwest::Response, ApiError> {
        let request = self.api_key.with_secret(|api_key| {
            self.client
                .post(endpoint)
                .header(CONTENT_TYPE, "application/json")
                .header(ACCEPT, "text/event-stream, application/json")
                .bearer_auth(api_key)
                .json(body)
        });
        with_initial_response_timeout(request.send(), INITIAL_RESPONSE_TIMEOUT.min(remaining)).await
    }
}

pub(crate) async fn classify_attempt(
    result: Result<reqwest::Response, ApiError>,
    final_attempt: bool,
    capture: Option<&CaptureRequest>,
) -> UpstreamAttempt {
    match result {
        Ok(response) => {
            record_response_metadata(capture, &response);
            if retryable_status(response.status()) && !final_attempt {
                let retry_after = retry_after(response.headers());
                let outcome = status_outcome(response.status());
                if let Ok(payload) = response.bytes().await {
                    record_payload(capture, CaptureLeg::ProviderResponse, &payload);
                }
                UpstreamAttempt::Retry {
                    outcome,
                    retry_after,
                }
            } else {
                UpstreamAttempt::Complete(response)
            }
        }
        Err(error) if is_retryable(&error) && !final_attempt => UpstreamAttempt::Retry {
            outcome: error_outcome(&error),
            retry_after: None,
        },
        Err(error) => UpstreamAttempt::Failed(error),
    }
}

const fn status_outcome(status: reqwest::StatusCode) -> AttemptOutcome {
    if status.as_u16() == 429 {
        AttemptOutcome::RateLimited
    } else {
        AttemptOutcome::ServerError
    }
}

const fn error_outcome(error: &ApiError) -> AttemptOutcome {
    if matches!(error, ApiError::UpstreamTimeout(_)) {
        AttemptOutcome::Timeout
    } else {
        AttemptOutcome::Transport
    }
}

impl UpstreamResponse {
    #[cfg(test)]
    pub(crate) fn uncoordinated(response: reqwest::Response) -> Self {
        Self {
            response,
            lease: None,
            capture: None,
        }
    }

    pub(crate) fn status(&self) -> reqwest::StatusCode {
        self.response.status()
    }

    pub(crate) fn content_length(&self) -> Option<u64> {
        self.response.content_length()
    }

    pub(crate) fn capture_handle(&self) -> Option<CaptureRequest> {
        self.capture.clone()
    }

    pub(crate) async fn text(self) -> Result<String, reqwest::Error> {
        let Self {
            response,
            mut lease,
            capture,
        } = self;
        let result = response.text().await;
        if let Ok(text) = &result
            && let Some(capture) = &capture
        {
            capture.record(CaptureLeg::ProviderResponse, text.as_bytes());
        }
        complete_body(&mut lease, result.is_ok()).await;
        result
    }

    pub(crate) async fn bytes(self) -> Result<Bytes, reqwest::Error> {
        let Self {
            response,
            mut lease,
            capture,
        } = self;
        let result = response.bytes().await;
        if let Ok(bytes) = &result
            && let Some(capture) = &capture
        {
            capture.record(CaptureLeg::ProviderResponse, bytes);
        }
        complete_body(&mut lease, result.is_ok()).await;
        result
    }

    pub(crate) async fn chunk(&mut self) -> Result<Option<Bytes>, reqwest::Error> {
        let chunk = match self.response.chunk().await {
            Ok(chunk) => chunk,
            Err(error) => {
                complete_body(&mut self.lease, false).await;
                return Err(error);
            }
        };
        if let Some(bytes) = &chunk {
            if let Some(capture) = &self.capture {
                capture.record(CaptureLeg::ProviderResponse, bytes);
            }
        } else {
            complete_body(&mut self.lease, true).await;
        }
        Ok(chunk)
    }

    pub(crate) fn bytes_stream(
        self,
    ) -> impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static {
        let Self {
            response,
            mut lease,
            capture,
        } = self;
        let source = response.bytes_stream();
        stream! {
            futures_util::pin_mut!(source);
            while let Some(item) = source.next().await {
                if let Ok(bytes) = &item
                    && let Some(capture) = &capture
                {
                    capture.record(CaptureLeg::ProviderResponse, bytes);
                }
                let failed = item.is_err();
                yield item;
                if failed {
                    if let Some(lease) = &mut lease {
                        let _ = lease.observe(AttemptOutcome::Transport, None).await;
                    }
                    return;
                }
            }
            if let Some(lease) = &mut lease {
                let _ = lease.observe(AttemptOutcome::Success, None).await;
            }
        }
    }
}

async fn complete_body(lease: &mut Option<RequestLease>, succeeded: bool) {
    if let Some(lease) = lease {
        let outcome = if succeeded {
            AttemptOutcome::Success
        } else {
            AttemptOutcome::Transport
        };
        let _ = lease.observe(outcome, None).await;
    }
}

async fn observe_terminal_error(lease: &mut Option<RequestLease>, error: &ApiError) {
    if let Some(lease) = lease {
        let outcome = if is_retryable(error) {
            AttemptOutcome::Transport
        } else {
            AttemptOutcome::Terminal
        };
        let _ = lease.observe(outcome, None).await;
    }
}

async fn retry_delay(
    lease: &mut Option<RequestLease>,
    outcome: AttemptOutcome,
    retry_after: Option<Duration>,
    attempt: u8,
) -> Duration {
    if let Some(lease) = lease
        && let RetryDirective::RetryAfter(delay) = lease.observe(outcome, retry_after).await
    {
        return delay;
    }
    retry_after.unwrap_or_else(|| Duration::from_millis(250 * u64::from(attempt)))
}

async fn sleep_within_budget(delay: Duration, started: Instant) -> bool {
    if started.elapsed().saturating_add(delay) >= REQUEST_BUDGET {
        return false;
    }
    tokio::time::sleep(delay).await;
    true
}

pub(crate) fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?;
    value
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
        .or_else(|| {
            httpdate::parse_http_date(value).ok().map(|deadline| {
                deadline
                    .duration_since(SystemTime::now())
                    .unwrap_or_default()
            })
        })
}

pub(crate) fn retryable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 425 | 429 | 500 | 502..=504)
}

fn is_retryable(error: &ApiError) -> bool {
    matches!(
        error,
        ApiError::UpstreamTransport(_) | ApiError::UpstreamTimeout(_)
    )
}

const fn initial_timeout() -> ApiError {
    ApiError::UpstreamTimeout(crate::error::UpstreamTimeoutPhase::InitialResponse)
}

#[cfg(test)]
mod tests {
    use super::retry_after;
    use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
    use std::time::Duration;

    #[test]
    fn retry_after_accepts_delta_seconds_and_http_dates() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("7"));
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(7)));

        headers.insert(
            RETRY_AFTER,
            HeaderValue::from_static("Sun, 06 Nov 1994 08:49:37 GMT"),
        );
        assert_eq!(retry_after(&headers), Some(Duration::ZERO));
    }
}
