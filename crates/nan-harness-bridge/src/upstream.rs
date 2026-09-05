use crate::error::{ApiError, BridgeError};
use crate::timeouts::{
    INITIAL_RESPONSE_TIMEOUT, STREAM_INACTIVITY_TIMEOUT, with_initial_response_timeout,
};
use crate::upstream_capture::record_json;
mod attempt;
mod response;

pub(crate) use attempt::{RetryLease, UpstreamAttempt, classify_attempt};
use nan_harness_coordinator::{
    AttemptOutcome, CaptureLeg, CaptureRequest, CaptureSink, CoordinatorClient, EndpointKind,
    RequestLane, RequestPriority,
};
use nan_harness_core::SecretValue;
use reqwest::header::{ACCEPT, CACHE_CONTROL, CONTENT_TYPE};
pub(crate) use response::{CoordinatedBody, UpstreamResponse};
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

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

#[derive(Clone)]
pub(crate) struct UpstreamCapture {
    handle: Option<CaptureRequest>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestCache {
    Default,
    Bypass,
}

struct SendPolicy<'a> {
    endpoint_kind: EndpointKind,
    model: Option<&'a str>,
    classification: Option<(RequestLane, RequestPriority)>,
    cache: RequestCache,
    budget: Option<&'a mut SendBudget>,
}

pub(crate) struct SendBudget {
    remaining: usize,
}

impl SendBudget {
    pub(crate) const fn new(max_sends: usize) -> Self {
        Self {
            remaining: max_sends,
        }
    }

    pub(crate) const fn is_exhausted(&self) -> bool {
        self.remaining == 0
    }

    fn consume(&mut self) -> Result<(), ApiError> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or_else(|| ApiError::InvalidUpstream("request send budget exhausted".to_owned()))?;
        Ok(())
    }
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
        let capture = self.begin_capture(harness_body);
        let model = body.get("model").and_then(Value::as_str);
        self.send_with_policy(
            &self.chat_endpoint,
            body,
            SendPolicy {
                endpoint_kind: EndpointKind::Inference,
                model,
                classification: None,
                cache: RequestCache::Default,
                budget: None,
            },
            &capture,
        )
        .await
    }

    pub(crate) fn begin_capture(&self, harness_body: &[u8]) -> UpstreamCapture {
        let capture = self.capture.begin_request(self.next_request_id());
        if let Some(capture) = &capture {
            capture.record(CaptureLeg::HarnessRequest, harness_body);
        }
        UpstreamCapture { handle: capture }
    }

    pub(crate) async fn send_with_priority(
        &self,
        body: &Value,
        priority: RequestPriority,
        cache: RequestCache,
        capture: &UpstreamCapture,
        budget: &mut SendBudget,
    ) -> Result<UpstreamResponse, ApiError> {
        let model = body.get("model").and_then(Value::as_str);
        self.send_with_policy(
            &self.chat_endpoint,
            body,
            SendPolicy {
                endpoint_kind: EndpointKind::Inference,
                model,
                classification: Some((RequestLane::Inference, priority)),
                cache,
                budget: Some(budget),
            },
            capture,
        )
        .await
    }

    pub(crate) async fn search(&self, body: &Value) -> Result<UpstreamResponse, ApiError> {
        let harness_body = serde_json::to_vec(body).unwrap_or_default();
        let capture = self.begin_capture(&harness_body);
        self.send_with_policy(
            &self.search_endpoint,
            body,
            SendPolicy {
                endpoint_kind: EndpointKind::Search,
                model: None,
                classification: None,
                cache: RequestCache::Default,
                budget: None,
            },
            &capture,
        )
        .await
    }

    async fn send_with_policy(
        &self,
        endpoint: &str,
        body: &Value,
        policy: SendPolicy<'_>,
        capture: &UpstreamCapture,
    ) -> Result<UpstreamResponse, ApiError> {
        let SendPolicy {
            endpoint_kind,
            model,
            classification,
            cache,
            mut budget,
        } = policy;
        let request_id = self.next_request_id();
        let capture_handle = capture.handle.as_ref();
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
            capture_handle,
            CaptureLeg::ProviderRequest,
            &request_metadata,
        );
        record_json(capture_handle, CaptureLeg::ProviderRequest, body);
        for attempt in 1..=MAX_ATTEMPTS {
            let mut lease = RetryLease::new(match &self.coordinator {
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
            });
            let send_started = Instant::now();
            if let Some(budget) = &mut budget {
                budget.consume()?;
            }
            let result = self.send_to(endpoint, body, cache, &request_id).await;
            if result.is_ok() {
                lease.headers_received(send_started.elapsed()).await;
            }
            let final_attempt =
                attempt == MAX_ATTEMPTS || budget.as_deref().is_some_and(SendBudget::is_exhausted);
            match classify_attempt(result, final_attempt, capture_handle).await {
                UpstreamAttempt::Retry {
                    outcome,
                    retry_after,
                } => {
                    let delay = lease.delay_for_retry(outcome, retry_after, attempt).await;
                    tokio::time::sleep(delay).await;
                }
                UpstreamAttempt::Complete(response) => {
                    if !response.status().is_success() {
                        lease.observe(AttemptOutcome::Terminal).await;
                    }
                    return Ok(UpstreamResponse::new(
                        response,
                        lease.into_inner(),
                        capture.handle.clone(),
                    ));
                }
                UpstreamAttempt::Failed(error) => {
                    lease.observe_error(&error).await;
                    return Err(error);
                }
            }
        }
        unreachable!("bounded retry loop always returns on its final attempt")
    }

    fn next_request_id(&self) -> String {
        format!(
            "request_{}_{}",
            std::process::id(),
            self.next_request_id.fetch_add(1, Ordering::Relaxed)
        )
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

impl UpstreamCapture {
    pub(crate) fn handle(&self) -> Option<CaptureRequest> {
        self.handle.clone()
    }
}
