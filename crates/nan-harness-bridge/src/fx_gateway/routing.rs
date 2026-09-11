use super::request::{is_permission_review, latest_user_text, provider_search_tool, translate};
use super::state::{AppState, FxGatewayConfig};
use super::stream;
use crate::auth::is_authorized;
use crate::diagnostics::BridgeDiagnostic;
use crate::error::{ApiError, BridgeError};
use crate::search_http;
use crate::upstream::{FINAL_ERROR_FALLBACK_MESSAGE, FinalErrorBody, UpstreamResponse};
use crate::upstream_capture::capture_harness_response;
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
    search_http::execute(
        &headers,
        &body,
        state.search_client.as_ref(),
        &state.session_token,
    )
    .await
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
        let upstream = match state.upstream.send(&translated, &body).await {
            Ok(response) => ensure_success(response, &provider_model).await?,
            Err(ApiError::BudgetExhausted(stop)) => {
                if is_permission_review(&request) || crate::session_budget::requires_contract(&body)
                {
                    return Err(stop.reject());
                }
                let _ = diagnostics.send(BridgeDiagnostic::from_api_error(
                    &stop.reject(),
                    BridgeEndpoint::FxGateway,
                ));
                return Ok(crate::session_budget::sse(stream::budget_notice(
                    stop, model_id,
                )));
            }
            Err(error) => return Err(error),
        };
        let capture = upstream.capture_handle();
        let usage_guard = RequestUsageGuard::new(&state.usage, provider_model);
        let events = stream::translate(
            upstream,
            model_id.to_owned(),
            state.search_client.clone(),
            provider_search,
            latest_user_text(&request),
            usage_guard,
        );
        let response = Sse::new(events)
            .keep_alive(
                KeepAlive::new()
                    .interval(std::time::Duration::from_secs(15))
                    .text("ping"),
            )
            .into_response();
        Ok(capture_harness_response(response, capture))
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

async fn ensure_success(
    response: UpstreamResponse,
    model: &str,
) -> Result<UpstreamResponse, ApiError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    Err(match response.read_final_error_body().await {
        FinalErrorBody::Complete(body) => {
            ApiError::from_provider_response(status, &body, Some(model))
        }
        FinalErrorBody::Incomplete => ApiError::UpstreamStatus {
            status,
            message: FINAL_ERROR_FALLBACK_MESSAGE.to_owned(),
        },
    })
}
