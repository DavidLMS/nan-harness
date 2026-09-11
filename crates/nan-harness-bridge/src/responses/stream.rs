mod chunk;
mod commit;
mod completion;
mod decode;
mod events;
#[cfg(test)]
mod framing_tests;
mod progress;
mod recovery;
mod state;
#[cfg(test)]
mod tests;
mod tools;

use crate::DiagnosticSender;
use crate::error::ApiError;
use crate::responses::request::ToolCatalog;
use crate::session_budget::SessionBudgetReached;
use crate::sse_framing::guard;
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

pub(crate) async fn translate_request(
    upstream: NanClient,
    body: Value,
    harness_body: &[u8],
    tools: ToolCatalog,
    usage_guard: RequestUsageGuard,
    diagnostics: DiagnosticSender,
    priority: RequestPriority,
) -> Result<
    (
        impl Stream<Item = Result<Event, Infallible>> + use<>,
        Option<nan_harness_coordinator::CaptureRequest>,
    ),
    ApiError,
> {
    let hold_contract =
        upstream.has_session_budget() && crate::session_budget::requires_contract(harness_body);
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
    let events = if hold_contract {
        // Responses already buffers assistant output until validation. Hold the
        // HTTP response too for contracts that cannot accept a local text notice,
        // including when budget admission rejects a semantic recovery attempt.
        use futures_util::TryStreamExt as _;
        let buffered: Vec<Event> = stream
            .try_collect()
            .await
            .map_err(SessionBudgetReached::reject)?;
        futures_util::stream::iter(buffered.into_iter().map(Ok)).boxed()
    } else {
        stream! {
            futures_util::pin_mut!(stream);
            while let Some(item) = stream.next().await {
                match item {
                    Ok(event) => yield Ok(event),
                    Err(stop) => {
                        for event in budget_notice(stop) { yield Ok(event); }
                        return;
                    }
                }
            }
        }
        .boxed()
    };
    Ok((events, response_capture))
}

fn translate_request_with_progress_interval(
    request: TranslationRequest,
    progress_interval: Duration,
) -> impl Stream<Item = Result<Event, SessionBudgetReached>> {
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
        let provider_model = body.get("model").and_then(Value::as_str).map(str::to_owned);
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
            let response = match accept_response(send_result, provider_model.as_deref()).await {
                Ok(response) => response,
                Err(ApiError::BudgetExhausted(stop)) => {
                    session.record_failure(&stop.reject());
                    if !session.has_sent() { usage_guard.local_response(); }
                    yield Err(stop);
                    return;
                }
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
            let bytes = body.bytes_stream();
            let source = guard(bytes).eventsource();
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
    model: Option<&str>,
) -> Result<UpstreamResponse, ApiError> {
    let response = send_result?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    Err(match response.read_final_error_body().await {
        FinalErrorBody::Complete(body) => ApiError::from_provider_response(status, &body, model),
        FinalErrorBody::Incomplete => ApiError::UpstreamStatus {
            status,
            message: FINAL_ERROR_FALLBACK_MESSAGE.to_owned(),
        },
    })
}

fn budget_notice(stop: SessionBudgetReached) -> Vec<Event> {
    let mut state = state::StreamState::logical_response();
    state.append_text(&stop.to_string());
    let mut output = commit::commit_prefix(&mut state);
    output.extend(events::finish_text(&state));
    output.push(events::responses_event("response.completed", &serde_json::json!({
        "type": "response.completed",
        "response": {
            "id": state.response_id(), "object": "response", "status": "completed", "error": null,
            "output": [{"type":"message", "id":"msg_nan_harness", "status":"completed", "role":"assistant",
                "content":[{"type":"output_text", "text":state.text(), "annotations":[]}]}],
            "usage":{"input_tokens":0,"output_tokens":0,"total_tokens":0,
                "input_tokens_details":null,"output_tokens_details":{"reasoning_tokens":0}}
        }
    })));
    output
}
