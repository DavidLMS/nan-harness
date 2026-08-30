use crate::auth::is_authorized;
use crate::diagnostics::BridgeDiagnostic;
use crate::error::{ApiError, BridgeError};
use crate::responses::{models, request, search, stream};
use crate::search_http;
use crate::timeouts::map_body_error;
use crate::upstream::NanClient;
use crate::usage::{RequestUsageGuard, SharedUsage};
use crate::{BridgeEndpoint, DiagnosticSender, ResponsesBridgeConfig};
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
    web_search_enabled: bool,
    diagnostics: DiagnosticSender,
    usage: SharedUsage,
}

pub(crate) fn router(
    config: ResponsesBridgeConfig,
    diagnostics: DiagnosticSender,
    usage: SharedUsage,
) -> Result<Router, BridgeError> {
    let state = AppState {
        upstream: NanClient::new(&config.provider_base_url, config.provider_api_key)?,
        models: config.models,
        session_token: config.session_token,
        search_references: Arc::new(search::SearchReferences::default()),
        web_search_enabled: config.web_search_enabled,
        diagnostics,
        usage,
    };
    Ok(Router::new()
        .route("/api/hello", head(hello))
        .route("/v1/models", get(model_catalog))
        .route("/v1/responses", post(responses))
        .route("/v1/alpha/search", post(web_search))
        .route("/v1/search", post(generic_web_search))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(state))
}

async fn generic_web_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<axum::Json<Value>, ApiError> {
    if !state.web_search_enabled {
        return Err(ApiError::SearchDisabled);
    }
    search_http::execute(&headers, &body, &state.upstream, &state.session_token).await
}

async fn hello() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn model_catalog(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<axum::Json<Value>, ApiError> {
    let diagnostics = state.diagnostics.clone();
    let result: Result<axum::Json<Value>, ApiError> = async {
        authorize(&headers, &state)?;
        Ok(axum::Json(state.models.api_response()))
    }
    .await;
    emit_diagnostic(&diagnostics, &result, BridgeEndpoint::Models);
    result
}

async fn responses(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let diagnostics = state.diagnostics.clone();
    let result: Result<Response, ApiError> = async {
        authorize(&headers, &state)?;
        let request: request::ResponsesRequest = serde_json::from_slice(&body)
            .map_err(|error| ApiError::InvalidRequest(format!("invalid JSON body: {error}")))?;
        let model = state.models.resolve(&request.model).ok_or_else(|| {
            ApiError::InvalidRequest(format!(
                "model '{}' is not available for this NaN credential",
                request.model
            ))
        })?;
        let provider_model = model.id.clone();
        let translated = request::translate(request, model)?;
        let upstream = ensure_success(state.upstream.send(&translated.body).await?).await?;
        let usage_guard = RequestUsageGuard::new(&state.usage, provider_model);
        let events = stream::translate(upstream, translated.tools, usage_guard);
        Ok(Sse::new(events)
            .keep_alive(
                KeepAlive::new()
                    .interval(Duration::from_secs(15))
                    .text("ping"),
            )
            .into_response())
    }
    .await;
    emit_diagnostic(&diagnostics, &result, BridgeEndpoint::Responses);
    result
}

async fn web_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<axum::Json<Value>, ApiError> {
    let diagnostics = state.diagnostics.clone();
    let result: Result<axum::Json<Value>, ApiError> = async {
        authorize(&headers, &state)?;
        if !state.web_search_enabled {
            return Err(ApiError::SearchDisabled);
        }
        let request = serde_json::from_slice(&body)
            .map_err(|error| ApiError::InvalidRequest(format!("invalid search JSON: {error}")))?;
        let response = search::execute(&state.upstream, &state.search_references, request).await?;
        Ok(axum::Json(response))
    }
    .await;
    emit_diagnostic(&diagnostics, &result, BridgeEndpoint::Search);
    result
}

fn emit_diagnostic<T>(
    diagnostics: &DiagnosticSender,
    result: &Result<T, ApiError>,
    endpoint: BridgeEndpoint,
) {
    if let Err(error) = result {
        let _ = diagnostics.send(BridgeDiagnostic::from_api_error(error, endpoint));
    }
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
    let body = response.text().await.map_err(map_body_error)?;
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
