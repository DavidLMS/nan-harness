mod chunk;
mod commit;
mod completion;
mod decode;
mod events;
mod progress;
mod recovery;
mod state;
#[cfg(test)]
mod tests;
mod tools;

use crate::DiagnosticSender;
use crate::error::ApiError;
use crate::responses::request::ToolCatalog;
use crate::upstream::{
    FINAL_ERROR_FALLBACK_MESSAGE, FinalErrorBody, NanClient, UpstreamCapture, UpstreamResponse,
};
use crate::usage::RequestUsageGuard;
use async_stream::stream;
use axum::response::sse::Event;
use eventsource_stream::Eventsource;
use futures_util::{Stream, StreamExt};
use nan_harness_coordinator::RequestPriority;
use progress::{PROGRESS_INTERVAL, ProgressTicker};
use recovery::{RecoverableFailure, RecoveryDecision, RecoverySession};
use serde_json::Value;
use std::convert::Infallible;
use std::time::Duration;

enum TranslationItem {
    Event(Event),
    Delegated(ApiError),
    Recoverable(RecoverableFailure),
    Failed(ApiError),
    Complete,
}

struct TranslationRequest {
    upstream: NanClient,
    body: Value,
    tools: ToolCatalog,
    usage_guard: RequestUsageGuard,
    diagnostics: DiagnosticSender,
    priority: RequestPriority,
    capture: UpstreamCapture,
}

pub(crate) fn translate_request(
    upstream: NanClient,
    body: Value,
    harness_body: &[u8],
    tools: ToolCatalog,
    usage_guard: RequestUsageGuard,
    diagnostics: DiagnosticSender,
    priority: RequestPriority,
) -> (
    impl Stream<Item = Result<Event, Infallible>> + use<>,
    Option<nan_harness_coordinator::CaptureRequest>,
) {
    let capture = upstream.begin_capture(harness_body);
    let response_capture = capture.handle();
    let stream = translate_request_with_progress_interval(
        TranslationRequest {
            upstream,
            body,
            tools,
            usage_guard,
            diagnostics,
            priority,
            capture,
        },
        PROGRESS_INTERVAL,
    );
    (stream, response_capture)
}

fn translate_request_with_progress_interval(
    request: TranslationRequest,
    progress_interval: Duration,
) -> impl Stream<Item = Result<Event, Infallible>> {
    let TranslationRequest {
        upstream,
        body,
        tools,
        usage_guard,
        diagnostics,
        priority,
        capture,
    } = request;
    stream! {
        let mut usage_guard = usage_guard;
        let logical_response = state::StreamState::logical_response();
        yield Ok(events::created(&logical_response));
        yield Ok(events::in_progress(&logical_response));
        let mut progress = ProgressTicker::start(progress_interval, logical_response).await;
        let mut session = RecoverySession::new(body, diagnostics, priority);
        for attempt in 0..recovery::MAX_SEMANTIC_RECOVERY_ATTEMPTS {
            let cache = session.begin_attempt();
            let send_result = {
                let (request_body, budget) = session.attempt_body();
                let send_future = upstream.send_with_priority(
                    &request_body,
                    priority,
                    cache,
                    &capture,
                    budget,
                );
                tokio::pin!(send_future);
                loop {
                    tokio::select! {
                        result = &mut send_future => break result,
                        event = progress.beat() => yield Ok(event),
                    }
                }
            };
            let response = match accept_response(send_result).await {
                Ok(response) => response,
                Err(error) => {
                    session.record_failure(&error);
                    yield Ok(events::failed(&state::StreamState::default(), &error));
                    return;
                }
            };
            let items = translate_items(
                response,
                &tools,
                &mut usage_guard,
                true,
                session.is_final_attempt(attempt),
            );
            futures_util::pin_mut!(items);
            let mut retry = false;
            loop {
                let item = tokio::select! {
                    item = items.next() => item,
                    event = progress.beat() => {
                        yield Ok(event);
                        continue;
                    }
                };
                let Some(item) = item else {
                    break;
                };
                match item {
                    TranslationItem::Event(event) => yield Ok(event),
                    TranslationItem::Delegated(error) => session.record_delegated(attempt, &error),
                    TranslationItem::Complete => return,
                    TranslationItem::Recoverable(failure) => {
                        match session.handle_recoverable(attempt, failure).await {
                            RecoveryDecision::Retry => {
                                retry = true;
                                break;
                            }
                            RecoveryDecision::Exhausted(error) => {
                                yield Ok(events::failed(&state::StreamState::default(), &error));
                                return;
                            }
                        }
                    }
                    TranslationItem::Failed(error) => {
                        session.record_failure(&error);
                        yield Ok(events::failed(&state::StreamState::default(), &error));
                        return;
                    }
                }
            }
            if !retry {
                return;
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn translate(
    response: UpstreamResponse,
    tools: ToolCatalog,
    usage_guard: RequestUsageGuard,
) -> impl Stream<Item = Result<Event, Infallible>> {
    stream! {
        let mut usage_guard = usage_guard;
        let items = translate_items(response, &tools, &mut usage_guard, false, false);
        futures_util::pin_mut!(items);
        while let Some(item) = items.next().await {
            let error = match item {
                TranslationItem::Event(event) => {
                    yield Ok(event);
                    continue;
                }
                TranslationItem::Delegated(error)
                | TranslationItem::Failed(error)
                | TranslationItem::Recoverable(RecoverableFailure { error, .. }) => error,
                TranslationItem::Complete => continue,
            };
            yield Ok(events::failed(&state::StreamState::default(), &error));
        }
    }
}

fn translate_items<'a>(
    response: UpstreamResponse,
    tools: &'a ToolCatalog,
    usage_guard: &'a mut RequestUsageGuard,
    logical_response: bool,
    allow_incomplete_patch: bool,
) -> impl Stream<Item = TranslationItem> + 'a {
    stream! {
        let mut body = response.into_coordinated_body();
        let mut decoder = decode::Decoder::new(logical_response);
        {
            let bytes = decode::body_bytes(&mut body);
            let source = bytes.eventsource();
            futures_util::pin_mut!(source);
            while let Some(item) = source.next().await {
                let Some(events) = decoder.step(item) else {
                    break;
                };
                for event in events {
                    yield TranslationItem::Event(event);
                }
            }
        }
        let phase = commit::CommitPhase {
            body: &mut body,
            decoded: decoder.finish(),
            tools,
            usage_guard,
            allow_incomplete_patch,
        };
        for item in phase.settle().await {
            yield item;
        }
    }
}

async fn accept_response(
    send_result: Result<UpstreamResponse, ApiError>,
) -> Result<UpstreamResponse, ApiError> {
    let response = send_result?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let message = match response.read_final_error_body().await {
        FinalErrorBody::Complete(body) => {
            let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            parsed
                .pointer("/error/message")
                .or_else(|| parsed.get("message"))
                .and_then(Value::as_str)
                .unwrap_or(FINAL_ERROR_FALLBACK_MESSAGE)
                .replace(['\r', '\n'], " ")
                .chars()
                .take(300)
                .collect()
        }
        FinalErrorBody::Incomplete => FINAL_ERROR_FALLBACK_MESSAGE.to_owned(),
    };
    Err(ApiError::UpstreamStatus { status, message })
}
