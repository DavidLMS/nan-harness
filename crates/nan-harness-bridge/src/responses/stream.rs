mod chunk;
mod completion;
mod events;
mod state;
#[cfg(test)]
mod tests;
mod tools;

use crate::diagnostics::{
    BridgeAttemptBucket, BridgeDiagnostic, BridgeRecoveryOutcome, BridgeRequestPriority,
};
use crate::error::ApiError;
use crate::responses::request::ToolCatalog;
use crate::upstream::{CoordinatedBody, NanClient, UpstreamResponse};
use crate::usage::RequestUsageGuard;
use crate::{BridgeEndpoint, DiagnosticSender};
use async_stream::stream;
use axum::response::sse::Event;
use eventsource_stream::Eventsource;
use futures_util::{Stream, StreamExt};
use nan_harness_coordinator::{AttemptOutcome, RequestPriority, RetryDirective};
use serde_json::Value;
use std::convert::Infallible;
use std::time::Duration;

const MAX_RECOVERY_BUFFER_BYTES: usize = 8 * 1024 * 1024;
const MAX_RECOVERY_ATTEMPTS: usize = 3;
const RECOVERY_JITTER_LIMIT: Duration = Duration::from_secs(1);
const PROGRESS_INTERVAL: Duration = Duration::from_secs(30);

enum TranslationItem {
    Event(Event),
    Recoverable {
        error: ApiError,
        directive: RetryDirective,
    },
    Failed(ApiError),
    Complete,
}

pub(crate) fn translate_request(
    upstream: NanClient,
    body: Value,
    harness_body: Vec<u8>,
    tools: ToolCatalog,
    usage_guard: RequestUsageGuard,
    diagnostics: DiagnosticSender,
    priority: RequestPriority,
) -> impl Stream<Item = Result<Event, Infallible>> {
    stream! {
        let mut usage_guard = usage_guard;
        let logical_response = state::StreamState::logical_response();
        yield Ok(events::created(&logical_response));
        yield Ok(events::in_progress(&logical_response));
        for recovery_attempt in 0..MAX_RECOVERY_ATTEMPTS {
            let response = match upstream
                .send_with_priority(&body, &harness_body, priority)
                .await
            {
                Ok(response) => match ensure_success(response).await {
                    Ok(response) => response,
                    Err(error) => {
                        emit_diagnostic(&diagnostics, &error);
                        yield Ok(events::failed(&state::StreamState::default(), &error));
                        return;
                    }
                },
                Err(error) => {
                    emit_diagnostic(&diagnostics, &error);
                    yield Ok(events::failed(&state::StreamState::default(), &error));
                    return;
                }
            };
            let items = translate_items(response, &tools, &mut usage_guard, true);
            futures_util::pin_mut!(items);
            let mut progress = tokio::time::interval(PROGRESS_INTERVAL);
            progress.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            progress.tick().await;
            let mut retry = false;
            loop {
                let item = tokio::select! {
                    item = items.next() => item,
                    _ = progress.tick() => {
                        yield Ok(events::in_progress(&logical_response));
                        continue;
                    }
                };
                let Some(item) = item else {
                    break;
                };
                match item {
                    TranslationItem::Event(event) => yield Ok(event),
                    TranslationItem::Complete => return,
                    TranslationItem::Recoverable { error, directive }
                        if recovery_attempt + 1 < MAX_RECOVERY_ATTEMPTS =>
                    {
                        emit_recovery_diagnostic(
                            &diagnostics,
                            &error,
                            BridgeRecoveryOutcome::Retrying,
                            recovery_attempt_bucket(recovery_attempt),
                            priority,
                        );
                        tokio::time::sleep(recovery_retry_delay(
                            recovery_attempt,
                            directive,
                        ))
                        .await;
                        retry = true;
                        break;
                    }
                    TranslationItem::Recoverable { error, .. } => {
                        emit_recovery_diagnostic(
                            &diagnostics,
                            &error,
                            BridgeRecoveryOutcome::Exhausted,
                            recovery_attempt_bucket(recovery_attempt),
                            priority,
                        );
                        yield Ok(events::failed(&state::StreamState::default(), &error));
                        return;
                    }
                    TranslationItem::Failed(error) => {
                        emit_diagnostic(&diagnostics, &error);
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
        let items = translate_items(response, &tools, &mut usage_guard, false);
        futures_util::pin_mut!(items);
        while let Some(item) = items.next().await {
            match item {
                TranslationItem::Event(event) => yield Ok(event),
                TranslationItem::Recoverable { error, .. } => {
                    yield Ok(events::failed(&state::StreamState::default(), &error));
                }
                TranslationItem::Failed(error) => {
                    yield Ok(events::failed(&state::StreamState::default(), &error));
                }
                TranslationItem::Complete => {}
            }
        }
    }
}

fn translate_items<'a>(
    response: UpstreamResponse,
    tools: &'a ToolCatalog,
    usage_guard: &'a mut RequestUsageGuard,
    logical_response: bool,
) -> impl Stream<Item = TranslationItem> + 'a {
    stream! {
        let mut body = response.into_coordinated_body();
        let mut state = initial_stream_state(logical_response);
        let mut committed = false;
        let mut done = false;
        let mut failure = None;
        {
            let bytes = body_bytes(&mut body);
            let source = bytes.eventsource();
            futures_util::pin_mut!(source);
            while let Some(item) = source.next().await {
                let source_event = match item {
                    Ok(event) => event,
                    Err(error) => {
                        failure = Some(crate::timeouts::map_sse_error(error));
                        break;
                    }
                };
                if source_event.data.trim() == "[DONE]" {
                    done = true;
                    break;
                }
                if source_event.data.trim().is_empty() {
                    continue;
                }
                let parsed = match chunk::parse(&source_event.data) {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                };
                state.update_metadata(&parsed);
                for choice in parsed.choices {
                    for event in apply_choice(&mut state, &mut committed, choice) {
                        yield TranslationItem::Event(event);
                    }
                }
                if !committed && state.buffered_bytes() > MAX_RECOVERY_BUFFER_BYTES {
                    failure = Some(ApiError::InvalidUpstream(
                        "buffered response exceeded the 8 MiB recovery limit".to_owned(),
                    ));
                    break;
                }
            }
        }
        if let Some(error) = failure {
            let directive = body.finish(stream_failure_outcome(&error)).await;
            if !committed && is_recoverable_stream_failure(&error) {
                yield TranslationItem::Recoverable { error, directive };
            } else {
                yield TranslationItem::Failed(error);
            }
            return;
        }
        if !done {
            let directive = body.finish(AttemptOutcome::InvalidResponse).await;
            let error = ApiError::InvalidUpstream(
                "stream ended before the [DONE] marker".to_owned(),
            );
            if committed {
                yield TranslationItem::Failed(error);
            } else {
                yield TranslationItem::Recoverable { error, directive };
            }
            return;
        }
        if state.text().is_empty() && state.tools().is_empty() {
            let directive = body.finish(AttemptOutcome::InvalidResponse).await;
            yield TranslationItem::Recoverable {
                error: empty_response_error(),
                directive,
            };
            return;
        }
        let finishing = match completion::finish_events(&state, tools) {
            Ok(events) => events,
            Err(error) => {
                let directive = body.finish(AttemptOutcome::InvalidResponse).await;
                if committed {
                    yield TranslationItem::Failed(error);
                } else {
                    yield TranslationItem::Recoverable { error, directive };
                }
                return;
            }
        };
        let _ = body.finish(AttemptOutcome::Success).await;
        usage_guard.complete(state.usage());
        if !committed {
            for event in commit_prefix(&mut state) {
                yield TranslationItem::Event(event);
            }
        }
        for event in finishing {
            yield TranslationItem::Event(event);
        }
        yield TranslationItem::Complete;
    }
}

fn initial_stream_state(logical_response: bool) -> state::StreamState {
    if logical_response {
        state::StreamState::logical_response()
    } else {
        state::StreamState::default()
    }
}

fn recovery_attempt_bucket(attempt: usize) -> BridgeAttemptBucket {
    match attempt {
        0 => BridgeAttemptBucket::First,
        1 => BridgeAttemptBucket::Second,
        _ => BridgeAttemptBucket::Later,
    }
}

fn recovery_retry_delay(attempt: usize, directive: RetryDirective) -> Duration {
    recovery_retry_delay_with_jitter(attempt, directive, random_recovery_jitter())
}

fn recovery_retry_delay_with_jitter(
    attempt: usize,
    directive: RetryDirective,
    jitter: Duration,
) -> Duration {
    let floor = if attempt == 0 {
        Duration::from_secs(1)
    } else {
        Duration::from_secs(2)
    };
    let local = floor.saturating_add(jitter.min(RECOVERY_JITTER_LIMIT));
    match directive {
        RetryDirective::Complete => local,
        RetryDirective::RetryAfter(coordinator) => local.max(coordinator),
    }
}

const fn stream_failure_outcome(error: &ApiError) -> AttemptOutcome {
    match error {
        ApiError::UpstreamTimeout(_) => AttemptOutcome::Timeout,
        ApiError::UpstreamTransport(_) => AttemptOutcome::Transport,
        _ => AttemptOutcome::InvalidResponse,
    }
}

const fn is_recoverable_stream_failure(error: &ApiError) -> bool {
    matches!(
        error,
        ApiError::UpstreamTimeout(_)
            | ApiError::UpstreamTransport(_)
            | ApiError::InvalidUpstream(_)
    )
}

fn random_recovery_jitter() -> Duration {
    let mut random = [0_u8; 8];
    if getrandom::fill(&mut random).is_err() {
        return Duration::ZERO;
    }
    Duration::from_millis(u64::from_le_bytes(random) % 1_001)
}

fn apply_choice(
    state: &mut state::StreamState,
    committed: &mut bool,
    choice: chunk::Choice,
) -> Vec<Event> {
    let mut translated = Vec::new();
    if let Some(reasoning) = choice
        .delta
        .reasoning_content
        .filter(|value| !value.is_empty())
    {
        state.append_reasoning(&reasoning);
        if *committed {
            translated.push(events::reasoning_delta(&reasoning));
        }
    }
    if let Some(content) = choice.delta.content.filter(|value| !value.is_empty()) {
        if !*committed {
            translated.extend(commit_prefix(state));
            *committed = true;
        }
        if state.text().is_empty() {
            let output_index = state.text_output_index();
            translated.push(events::text_item_added(output_index));
            translated.push(events::text_content_part_added(output_index));
        }
        state.append_text(&content);
        translated.push(events::text_delta(state.text_output_index(), &content));
    }
    for tool_call in choice.delta.tool_calls {
        state.update_tool(tool_call);
    }
    translated
}

fn body_bytes(
    body: &mut CoordinatedBody,
) -> impl Stream<Item = Result<bytes::Bytes, ApiError>> + '_ {
    stream! {
        loop {
            match body.next().await {
                Ok(Some(bytes)) => yield Ok(bytes),
                Ok(None) => break,
                Err(error) => {
                    yield Err(error);
                    break;
                }
            }
        }
    }
}

fn commit_prefix(state: &mut state::StreamState) -> Vec<Event> {
    let mut result = Vec::new();
    if !state.is_created() {
        result.push(events::created(state));
        state.mark_created();
    }
    if !state.reasoning().is_empty() {
        result.push(events::reasoning_item_added());
        result.push(events::reasoning_part_added());
        result.push(events::reasoning_delta(state.reasoning()));
    }
    result
}

async fn ensure_success(response: UpstreamResponse) -> Result<UpstreamResponse, ApiError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response
        .text()
        .await
        .map_err(crate::timeouts::map_body_error)?;
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

fn empty_response_error() -> ApiError {
    ApiError::InvalidUpstream("stream completed without visible content or a tool call".to_owned())
}

fn emit_diagnostic(diagnostics: &DiagnosticSender, error: &ApiError) {
    let _ = diagnostics.send(BridgeDiagnostic::from_api_error(
        error,
        BridgeEndpoint::Responses,
    ));
}

fn emit_recovery_diagnostic(
    diagnostics: &DiagnosticSender,
    error: &ApiError,
    outcome: BridgeRecoveryOutcome,
    attempt: BridgeAttemptBucket,
    priority: RequestPriority,
) {
    let priority = match priority {
        RequestPriority::Foreground => BridgeRequestPriority::Foreground,
        RequestPriority::Background => BridgeRequestPriority::Background,
    };
    let diagnostic = BridgeDiagnostic::from_api_error(error, BridgeEndpoint::Responses)
        .with_recovery(outcome, attempt, priority);
    let _ = diagnostics.send(diagnostic);
}
