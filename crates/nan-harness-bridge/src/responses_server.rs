use crate::ResponsesBridgeConfig;
use crate::auth::is_authorized;
use crate::error::{ApiError, BridgeError};
use crate::responses::{models, request, search, stream};
use crate::upstream::NanClient;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, head, post};
use nan_harness_core::SecretValue;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

const MAX_REQUEST_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone)]
struct AppState {
    upstream: NanClient,
    models: models::CodexModelCatalog,
    session_token: Arc<SecretValue>,
    search_references: Arc<search::SearchReferences>,
}

pub(crate) fn router(config: ResponsesBridgeConfig) -> Result<Router, BridgeError> {
    let state = AppState {
        upstream: NanClient::new(&config.provider_base_url, config.provider_api_key)?,
        models: config.models,
        session_token: config.session_token,
        search_references: Arc::new(search::SearchReferences::default()),
    };
    Ok(Router::new()
        .route("/api/hello", head(hello))
        .route("/v1/models", get(model_catalog))
        .route("/v1/responses", post(responses))
        .route("/v1/alpha/search", post(web_search))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(state))
}

async fn hello() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn model_catalog(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<axum::Json<Value>, ApiError> {
    authorize(&headers, &state)?;
    Ok(axum::Json(state.models.api_response()))
}

async fn responses(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    authorize(&headers, &state)?;
    let request: request::ResponsesRequest = serde_json::from_slice(&body)
        .map_err(|error| ApiError::InvalidRequest(format!("invalid JSON body: {error}")))?;
    let model = state.models.resolve(&request.model).ok_or_else(|| {
        ApiError::InvalidRequest(format!(
            "model '{}' is not available for this NaN credential",
            request.model
        ))
    })?;
    let translated = request::translate(request, &model.id, model.max_output_tokens)?;
    let upstream = ensure_success(state.upstream.send(&translated.body).await?).await?;
    let events = stream::translate(upstream, translated.tools);
    Ok(Sse::new(events)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("ping"),
        )
        .into_response())
}

async fn web_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<axum::Json<Value>, ApiError> {
    authorize(&headers, &state)?;
    let request = serde_json::from_slice(&body)
        .map_err(|error| ApiError::InvalidRequest(format!("invalid search JSON: {error}")))?;
    let response = search::execute(&state.upstream, &state.search_references, request).await?;
    Ok(axum::Json(response))
}

fn authorize(headers: &HeaderMap, state: &AppState) -> Result<(), ApiError> {
    if is_authorized(headers, &state.session_token) {
        Ok(())
    } else {
        Err(ApiError::Unauthorized)
    }
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, ApiError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let message = parsed
        .pointer("/error/message")
        .or_else(|| parsed.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("NaN request failed")
        .replace(['\r', '\n'], " ")
        .chars()
        .take(300)
        .collect();
    Err(ApiError::UpstreamStatus { status, message })
}
