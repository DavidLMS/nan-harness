use super::state::AppState;
use super::usage_observer::UsageObserver;
use crate::diagnostics::BridgeDiagnostic;
use crate::error::{ApiError, UpstreamTimeoutPhase};
use crate::timeouts::INITIAL_RESPONSE_TIMEOUT;
use crate::upstream::{UpstreamAttempt, classify_attempt};
use crate::upstream_capture::capture_harness_response;
use crate::usage::{RequestUsageGuard, SharedUsage};
use crate::{BridgeEndpoint, DiagnosticSender};
use async_stream::stream;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, HeaderName, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use nan_harness_coordinator::{
    AttemptOutcome, CaptureLeg, CaptureRequest, EndpointKind, RequestLease, RetryDirective,
};
use nan_harness_core::is_known_non_coding_model;
use serde_json::Value;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

pub(super) const MAX_REQUEST_BYTES: usize = 32 * 1024 * 1024;
const MAX_MODELS_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const COORDINATOR_WAIT_BUDGET: Duration = Duration::from_secs(90);
const MAX_ATTEMPTS: u8 = 3;

pub(super) struct ProxyOptions<'a> {
    pub(super) path: &'a str,
    pub(super) filter_model_catalog: bool,
}

pub(super) async fn proxy_with_reqwest_body(
    state: AppState,
    parts: axum::http::request::Parts,
    harness_body: Bytes,
    provider_body: Bytes,
    streaming: bool,
    usage_model_id: Option<String>,
    options: ProxyOptions<'_>,
) -> Response {
    let endpoint = format!("{}{}", state.provider_base_url, options.path);
    let endpoint = append_query(endpoint, parts.uri.query());
    let request_id = format!(
        "request_{}_{}",
        std::process::id(),
        state.next_request_id.fetch_add(1, Ordering::Relaxed)
    );
    let capture = state.capture.begin_request(request_id);
    record_request_metadata(capture.as_ref(), &parts, &endpoint);
    record_capture(capture.as_ref(), CaptureLeg::HarnessRequest, &harness_body);
    record_capture(
        capture.as_ref(),
        CaptureLeg::ProviderRequest,
        &provider_body,
    );
    let request = ProxyRequest {
        endpoint,
        method: parts.method,
        headers: forward_request_headers(&parts.headers),
        body: provider_body,
        endpoint_kind: if options.filter_model_catalog {
            EndpointKind::Models
        } else {
            EndpointKind::Inference
        },
        model: usage_model_id.as_deref(),
    };
    let (response, lease) = match send_with_policy(&state, &request, capture.as_ref()).await {
        Ok(result) => result,
        Err(error) => return capture_harness_response(error.into_response(), capture),
    };
    let response_capture = capture.clone();
    let response = if options.filter_model_catalog {
        response_to_filtered_model_catalog(response, lease, capture).await
    } else {
        response_to_axum(
            response,
            lease,
            capture,
            streaming,
            usage_model_id,
            &state.usage,
            &state.diagnostics,
        )
    };
    capture_harness_response(response, response_capture)
}

struct ProxyRequest<'a> {
    endpoint: String,
    method: axum::http::Method,
    headers: HeaderMap,
    body: Bytes,
    endpoint_kind: EndpointKind,
    model: Option<&'a str>,
}

async fn send_with_policy(
    state: &AppState,
    request: &ProxyRequest<'_>,
    capture: Option<&CaptureRequest>,
) -> Result<(reqwest::Response, Option<RequestLease>), ApiError> {
    for attempt in 1..=MAX_ATTEMPTS {
        let mut lease = match &state.coordinator {
            Some(coordinator) => coordinator
                .acquire(
                    request.endpoint_kind,
                    request.model,
                    COORDINATOR_WAIT_BUDGET,
                )
                .await
                .map_err(ApiError::from)?,
            None => None,
        };
        let send_started = Instant::now();
        let result = send_attempt(state, request).await;
        if result.is_ok()
            && let Some(lease) = &mut lease
        {
            lease.headers_received(send_started.elapsed()).await;
        }
        match classify_attempt(result, attempt == MAX_ATTEMPTS, capture).await {
            UpstreamAttempt::Retry {
                outcome,
                retry_after,
            } => {
                let delay = retry_delay(&mut lease, outcome, retry_after, attempt).await;
                tokio::time::sleep(delay).await;
            }
            UpstreamAttempt::Complete(response) => return Ok((response, lease)),
            UpstreamAttempt::Failed(error) => {
                let outcome = if matches!(error, ApiError::UpstreamTimeout(_)) {
                    AttemptOutcome::Timeout
                } else {
                    AttemptOutcome::Transport
                };
                observe(&mut lease, outcome).await;
                return Err(error);
            }
        }
    }
    unreachable!("bounded retry loop always returns on its final attempt")
}

async fn send_attempt(
    state: &AppState,
    request: &ProxyRequest<'_>,
) -> Result<reqwest::Response, ApiError> {
    let mut builder = state
        .client
        .request(request.method.clone(), &request.endpoint)
        .headers(request.headers.clone());
    builder = state
        .provider_api_key
        .with_secret(|key| builder.bearer_auth(key));
    match tokio::time::timeout(
        INITIAL_RESPONSE_TIMEOUT,
        builder.body(request.body.clone()).send(),
    )
    .await
    {
        Ok(result) => result.map_err(ApiError::UpstreamTransport),
        Err(_) => Err(initial_timeout()),
    }
}

async fn response_to_filtered_model_catalog(
    response: reqwest::Response,
    mut lease: Option<RequestLease>,
    capture: Option<CaptureRequest>,
) -> Response {
    let status = response.status();
    let headers = response.headers().clone();
    let mut source = response.bytes_stream();
    let mut payload = Vec::new();
    while let Some(chunk) = source.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                observe(&mut lease, AttemptOutcome::Transport).await;
                return upstream_transport_response(error);
            }
        };
        record_capture(capture.as_ref(), CaptureLeg::ProviderResponse, &chunk);
        let Some(next_length) = payload.len().checked_add(chunk.len()) else {
            return StatusCode::BAD_GATEWAY.into_response();
        };
        if next_length > MAX_MODELS_RESPONSE_BYTES {
            return StatusCode::BAD_GATEWAY.into_response();
        }
        payload.extend_from_slice(&chunk);
    }
    let outcome = if status.is_success() {
        AttemptOutcome::Success
    } else {
        AttemptOutcome::Terminal
    };
    observe(&mut lease, outcome).await;

    if status.is_success()
        && let Ok(mut catalog) = serde_json::from_slice::<Value>(&payload)
        && let Some(models) = catalog.get_mut("data").and_then(Value::as_array_mut)
    {
        models.retain(|model| {
            model
                .get("id")
                .and_then(Value::as_str)
                .is_none_or(|model_id| !is_known_non_coding_model(model_id))
        });
        if let Ok(filtered) = serde_json::to_vec(&catalog) {
            payload = filtered;
        }
    }
    let mut builder = Response::builder().status(status);
    for (name, value) in &filter_response_headers(&headers) {
        builder = builder.header(name, value);
    }
    builder
        .body(Body::from(payload))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}

fn response_to_axum(
    response: reqwest::Response,
    mut lease: Option<RequestLease>,
    capture: Option<CaptureRequest>,
    streaming: bool,
    usage_model_id: Option<String>,
    usage: &SharedUsage,
    diagnostics: &DiagnosticSender,
) -> Response {
    let status = response.status();
    let headers = response.headers().clone();
    let source = response.bytes_stream();
    let usage = usage.clone();
    let diagnostics = diagnostics.clone();
    let guard = usage_model_id
        .filter(|_| status.is_success())
        .map(|model_id| RequestUsageGuard::new(&usage, model_id));
    let body = stream! {
        let mut observer = UsageObserver::new(streaming, guard);
        futures_util::pin_mut!(source);
        while let Some(item) = source.next().await {
            match item {
                Ok(chunk) => {
                    record_capture(capture.as_ref(), CaptureLeg::ProviderResponse, &chunk);
                    observer.observe(&chunk);
                    yield Ok::<Bytes, std::io::Error>(chunk);
                }
                Err(error) => {
                    observe(&mut lease, AttemptOutcome::Transport).await;
                    let _ = diagnostics.send(BridgeDiagnostic::from_api_error(
                        &ApiError::UpstreamTransport(error),
                        BridgeEndpoint::Messages,
                    ));
                    yield Err(std::io::Error::other("upstream response body failed"));
                    return;
                }
            }
        }
        observer.finish();
        let outcome = if status.is_success() {
            AttemptOutcome::Success
        } else {
            AttemptOutcome::Terminal
        };
        observe(&mut lease, outcome).await;
    };
    let mut builder = Response::builder().status(status);
    for (name, value) in &filter_response_headers(&headers) {
        builder = builder.header(name, value);
    }
    builder
        .body(Body::from_stream(body))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}

fn record_capture(capture: Option<&CaptureRequest>, leg: CaptureLeg, payload: &[u8]) {
    if let Some(capture) = capture {
        capture.record(leg, payload);
    }
}

fn record_request_metadata(
    capture: Option<&CaptureRequest>,
    parts: &axum::http::request::Parts,
    provider_url: &str,
) {
    let Some(capture) = capture else {
        return;
    };
    let harness_headers = header_values(&parts.headers);
    let harness = serde_json::json!({
        "method": parts.method.as_str(),
        "uri": parts.uri.to_string(),
        "headers": harness_headers,
    });
    if let Ok(payload) = serde_json::to_vec(&harness) {
        capture.record(CaptureLeg::HarnessRequest, &payload);
    }
    let provider = serde_json::json!({
        "method": parts.method.as_str(),
        "url": provider_url,
        "headers": {
            "authorization": "[REDACTED]",
        },
    });
    if let Ok(payload) = serde_json::to_vec(&provider) {
        capture.record(CaptureLeg::ProviderRequest, &payload);
    }
}

fn header_values(headers: &HeaderMap) -> serde_json::Map<String, Value> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_owned(), Value::String(value.to_owned())))
        })
        .collect()
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

async fn observe(lease: &mut Option<RequestLease>, outcome: AttemptOutcome) {
    if let Some(lease) = lease {
        let _ = lease.observe(outcome, None).await;
    }
}

const fn initial_timeout() -> ApiError {
    ApiError::UpstreamTimeout(UpstreamTimeoutPhase::InitialResponse)
}

fn upstream_transport_response(error: reqwest::Error) -> Response {
    ApiError::UpstreamTransport(error).into_response()
}

pub(super) fn request_body_is_empty(headers: &HeaderMap) -> bool {
    if headers.contains_key(header::TRANSFER_ENCODING) {
        return false;
    }
    headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|value| value == "0")
}

fn forward_request_headers(headers: &HeaderMap) -> HeaderMap {
    let mut result = HeaderMap::new();
    for (name, value) in headers {
        if is_hop_by_hop(name) || *name == header::AUTHORIZATION || *name == header::HOST {
            continue;
        }
        if *name == header::CONTENT_LENGTH {
            continue;
        }
        result.append(name.clone(), value.clone());
    }
    result
}

fn filter_response_headers(headers: &HeaderMap) -> HeaderMap {
    let mut result = HeaderMap::new();
    for (name, value) in headers {
        if is_hop_by_hop(name) || *name == header::CONTENT_LENGTH {
            continue;
        }
        result.append(name.clone(), value.clone());
    }
    result
}

fn is_hop_by_hop(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn append_query(mut endpoint: String, query: Option<&str>) -> String {
    if let Some(query) = query {
        endpoint.push('?');
        endpoint.push_str(query);
    }
    endpoint
}

#[cfg(test)]
mod tests;
