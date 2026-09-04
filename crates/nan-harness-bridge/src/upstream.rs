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
    RequestLane, RequestLease, RequestPriority, RetryDirective,
};
use nan_harness_core::SecretValue;
use reqwest::header::{ACCEPT, CACHE_CONTROL, CONTENT_TYPE, RETRY_AFTER};
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

const COORDINATOR_WAIT_BUDGET: Duration = Duration::from_hours(1);
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

pub(crate) struct CoordinatedBody {
    source: std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    lease: Option<RequestLease>,
    capture: Option<CaptureRequest>,
    finished: Option<RetryDirective>,
}

pub(crate) enum UpstreamAttempt {
    Complete(reqwest::Response),
    Retry {
        outcome: AttemptOutcome,
        retry_after: Option<Duration>,
    },
    Failed(ApiError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestCache {
    Default,
    Bypass,
}

#[derive(Clone, Copy)]
struct SendPolicy<'a> {
    endpoint_kind: EndpointKind,
    model: Option<&'a str>,
    classification: Option<(RequestLane, RequestPriority)>,
    cache: RequestCache,
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
        let coordinator = CoordinatorClient::try_new(provider_base_url, &api_key, launch_id)?;
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
            SendPolicy {
                endpoint_kind: EndpointKind::Inference,
                model,
                classification: None,
                cache: RequestCache::Default,
            },
        )
        .await
    }

    pub(crate) async fn send_with_priority(
        &self,
        body: &Value,
        harness_body: &[u8],
        priority: RequestPriority,
        cache: RequestCache,
    ) -> Result<UpstreamResponse, ApiError> {
        let model = body.get("model").and_then(Value::as_str);
        self.send_with_policy(
            &self.chat_endpoint,
            body,
            harness_body,
            SendPolicy {
                endpoint_kind: EndpointKind::Inference,
                model,
                classification: Some((RequestLane::Inference, priority)),
                cache,
            },
        )
        .await
    }

    pub(crate) async fn search(&self, body: &Value) -> Result<UpstreamResponse, ApiError> {
        let harness_body = serde_json::to_vec(body).unwrap_or_default();
        self.send_with_policy(
            &self.search_endpoint,
            body,
            &harness_body,
            SendPolicy {
                endpoint_kind: EndpointKind::Search,
                model: None,
                classification: None,
                cache: RequestCache::Default,
            },
        )
        .await
    }

    async fn send_with_policy(
        &self,
        endpoint: &str,
        body: &Value,
        harness_body: &[u8],
        policy: SendPolicy<'_>,
    ) -> Result<UpstreamResponse, ApiError> {
        let SendPolicy {
            endpoint_kind,
            model,
            classification,
            cache,
        } = policy;
        let request_id = format!(
            "request_{}_{}",
            std::process::id(),
            self.next_request_id.fetch_add(1, Ordering::Relaxed)
        );
        let capture = self.capture.begin_request(request_id.clone());
        if let Some(capture) = &capture {
            capture.record(CaptureLeg::HarnessRequest, harness_body);
        }
        let mut request_metadata = serde_json::json!({
            "method": "POST",
            "url": endpoint,
            "headers": {
                "accept": "text/event-stream, application/json",
                "content-type": "application/json",
                "authorization": "[REDACTED]",
            }
        });
        if cache == RequestCache::Bypass {
            request_metadata["headers"]["cache-control"] = Value::from("no-cache");
            request_metadata["headers"]["x-request-id"] = Value::from("[GENERATED]");
        }
        record_json(
            capture.as_ref(),
            CaptureLeg::ProviderRequest,
            &request_metadata,
        );
        record_json(capture.as_ref(), CaptureLeg::ProviderRequest, body);
        for attempt in 1..=MAX_ATTEMPTS {
            let mut lease = match &self.coordinator {
                Some(coordinator) => match classification {
                    Some((lane, priority)) => {
                        coordinator
                            .acquire_classified(
                                endpoint_kind,
                                model,
                                lane,
                                priority,
                                COORDINATOR_WAIT_BUDGET,
                            )
                            .await
                    }
                    None => {
                        coordinator
                            .acquire(endpoint_kind, model, COORDINATOR_WAIT_BUDGET)
                            .await
                    }
                }
                .map_err(ApiError::from)?,
                None => None,
            };
            let send_started = Instant::now();
            let result = self.send_to(endpoint, body, cache, &request_id).await;
            if result.is_ok()
                && let Some(lease) = &mut lease
            {
                lease.headers_received(send_started.elapsed()).await;
            }
            match classify_attempt(result, attempt == MAX_ATTEMPTS, capture.as_ref()).await {
                UpstreamAttempt::Retry {
                    outcome,
                    retry_after,
                } => {
                    let delay = retry_delay(&mut lease, outcome, retry_after, attempt).await;
                    tokio::time::sleep(delay).await;
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
        cache: RequestCache,
        request_id: &str,
    ) -> Result<reqwest::Response, ApiError> {
        let request = self.api_key.with_secret(|api_key| {
            let request = self
                .client
                .post(endpoint)
                .header(CONTENT_TYPE, "application/json")
                .header(ACCEPT, "text/event-stream, application/json")
                .bearer_auth(api_key)
                .json(body);
            match cache {
                RequestCache::Default => request,
                RequestCache::Bypass => request
                    .header(CACHE_CONTROL, "no-cache")
                    .header("x-request-id", request_id),
            }
        });
        with_initial_response_timeout(request.send(), INITIAL_RESPONSE_TIMEOUT).await
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
            let mut terminal_tail = Vec::new();
            while let Some(item) = source.next().await {
                if let Ok(bytes) = &item
                    && let Some(capture) = &capture
                {
                    capture.record(CaptureLeg::ProviderResponse, bytes);
                }
                let failed = item.is_err();
                let terminal = item.as_ref().is_ok_and(|bytes| {
                    terminal_tail.extend_from_slice(bytes);
                    let found = terminal_tail
                        .windows(b"data: [DONE]".len())
                        .any(|window| window == b"data: [DONE]");
                    if terminal_tail.len() > 32 {
                        let keep_from = terminal_tail.len() - 32;
                        terminal_tail.drain(..keep_from);
                    }
                    found
                });
                if failed {
                    if let Some(lease) = &mut lease {
                        let _ = lease.observe(AttemptOutcome::Transport, None).await;
                    }
                    yield item;
                    return;
                }
                if terminal
                    && let Some(lease) = &mut lease
                {
                    let _ = lease.observe(AttemptOutcome::Success, None).await;
                }
                yield item;
            }
            if let Some(lease) = &mut lease {
                let _ = lease.observe(AttemptOutcome::InvalidResponse, None).await;
            }
        }
    }

    pub(crate) fn into_coordinated_body(self) -> CoordinatedBody {
        let Self {
            response,
            lease,
            capture,
        } = self;
        CoordinatedBody {
            source: Box::pin(response.bytes_stream()),
            lease,
            capture,
            finished: None,
        }
    }
}

impl CoordinatedBody {
    pub(crate) async fn next(&mut self) -> Result<Option<Bytes>, ApiError> {
        let Ok(item) = tokio::time::timeout(STREAM_INACTIVITY_TIMEOUT, self.source.next()).await
        else {
            self.finish(AttemptOutcome::Timeout).await;
            return Err(ApiError::UpstreamTimeout(
                crate::error::UpstreamTimeoutPhase::Inactivity,
            ));
        };
        match item {
            Some(Ok(bytes)) => {
                if let Some(capture) = &self.capture {
                    capture.record(CaptureLeg::ProviderResponse, &bytes);
                }
                Ok(Some(bytes))
            }
            Some(Err(error)) => {
                self.finish(AttemptOutcome::Transport).await;
                Err(crate::timeouts::map_body_error(error))
            }
            None => Ok(None),
        }
    }

    pub(crate) async fn finish(&mut self, outcome: AttemptOutcome) -> RetryDirective {
        if let Some(directive) = self.finished {
            return directive;
        }
        let directive = match &mut self.lease {
            Some(lease) => lease.observe(outcome, None).await,
            None => RetryDirective::Complete,
        };
        self.finished = Some(directive);
        directive
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
