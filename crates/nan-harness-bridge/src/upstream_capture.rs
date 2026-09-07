use async_stream::stream;
use axum::body::Body;
use axum::response::Response;
use futures_util::StreamExt as _;
use nan_harness_coordinator::{CaptureLeg, CaptureRequest};
use serde_json::Value;
use std::time::Duration;

#[cfg(test)]
mod tests;

// Retry bodies are diagnostic-only. This keeps useful ordinary provider errors
// without letting an intermediate response compete with inference payloads.
const RETRY_RESPONSE_BODY_LIMIT: usize = 64 * 1024;
// An absolute deadline is required because reqwest's read timeout resets when
// a body keeps making progress.
const RETRY_RESPONSE_BODY_TIMEOUT: Duration = Duration::from_secs(2);

struct IncompleteCapture<'a> {
    capture: Option<&'a CaptureRequest>,
}

impl IncompleteCapture<'_> {
    fn complete(&mut self) {
        self.capture = None;
    }
}

impl Drop for IncompleteCapture<'_> {
    fn drop(&mut self) {
        if let Some(capture) = self.capture {
            capture.mark_incomplete();
        }
    }
}

pub(crate) fn capture_harness_response(
    response: Response,
    capture: Option<CaptureRequest>,
) -> Response {
    let Some(capture) = capture else {
        return response;
    };
    record_http_metadata(
        &capture,
        CaptureLeg::HarnessResponse,
        response.status(),
        response.headers(),
    );
    let (parts, body) = response.into_parts();
    let source = body.into_data_stream();
    let body = stream! {
        futures_util::pin_mut!(source);
        while let Some(item) = source.next().await {
            if let Ok(bytes) = &item {
                capture.record(CaptureLeg::HarnessResponse, bytes);
            }
            yield item;
        }
    };
    Response::from_parts(parts, Body::from_stream(body))
}

pub(crate) fn record_json(capture: Option<&CaptureRequest>, leg: CaptureLeg, body: &Value) {
    if let Some(capture) = capture
        && let Ok(payload) = serde_json::to_vec(body)
    {
        capture.record(leg, &payload);
    }
}

pub(crate) async fn handle_retry_response_body(
    capture: Option<&CaptureRequest>,
    response: reqwest::Response,
) {
    let Some(capture) = capture else {
        return;
    };
    let mut incomplete = IncompleteCapture {
        capture: Some(capture),
    };
    let result = tokio::time::timeout(
        RETRY_RESPONSE_BODY_TIMEOUT,
        read_bounded_retry_response(response),
    )
    .await;
    if let Ok(Some(payload)) = result {
        capture.record(CaptureLeg::ProviderResponse, &payload);
        incomplete.complete();
    }
}

async fn read_bounded_retry_response(mut response: reqwest::Response) -> Option<Vec<u8>> {
    let capacity = match response.content_length() {
        Some(length) => {
            let length = usize::try_from(length).ok()?;
            (length <= RETRY_RESPONSE_BODY_LIMIT).then_some(length)?
        }
        None => 0,
    };

    let mut payload = Vec::with_capacity(capacity);
    while let Some(chunk) = response.chunk().await.ok()? {
        if chunk.len() > RETRY_RESPONSE_BODY_LIMIT.saturating_sub(payload.len()) {
            return None;
        }
        payload.extend_from_slice(&chunk);
    }
    Some(payload)
}

pub(crate) fn record_response_metadata(
    capture: Option<&CaptureRequest>,
    response: &reqwest::Response,
) {
    if let Some(capture) = capture {
        record_http_metadata(
            capture,
            CaptureLeg::ProviderResponse,
            response.status(),
            response.headers(),
        );
    }
}

fn record_http_metadata(
    capture: &CaptureRequest,
    leg: CaptureLeg,
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
) {
    let metadata = serde_json::json!({
        "status": status.as_u16(),
        "headers": header_map_json(headers),
    });
    if let Ok(payload) = serde_json::to_vec(&metadata) {
        capture.record(leg, &payload);
    }
}

fn header_map_json(headers: &reqwest::header::HeaderMap) -> serde_json::Map<String, Value> {
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
