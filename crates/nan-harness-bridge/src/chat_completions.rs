use crate::auth::is_authorized;
use crate::diagnostics::BridgeDiagnostic;
use crate::error::{ApiError, BridgeError};
use crate::timeouts::{INITIAL_RESPONSE_TIMEOUT, STREAM_INACTIVITY_TIMEOUT};
use crate::{BridgeEndpoint, DiagnosticSender};
use async_stream::stream;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, HeaderName, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use futures_util::StreamExt;
use nan_harness_core::SecretValue;
use reqwest::Client;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAX_REQUEST_BYTES: usize = 32 * 1024 * 1024;
const MODELS_PATH: &str = "/v1/models";
const CHAT_PATH: &str = "/v1/chat/completions";
const UPSTREAM_MODELS_PATH: &str = "/models";
const UPSTREAM_CHAT_PATH: &str = "/chat/completions";

pub type SharedUsage = Arc<Mutex<ChatUsageSnapshot>>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatUsageSnapshot {
    pub completed_requests: u64,
    pub responses_with_usage: u64,
    pub responses_without_usage: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub reasoning_tokens: u64,
}

#[derive(Debug)]
pub struct ChatCompletionsBridgeConfig {
    pub provider_base_url: String,
    pub provider_api_key: Arc<SecretValue>,
    pub session_token: Arc<SecretValue>,
}

#[derive(Clone)]
struct AppState {
    client: Client,
    provider_base_url: String,
    provider_api_key: Arc<SecretValue>,
    session_token: Arc<SecretValue>,
    usage: SharedUsage,
    diagnostics: DiagnosticSender,
}

pub(crate) fn router(
    config: ChatCompletionsBridgeConfig,
    diagnostics: DiagnosticSender,
    usage: SharedUsage,
) -> Result<Router, BridgeError> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(STREAM_INACTIVITY_TIMEOUT)
        .build()
        .map_err(BridgeError::BuildClient)?;
    Ok(Router::new()
        .route(MODELS_PATH, get(models))
        .route(CHAT_PATH, post(chat_completions))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(AppState {
            client,
            provider_base_url: config.provider_base_url.trim_end_matches('/').to_owned(),
            provider_api_key: config.provider_api_key,
            session_token: config.session_token,
            usage,
            diagnostics,
        }))
}

pub(crate) fn new_usage() -> SharedUsage {
    Arc::new(Mutex::new(ChatUsageSnapshot::default()))
}

pub(crate) fn snapshot(usage: &SharedUsage) -> ChatUsageSnapshot {
    usage
        .lock()
        .expect("chat usage mutex should not be poisoned")
        .clone()
}

async fn models(State(state): State<AppState>, request: Request<Body>) -> Response {
    proxy(state, request, false, false, UPSTREAM_MODELS_PATH).await
}

async fn chat_completions(State(state): State<AppState>, request: Request<Body>) -> Response {
    let (parts, body) = request.into_parts();
    if !is_authorized(&parts.headers, &state.session_token) {
        return ApiError::Unauthorized.into_response();
    }

    let body = match axum::body::to_bytes(body, MAX_REQUEST_BYTES).await {
        Ok(body) => body,
        Err(error) => {
            return ApiError::InvalidRequest(format!("could not read request body: {error}"))
                .into_response();
        }
    };
    let (body, streaming) = match prepare_chat_body(&body) {
        Ok(prepared) => prepared,
        Err(error) => return error.into_response(),
    };
    let request = Request::from_parts(parts, Body::from(body));
    proxy_with_body(state, request, streaming, true, UPSTREAM_CHAT_PATH).await
}

async fn proxy(
    state: AppState,
    request: Request<Body>,
    streaming: bool,
    observe_usage: bool,
    path: &str,
) -> Response {
    let (parts, body) = request.into_parts();
    let body = match axum::body::to_bytes(body, MAX_REQUEST_BYTES).await {
        Ok(body) => body,
        Err(error) => {
            return ApiError::InvalidRequest(format!("could not read request body: {error}"))
                .into_response();
        }
    };
    let request = Request::from_parts(parts, Body::from(body));
    proxy_with_body(state, request, streaming, observe_usage, path).await
}

async fn proxy_with_body(
    state: AppState,
    request: Request<Body>,
    streaming: bool,
    observe_usage: bool,
    path: &str,
) -> Response {
    let (parts, body) = request.into_parts();
    if !is_authorized(&parts.headers, &state.session_token) {
        return ApiError::Unauthorized.into_response();
    }
    let body = match axum::body::to_bytes(body, MAX_REQUEST_BYTES).await {
        Ok(body) => body,
        Err(error) => {
            return ApiError::InvalidRequest(format!("could not read request body: {error}"))
                .into_response();
        }
    };
    let endpoint = format!("{}{path}", state.provider_base_url);
    let endpoint = append_query(endpoint, parts.uri.query());
    let mut builder = state.client.request(parts.method.clone(), endpoint);
    builder = builder.headers(forward_request_headers(&parts.headers));
    builder = state
        .provider_api_key
        .with_secret(|key| builder.bearer_auth(key));
    let response = match tokio::time::timeout(INITIAL_RESPONSE_TIMEOUT, builder.body(body).send())
        .await
    {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => return upstream_transport_response(error),
        Err(_) => {
            return ApiError::UpstreamTimeout(crate::error::UpstreamTimeoutPhase::InitialResponse)
                .into_response();
        }
    };
    response_to_axum(
        response,
        streaming,
        observe_usage,
        &state.usage,
        &state.diagnostics,
    )
}

fn prepare_chat_body(body: &[u8]) -> Result<(Bytes, bool), ApiError> {
    let mut value: Value = serde_json::from_slice(body)
        .map_err(|error| ApiError::InvalidRequest(format!("invalid JSON body: {error}")))?;
    let streaming = value
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !streaming {
        return Ok((Bytes::copy_from_slice(body), false));
    }
    if streaming {
        let options = value
            .as_object_mut()
            .ok_or_else(|| {
                ApiError::InvalidRequest("request body must be a JSON object".to_owned())
            })?
            .entry("stream_options")
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        let options = options.as_object_mut().ok_or_else(|| {
            ApiError::InvalidRequest("stream_options must be a JSON object".to_owned())
        })?;
        options.insert("include_usage".to_owned(), Value::Bool(true));
    }
    serde_json::to_vec(&value)
        .map(|body| (Bytes::from(body), streaming))
        .map_err(|error| ApiError::InvalidRequest(format!("could not encode JSON body: {error}")))
}

fn response_to_axum(
    response: reqwest::Response,
    streaming: bool,
    observe_usage: bool,
    usage: &SharedUsage,
    diagnostics: &DiagnosticSender,
) -> Response {
    let status = response.status();
    let headers = response.headers().clone();
    let source = response.bytes_stream();
    let usage = usage.clone();
    let diagnostics = diagnostics.clone();
    let body = stream! {
        let mut observer = UsageObserver::new(streaming, observe_usage);
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
        observer.finish(&usage);
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

#[derive(Debug, Default)]
struct UsageObserver {
    streaming: bool,
    observe_usage: bool,
    buffer: Vec<u8>,
    usage: Option<UsageValues>,
}

#[derive(Debug, Clone, Copy)]
struct UsageValues {
    prompt: u64,
    completion: u64,
    reasoning: u64,
}

impl UsageObserver {
    fn new(streaming: bool, observe_usage: bool) -> Self {
        Self {
            streaming,
            observe_usage,
            ..Self::default()
        }
    }

    fn observe(&mut self, chunk: &[u8]) {
        if !self.observe_usage {
            return;
        }
        self.buffer.extend_from_slice(chunk);
        if self.streaming {
            self.observe_sse_lines();
        }
    }

    fn observe_sse_lines(&mut self) {
        while let Some(index) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line = self.buffer.drain(..=index).collect::<Vec<_>>();
            let line = line.strip_suffix(b"\n").unwrap_or(&line);
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let Some(data) = line.strip_prefix(b"data:") else {
                continue;
            };
            let data = data.strip_prefix(b" ").unwrap_or(data);
            if let Ok(value) = serde_json::from_slice::<Value>(data)
                && let Some(usage) = parse_usage(&value)
            {
                self.usage = Some(usage);
            }
        }
        if self.buffer.len() > MAX_REQUEST_BYTES {
            self.buffer.clear();
        }
    }

    fn finish(&mut self, shared: &SharedUsage) {
        if !self.observe_usage {
            return;
        }
        if !self.streaming
            && let Ok(value) = serde_json::from_slice::<Value>(&self.buffer)
        {
            self.usage = parse_usage(&value);
        }
        let mut state = shared
            .lock()
            .expect("chat usage mutex should not be poisoned");
        state.completed_requests += 1;
        if let Some(usage) = self.usage {
            state.responses_with_usage += 1;
            state.prompt_tokens = state.prompt_tokens.saturating_add(usage.prompt);
            state.completion_tokens = state.completion_tokens.saturating_add(usage.completion);
            state.reasoning_tokens = state.reasoning_tokens.saturating_add(usage.reasoning);
        } else {
            state.responses_without_usage += 1;
        }
    }
}

fn parse_usage(value: &Value) -> Option<UsageValues> {
    let usage = value.get("usage")?.as_object()?;
    let prompt_tokens = usage.get("prompt_tokens")?.as_u64()?;
    let completion_tokens = usage.get("completion_tokens")?.as_u64()?;
    let reasoning_tokens = usage
        .get("completion_tokens_details")
        .and_then(Value::as_object)
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(UsageValues {
        prompt: prompt_tokens,
        completion: completion_tokens,
        reasoning: reasoning_tokens,
    })
}
