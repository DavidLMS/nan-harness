use crate::auth::is_authorized;
use crate::diagnostics::BridgeDiagnostic;
use crate::error::{ApiError, BridgeError};
use crate::responses::{models, request, search, stream};
use crate::search_http;
use crate::upstream::NanClient;
use crate::usage::{RequestUsageGuard, SharedUsage};
use crate::{
    ActivitySender, BridgeActivity, BridgeEndpoint, DiagnosticSender, ResponsesBridgeConfig,
};
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, head, post};
use nan_harness_coordinator::RequestPriority;
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
    activities: ActivitySender,
}

pub(crate) fn router(
    config: ResponsesBridgeConfig,
    diagnostics: DiagnosticSender,
    activities: ActivitySender,
    usage: SharedUsage,
) -> Result<Router, BridgeError> {
    let state = AppState {
        upstream: NanClient::new(
            &config.provider_base_url,
            config.provider_api_key,
            &config.launch_id,
        )?,
        models: config.models,
        session_token: config.session_token,
        search_references: Arc::new(search::SearchReferences::default()),
        web_search_enabled: config.web_search_enabled,
        diagnostics,
        usage,
        activities,
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
        let priority = request_priority(&headers);
        let usage_guard = RequestUsageGuard::new(&state.usage, provider_model);
        let events = stream::translate_request(
            state.upstream.clone(),
            translated.body,
            body.to_vec(),
            translated.tools,
            usage_guard,
            diagnostics.clone(),
            priority,
        );
        let response = Sse::new(events)
            .keep_alive(
                KeepAlive::new()
                    .interval(Duration::from_secs(10))
                    .text("ping"),
            )
            .into_response();
        Ok(response)
    }
    .await;
    emit_diagnostic(&diagnostics, &result, BridgeEndpoint::Responses);
    result
}

fn request_priority(headers: &HeaderMap) -> RequestPriority {
    let system = headers
        .get("x-codex-turn-metadata")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .and_then(|value| {
            value
                .get("thread_source")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .is_some_and(|source| source == "system");
    if system {
        RequestPriority::Background
    } else {
        RequestPriority::Foreground
    }
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
        let _ = state.activities.send(BridgeActivity::AuthenticatedClient);
        Ok(())
    } else {
        Err(ApiError::Unauthorized)
    }
}
