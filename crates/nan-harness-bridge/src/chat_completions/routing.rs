use super::ChatCompletionsBridgeConfig;
use super::proxy::{
    MAX_REQUEST_BYTES, limited_body, proxy_with_reqwest_body, request_body_is_empty,
};
use super::request::prepare_chat_body;
use super::state::AppState;
use crate::DiagnosticSender;
use crate::anthropic::{request as anthropic_request, web_search as anthropic_web_search};
use crate::auth::is_authorized;
use crate::error::{ApiError, BridgeError};
use crate::search_http;
use crate::usage::SharedUsage;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, Request};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::Value;

const MODELS_PATH: &str = "/v1/models";
const CHAT_PATH: &str = "/v1/chat/completions";
const MESSAGES_PATH: &str = "/v1/messages";
const UPSTREAM_MODELS_PATH: &str = "/models";
const UPSTREAM_CHAT_PATH: &str = "/chat/completions";

pub(crate) fn router(
    config: ChatCompletionsBridgeConfig,
    diagnostics: DiagnosticSender,
    usage: SharedUsage,
) -> Result<Router, BridgeError> {
    let state = AppState::new(config, diagnostics, usage)?;
    Ok(Router::new()
        .route(MODELS_PATH, get(models))
        .route(CHAT_PATH, post(chat_completions))
        .route(MESSAGES_PATH, post(messages_search))
        .route("/v1/search", post(search))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(state))
}

async fn messages_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    if !is_authorized(&headers, &state.session_token) {
        return Err(ApiError::Unauthorized);
    }
    if !state.web_search_enabled {
        return Err(ApiError::SearchDisabled);
    }
    let request: anthropic_request::MessagesRequest = serde_json::from_slice(&body)
        .map_err(|error| ApiError::InvalidRequest(format!("invalid JSON body: {error}")))?;
    let invocation =
        anthropic_request::web_search_invocation(&request)?.ok_or(ApiError::SearchDisabled)?;
    Ok(anthropic_web_search::execute(&state.search_upstream, invocation, request.model()).await)
}

async fn search(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<axum::Json<Value>, ApiError> {
    if !state.web_search_enabled {
        return Err(ApiError::SearchDisabled);
    }
    search_http::execute(
        &headers,
        &body,
        &state.search_upstream,
        &state.session_token,
    )
    .await
}

async fn models(State(state): State<AppState>, request: Request<Body>) -> Response {
    let (parts, body) = request.into_parts();
    if !is_authorized(&parts.headers, &state.session_token) {
        return ApiError::Unauthorized.into_response();
    }
    let body = if request_body_is_empty(&parts.headers) {
        reqwest::Body::from(Bytes::new())
    } else {
        reqwest::Body::wrap_stream(limited_body(body))
    };
    proxy_with_reqwest_body(state, parts, body, false, None, UPSTREAM_MODELS_PATH, true).await
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
    let prepared = match prepare_chat_body(&body) {
        Ok(prepared) => prepared,
        Err(error) => return error.into_response(),
    };
    let usage_model_id = prepared
        .requested_model_id
        .unwrap_or_else(|| state.fallback_model_id.clone());
    proxy_with_reqwest_body(
        state,
        parts,
        reqwest::Body::from(prepared.body),
        prepared.streaming,
        Some(usage_model_id),
        UPSTREAM_CHAT_PATH,
        false,
    )
    .await
}
