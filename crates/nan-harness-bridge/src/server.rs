use crate::BridgeConfig;
use crate::anthropic::{request, response, stream, web_search};
use crate::auth::is_authorized;
use crate::error::{ApiError, BridgeError};
use crate::upstream::NanClient;
use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, head, post};
use nan_harness_core::SecretValue;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

const MAX_REQUEST_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone)]
struct AppState {
    upstream: NanClient,
    models: crate::ClaudeModelCatalog,
    session_token: Arc<SecretValue>,
}

pub(crate) fn router(config: BridgeConfig) -> Result<Router, BridgeError> {
    let state = AppState {
        upstream: NanClient::new(&config.provider_base_url, config.provider_api_key)?,
        models: config.models,
        session_token: config.session_token,
    };
    Ok(Router::new()
        .route("/api/hello", head(hello))
        .route("/v1/models", get(models))
        .route("/v1/messages", post(messages))
        .route("/v1/messages/count_tokens", post(count_tokens))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(state))
}

async fn hello() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::models::AnthropicModelsResponse>, ApiError> {
    authorize(&headers, &state)?;
    Ok(Json(state.models.api_response()))
}

async fn messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    authorize(&headers, &state)?;
    let request = parse_request(&body)?;
    let client_model = request.model().to_owned();
    let model = resolve_model(&state, &client_model)?;
    let provider_model = model.provider_id().to_owned();
    let response_model = model.gateway_id().to_owned();
    let max_output_tokens = model.max_output_tokens();
    let reasoning = model.reasoning();
    if let Some(invocation) = request::web_search_invocation(&request)? {
        return Ok(web_search::execute(&state.upstream, invocation, &client_model).await);
    }
    let translated = request::translate(request, &provider_model, max_output_tokens, reasoning)?;
    let upstream = ensure_success(state.upstream.send(&translated.body).await?).await?;

    if translated.stream {
        let events = stream::translate(upstream, response_model);
        Ok(Sse::new(events)
            .keep_alive(
                KeepAlive::new()
                    .interval(Duration::from_secs(15))
                    .text("ping"),
            )
            .into_response())
    } else {
        let value = upstream
            .json::<Value>()
            .await
            .map_err(|error| ApiError::InvalidUpstream(error.to_string()))?;
        Ok(Json(response::translate(value, &response_model)?).into_response())
    }
}

async fn count_tokens(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    authorize(&headers, &state)?;
    let request = parse_request(&body)?;
    resolve_model(&state, request.model())?;
    Ok(Json(
        json!({"input_tokens": request::estimate_input_tokens(&request)}),
    ))
}

fn resolve_model<'a>(
    state: &'a AppState,
    gateway_id: &str,
) -> Result<&'a crate::ClaudeModel, ApiError> {
    state.models.resolve(gateway_id).ok_or_else(|| {
        ApiError::InvalidRequest(format!(
            "model '{gateway_id}' is not available through this bridge"
        ))
    })
}

fn authorize(headers: &HeaderMap, state: &AppState) -> Result<(), ApiError> {
    if is_authorized(headers, &state.session_token) {
        Ok(())
    } else {
        Err(ApiError::Unauthorized)
    }
}

fn parse_request(body: &[u8]) -> Result<request::MessagesRequest, ApiError> {
    serde_json::from_slice(body)
        .map_err(|error| ApiError::InvalidRequest(format!("invalid JSON body: {error}")))
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, ApiError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let message = response.text().await.map_or_else(
        |_| "NaN request failed".to_owned(),
        |body| sanitize_upstream_error(&body),
    );
    Err(ApiError::UpstreamStatus { status, message })
}

fn sanitize_upstream_error(body: &str) -> String {
    let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let raw = parsed
        .pointer("/error/message")
        .or_else(|| parsed.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("NaN request failed");
    raw.replace(['\r', '\n'], " ").chars().take(300).collect()
}
