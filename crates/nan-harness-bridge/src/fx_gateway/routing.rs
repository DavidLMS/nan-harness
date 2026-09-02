use super::request::{is_permission_review, latest_user_text, provider_search_tool, translate};
use super::state::{AppState, FxGatewayConfig};
use super::stream;
use crate::auth::is_authorized;
use crate::diagnostics::BridgeDiagnostic;
use crate::error::{ApiError, BridgeError};
use crate::search_http;
use crate::timeouts::map_body_error;
use crate::usage::{RequestUsageGuard, SharedUsage};
use crate::{BridgeEndpoint, DiagnosticSender};
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::HeaderMap;
use axum::response::sse::{KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::Value;

const MAX_REQUEST_BYTES: usize = 32 * 1024 * 1024;
const CHAT_PATH: &str = "/v3/ai/language-model";
const MODELS_PATH: &str = "/coding-agent/v1/models";

pub(crate) fn router(
    config: FxGatewayConfig,
    diagnostics: DiagnosticSender,
    usage: SharedUsage,
) -> Result<Router, BridgeError> {
    let state = AppState::new(config, diagnostics, usage)?;
    Ok(Router::new()
        .route(MODELS_PATH, get(models))
        .route(CHAT_PATH, post(chat))
        .route("/v1/search", post(search))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(state))
}

async fn search(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<axum::Json<Value>, ApiError> {
    if !state.web_search_enabled {
        return Err(ApiError::SearchDisabled);
    }
    search_http::execute(&headers, &body, &state.upstream, &state.session_token).await
}

async fn models(
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

async fn chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let diagnostics = state.diagnostics.clone();
    let result: Result<Response, ApiError> = async {
        authorize(&headers, &state)?;
        let model_id = headers
            .get("ai-language-model-id")
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| ApiError::InvalidRequest("fx did not provide a model ID".to_owned()))?;
        let request: Value = serde_json::from_slice(&body)
            .map_err(|error| ApiError::InvalidRequest(format!("invalid fx JSON body: {error}")))?;
        let provider_search = state
            .web_search_enabled
            .then(|| provider_search_tool(&request))
            .flatten();
        let model = state
            .models
            .resolve(model_id)
            .or_else(|| {
                is_permission_review(&request)
                    .then(|| state.models.resolve(&state.selected_model_id))
                    .flatten()
            })
            .ok_or_else(|| {
                ApiError::InvalidRequest(format!(
                    "model '{model_id}' is not available through this bridge"
                ))
            })?;
        let provider_model = model.id.clone();
        let translated = translate(&request, model)?;
        let upstream = ensure_success(state.upstream.send(&translated).await?).await?;
        let usage_guard = RequestUsageGuard::new(&state.usage, provider_model);
        let events = stream::translate(
            upstream,
            model_id.to_owned(),
            state.upstream.clone(),
            provider_search,
            latest_user_text(&request),
            usage_guard,
        );
        Ok(Sse::new(events)
            .keep_alive(
                KeepAlive::new()
                    .interval(std::time::Duration::from_secs(15))
                    .text("ping"),
            )
            .into_response())
    }
    .await;
    emit_diagnostic(&diagnostics, &result, BridgeEndpoint::FxGateway);
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
    let parsed: Value = serde_json::from_str(&body).unwrap_or_default();
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
