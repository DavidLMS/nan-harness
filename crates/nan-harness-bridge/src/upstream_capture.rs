use async_stream::stream;
use axum::body::Body;
use axum::response::Response;
use futures_util::StreamExt as _;
use nan_harness_coordinator::{CaptureLeg, CaptureRequest};
use serde_json::Value;

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

pub(crate) fn record_payload(capture: Option<&CaptureRequest>, leg: CaptureLeg, payload: &[u8]) {
    if let Some(capture) = capture {
        capture.record(leg, payload);
    }
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
