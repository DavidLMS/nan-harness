use super::state::AppState;
use super::usage_observer::UsageObserver;
use crate::diagnostics::BridgeDiagnostic;
use crate::error::{ApiError, UpstreamTimeoutPhase};
use crate::timeouts::INITIAL_RESPONSE_TIMEOUT;
use crate::usage::{RequestUsageGuard, SharedUsage};
use crate::{BridgeEndpoint, DiagnosticSender};
use async_stream::stream;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, HeaderName, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use nan_harness_core::is_known_non_coding_model;
use serde_json::Value;

pub(super) const MAX_REQUEST_BYTES: usize = 32 * 1024 * 1024;
const MAX_MODELS_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

pub(super) async fn proxy_with_reqwest_body(
    state: AppState,
    parts: axum::http::request::Parts,
    body: reqwest::Body,
    streaming: bool,
    usage_model_id: Option<String>,
    path: &str,
    filter_model_catalog: bool,
) -> Response {
    let endpoint = format!("{}{path}", state.provider_base_url);
    let endpoint = append_query(endpoint, parts.uri.query());
    let mut builder = state.client.request(parts.method.clone(), endpoint);
    builder = builder.headers(forward_request_headers(&parts.headers));
    builder = state
        .provider_api_key
        .with_secret(|key| builder.bearer_auth(key));
    let response =
        match tokio::time::timeout(INITIAL_RESPONSE_TIMEOUT, builder.body(body).send()).await {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => return upstream_transport_response(error),
            Err(_) => {
                return ApiError::UpstreamTimeout(UpstreamTimeoutPhase::InitialResponse)
                    .into_response();
            }
        };
    if filter_model_catalog {
        response_to_filtered_model_catalog(response).await
    } else {
        response_to_axum(
            response,
            streaming,
            usage_model_id,
            &state.usage,
            &state.diagnostics,
        )
    }
}

async fn response_to_filtered_model_catalog(response: reqwest::Response) -> Response {
    let status = response.status();
    let headers = response.headers().clone();
    let mut source = response.bytes_stream();
    let mut payload = Vec::new();
    while let Some(chunk) = source.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => return upstream_transport_response(error),
        };
        let Some(next_length) = payload.len().checked_add(chunk.len()) else {
            return StatusCode::BAD_GATEWAY.into_response();
        };
        if next_length > MAX_MODELS_RESPONSE_BYTES {
            return StatusCode::BAD_GATEWAY.into_response();
        }
        payload.extend_from_slice(&chunk);
    }

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
                    observer.observe(&chunk);
                    yield Ok::<Bytes, std::io::Error>(chunk);
                }
                Err(error) => {
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
    };
    let mut builder = Response::builder().status(status);
    for (name, value) in &filter_response_headers(&headers) {
        builder = builder.header(name, value);
    }
    builder
        .body(Body::from_stream(body))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
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

pub(super) fn limited_body(
    body: Body,
) -> impl futures_util::Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut body = body.into_data_stream();
        let mut total = 0_usize;
        while let Some(item) = body.next().await {
            let chunk = match item {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Err(std::io::Error::other(error.to_string()));
                    return;
                }
            };
            let Some(next_total) = total.checked_add(chunk.len()) else {
                yield Err(std::io::Error::other("request body is too large"));
                return;
            };
            if next_total > MAX_REQUEST_BYTES {
                yield Err(std::io::Error::other("request body is too large"));
                return;
            }
            total = next_total;
            yield Ok(chunk);
        }
    }
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
mod tests {
    use super::{
        append_query, filter_response_headers, forward_request_headers, request_body_is_empty,
    };
    use axum::http::{HeaderMap, HeaderValue, header};

    #[test]
    fn proxy_header_boundaries_preserve_only_end_to_end_metadata() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer local"),
        );
        headers.insert(header::HOST, HeaderValue::from_static("localhost"));
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("12"));
        headers.insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
        headers.append("x-client-marker", HeaderValue::from_static("one"));
        headers.append("x-client-marker", HeaderValue::from_static("two"));

        let forwarded = forward_request_headers(&headers);

        assert!(!forwarded.contains_key(header::AUTHORIZATION));
        assert!(!forwarded.contains_key(header::HOST));
        assert!(!forwarded.contains_key(header::CONTENT_LENGTH));
        assert!(!forwarded.contains_key(header::CONNECTION));
        assert_eq!(forwarded.get_all("x-client-marker").iter().count(), 2);

        let filtered = filter_response_headers(&headers);
        assert_eq!(filtered[header::AUTHORIZATION], "Bearer local");
        assert_eq!(filtered[header::HOST], "localhost");
        assert!(!filtered.contains_key(header::CONTENT_LENGTH));
        assert!(!filtered.contains_key(header::CONNECTION));
        assert_eq!(filtered.get_all("x-client-marker").iter().count(), 2);
    }

    #[test]
    fn request_body_presence_and_query_forwarding_keep_wire_semantics() {
        assert!(request_body_is_empty(&HeaderMap::new()));

        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("0"));
        assert!(request_body_is_empty(&headers));
        headers.insert(
            header::TRANSFER_ENCODING,
            HeaderValue::from_static("chunked"),
        );
        assert!(!request_body_is_empty(&headers));

        assert_eq!(
            append_query(
                "https://provider.test/models".to_owned(),
                Some("owned=true")
            ),
            "https://provider.test/models?owned=true"
        );
        assert_eq!(
            append_query("https://provider.test/models".to_owned(), None),
            "https://provider.test/models"
        );
    }
}
