use crate::anthropic::{auto_mode, request, response, stream, web_search};
use crate::auth::is_authorized;
use crate::diagnostics::BridgeDiagnostic;
use crate::error::{ApiError, BridgeError};
use crate::search_http;
use crate::timeouts::map_body_error;
use crate::upstream::{NanClient, UpstreamResponse};
use crate::upstream_capture::capture_harness_response;
use crate::usage::{RequestUsageGuard, SharedUsage};
use crate::{
    ActivitySender, BridgeActivity, BridgeConfig, BridgeEndpoint, ClaudeAutoModeReviewStage,
    ClaudeAutoModeTracePayload, DiagnosticSender,
};
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
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const MAX_REQUEST_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone)]
struct AppState {
    upstream: NanClient,
    models: crate::ClaudeModelCatalog,
    session_token: Arc<SecretValue>,
    diagnostics: DiagnosticSender,
    usage: SharedUsage,
    web_search_enabled: bool,
    activities: ActivitySender,
    auto_mode_traces: bool,
    next_auto_mode_review_id: Arc<AtomicU64>,
}

#[derive(Clone)]
struct AutoModeTrace {
    review_id: u64,
    activities: ActivitySender,
}

impl AutoModeTrace {
    fn emit_response(&self, status: u16, response: impl Into<String>) {
        let _ = self
            .activities
            .send(BridgeActivity::ClaudeAutoModeReviewResponse {
                review_id: self.review_id,
                status,
                response: ClaudeAutoModeTracePayload::new(response),
            });
    }

    fn emit_failed(&self, error_code: &'static str) {
        let _ = self
            .activities
            .send(BridgeActivity::ClaudeAutoModeReviewFailed {
                review_id: self.review_id,
                error_code,
            });
    }
}

pub(crate) fn router(
    config: BridgeConfig,
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
        diagnostics,
        usage,
        web_search_enabled: config.web_search_enabled,
        activities,
        auto_mode_traces: config.auto_mode_traces,
        next_auto_mode_review_id: Arc::new(AtomicU64::new(1)),
    };
    Ok(Router::new()
        .route("/api/hello", head(hello))
        .route("/v1/models", get(models))
        .route("/v1/messages", post(messages))
        .route("/v1/search", post(search))
        .route("/v1/messages/count_tokens", post(count_tokens))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(state))
}

async fn search(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    if !state.web_search_enabled {
        return Err(ApiError::SearchDisabled);
    }
    search_http::execute(&headers, &body, &state.upstream, &state.session_token).await
}

async fn hello() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::models::AnthropicModelsResponse>, ApiError> {
    let diagnostics = state.diagnostics.clone();
    let result: Result<Json<crate::models::AnthropicModelsResponse>, ApiError> = async {
        authorize(&headers, &state)?;
        Ok(Json(state.models.api_response()))
    }
    .await;
    emit_diagnostic(&diagnostics, &result, BridgeEndpoint::Models);
    result
}

async fn messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let diagnostics = state.diagnostics.clone();
    let result: Result<Response, ApiError> = async {
        authorize(&headers, &state)?;
        let request = parse_request(&body)?;
        let client_model = request.model().to_owned();
        let model = resolve_model(&state, &client_model)?;
        let provider_model = model.provider_id().to_owned();
        let response_model = model.gateway_id().to_owned();
        let max_output_tokens = model.max_output_tokens();
        let reasoning = model.reasoning();
        if let Some(invocation) = request::web_search_invocation(&request)? {
            if !state.web_search_enabled {
                return Err(ApiError::SearchDisabled);
            }
            return Ok(web_search::execute(&state.upstream, invocation, &client_model).await);
        }
        let translated =
            request::translate(request, &provider_model, max_output_tokens, reasoning)?;
        let auto_mode_trace = begin_auto_mode_trace(
            &state,
            translated.auto_mode_stage,
            &provider_model,
            &translated.body,
        );
        let upstream = match state.upstream.send(&translated.body, &body).await {
            Ok(response) => response,
            Err(error) => {
                if let Some(trace) = &auto_mode_trace {
                    trace.emit_failed(error.code());
                }
                return Err(error);
            }
        };
        let upstream = ensure_success(upstream, auto_mode_trace.as_ref()).await?;
        let capture = upstream.capture_handle();
        let mut usage_guard = RequestUsageGuard::new(&state.usage, provider_model);

        let response = if translated.stream {
            debug_assert!(auto_mode_trace.is_none());
            let events = stream::translate(upstream, response_model, usage_guard);
            Sse::new(events)
                .keep_alive(
                    KeepAlive::new()
                        .interval(Duration::from_secs(15))
                        .text("ping"),
                )
                .into_response()
        } else {
            let value = read_json_response(upstream, auto_mode_trace.as_ref()).await?;
            let provider_usage = response::provider_usage(&value);
            let translated = response::translate(value, &response_model);
            if let (Err(error), Some(trace)) = (&translated, &auto_mode_trace) {
                trace.emit_failed(error.code());
            }
            usage_guard.complete(provider_usage);
            Json(translated?).into_response()
        };
        Ok(capture_harness_response(response, capture))
    }
    .await;
    emit_diagnostic(&diagnostics, &result, BridgeEndpoint::Messages);
    result
}

fn begin_auto_mode_trace(
    app: &AppState,
    classifier_stage: Option<auto_mode::ClassifierStage>,
    model_id: &str,
    request: &Value,
) -> Option<AutoModeTrace> {
    if !app.auto_mode_traces {
        return None;
    }
    let stage = match classifier_stage? {
        auto_mode::ClassifierStage::One => ClaudeAutoModeReviewStage::Initial,
        auto_mode::ClassifierStage::Two => ClaudeAutoModeReviewStage::FollowUp,
    };
    let review_id = app.next_auto_mode_review_id.fetch_add(1, Ordering::Relaxed);
    let _ = app.activities.send(BridgeActivity::ClaudeAutoModeReview {
        review_id,
        stage,
        model_id: model_id.to_owned(),
        request: ClaudeAutoModeTracePayload::new(request.to_string()),
    });
    Some(AutoModeTrace {
        review_id,
        activities: app.activities.clone(),
    })
}

async fn read_json_response(
    response: UpstreamResponse,
    trace: Option<&AutoModeTrace>,
) -> Result<Value, ApiError> {
    if let Some(trace) = trace {
        let status = response.status().as_u16();
        let body = response.bytes().await.map_err(|error| {
            let error = map_body_error(error);
            trace.emit_failed(error.code());
            error
        })?;
        trace.emit_response(status, String::from_utf8_lossy(&body).into_owned());
        serde_json::from_slice(&body).map_err(|error| {
            let error = ApiError::InvalidUpstream(error.to_string());
            trace.emit_failed(error.code());
            error
        })
    } else {
        let body = response.bytes().await.map_err(map_body_error)?;
        serde_json::from_slice(&body).map_err(|error| ApiError::InvalidUpstream(error.to_string()))
    }
}

async fn count_tokens(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let diagnostics = state.diagnostics.clone();
    let result: Result<Json<Value>, ApiError> = async {
        authorize(&headers, &state)?;
        let request = parse_request(&body)?;
        resolve_model(&state, request.model())?;
        Ok(Json(
            json!({"input_tokens": request::estimate_input_tokens(&request)}),
        ))
    }
    .await;
    emit_diagnostic(&diagnostics, &result, BridgeEndpoint::CountTokens);
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
        let _ = state.activities.send(BridgeActivity::AuthenticatedClient);
        Ok(())
    } else {
        Err(ApiError::Unauthorized)
    }
}

fn parse_request(body: &[u8]) -> Result<request::MessagesRequest, ApiError> {
    serde_json::from_slice(body)
        .map_err(|error| ApiError::InvalidRequest(format!("invalid JSON body: {error}")))
}

async fn ensure_success(
    response: UpstreamResponse,
    trace: Option<&AutoModeTrace>,
) -> Result<UpstreamResponse, ApiError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.map_err(|error| {
        let error = map_body_error(error);
        if let Some(trace) = trace {
            trace.emit_failed(error.code());
        }
        error
    })?;
    if let Some(trace) = trace {
        trace.emit_response(status.as_u16(), body.clone());
    }
    let message = sanitize_upstream_error(&body);
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
