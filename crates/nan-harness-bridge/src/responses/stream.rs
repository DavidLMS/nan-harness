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
use crate::upstream::{CoordinatedBody, NanClient, RequestCache, UpstreamResponse};
use crate::usage::RequestUsageGuard;
use crate::{BridgeEndpoint, DiagnosticSender};
use async_stream::stream;
use axum::response::sse::Event;
use eventsource_stream::Eventsource;
use futures_util::{Stream, StreamExt};
use nan_harness_coordinator::{AttemptOutcome, RequestPriority, RetryDirective};
use serde_json::Value;
use std::convert::Infallible;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const MAX_RECOVERY_BUFFER_BYTES: usize = 8 * 1024 * 1024;
const MAX_RECOVERY_ATTEMPTS: usize = 5;
const MAX_SEMANTIC_RECOVERY_ATTEMPTS: usize = 8;
const RECOVERY_JITTER_LIMIT: Duration = Duration::from_secs(1);
const PROGRESS_INTERVAL: Duration = Duration::from_secs(30);
static NEXT_RECOVERY_ID: AtomicU64 = AtomicU64::new(1);

enum TranslationItem {
    Event(Event),
    Recoverable {
        error: ApiError,
        directive: RetryDirective,
        provider_response_id: Option<String>,
        empty: bool,
        nudge: Option<RecoveryNudge>,
    },
    Failed(ApiError),
    Complete,
}

#[derive(Clone, Copy)]
enum RecoveryNudge {
    Output,
    Tool,
}

struct TranslationRequest {
    upstream: NanClient,
    body: Value,
    harness_body: Vec<u8>,
    tools: ToolCatalog,
    usage_guard: RequestUsageGuard,
    diagnostics: DiagnosticSender,
    priority: RequestPriority,
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
    translate_request_with_progress_interval(
        TranslationRequest {
            upstream,
            body,
            harness_body,
            tools,
            usage_guard,
            diagnostics,
            priority,
        },
        PROGRESS_INTERVAL,
    )
}

#[allow(clippy::too_many_lines)]
fn translate_request_with_progress_interval(
    request: TranslationRequest,
    progress_interval: Duration,
) -> impl Stream<Item = Result<Event, Infallible>> {
    let TranslationRequest {
        upstream,
        body,
        harness_body,
        tools,
        usage_guard,
        diagnostics,
        priority,
    } = request;
    stream! {
        let mut usage_guard = usage_guard;
        let logical_response = state::StreamState::logical_response();
        yield Ok(events::created(&logical_response));
        yield Ok(events::in_progress(&logical_response));
        let mut progress = tokio::time::interval(progress_interval);
        progress.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        progress.tick().await;
        let mut previous_empty_id = None;
        let mut bypass_cache = false;
        let mut recovery_nudge = None;
        for recovery_attempt in 0..MAX_SEMANTIC_RECOVERY_ATTEMPTS {
            let cache = if bypass_cache {
                RequestCache::Bypass
            } else {
                RequestCache::Default
            };
            let recovered_body = recovery_nudge.map(|nudge| recovery_body(&body, nudge));
            let request_body = recovered_body.as_ref().unwrap_or(&body);
            let send_future = upstream.send_with_priority(
                request_body,
                &harness_body,
                priority,
                cache,
            );
            tokio::pin!(send_future);
            let send_result = loop {
                tokio::select! {
                    result = &mut send_future => break result,
                    _ = progress.tick() => yield Ok(events::in_progress(&logical_response)),
                }
            };
            let response = match send_result {
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
                    TranslationItem::Recoverable {
                        error,
                        directive,
                        provider_response_id,
                        empty,
                        nudge,
                    }
                        if recovery_attempt + 1 < recovery_attempt_limit(nudge) =>
                    {
                        let replay_detected = empty
                            && repeated_response_id(
                                previous_empty_id.as_deref(),
                                provider_response_id.as_deref(),
                            );
                        bypass_cache |= empty;
                        if let Some(nudge) = nudge {
                            recovery_nudge = Some(nudge);
                        }
                        if empty {
                            previous_empty_id = provider_response_id;
                        }
                        emit_recovery_diagnostic(
                            &diagnostics,
                            &error,
                            BridgeRecoveryOutcome::Retrying,
                            recovery_attempt_bucket(recovery_attempt),
                            priority,
                            replay_detected,
                            cache == RequestCache::Bypass,
                        );
                        tokio::time::sleep(recovery_retry_delay(
                            recovery_attempt,
                            directive,
                        ))
                        .await;
                        retry = true;
                        break;
                    }
                    TranslationItem::Recoverable {
                        error,
                        provider_response_id,
                        empty,
                        ..
                    } => {
                        let replay_detected = empty
                            && repeated_response_id(
                                previous_empty_id.as_deref(),
                                provider_response_id.as_deref(),
                            );
                        emit_recovery_diagnostic(
                            &diagnostics,
                            &error,
                            BridgeRecoveryOutcome::Exhausted,
                            recovery_attempt_bucket(recovery_attempt),
                            priority,
                            replay_detected,
                            cache == RequestCache::Bypass,
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

fn recovery_attempt_limit(nudge: Option<RecoveryNudge>) -> usize {
    if nudge.is_some() {
        MAX_SEMANTIC_RECOVERY_ATTEMPTS
    } else {
        MAX_RECOVERY_ATTEMPTS
    }
}

fn recovery_body(body: &Value, nudge: RecoveryNudge) -> Value {
    let mut recovered = body.clone();
    let recovery_id = NEXT_RECOVERY_ID.fetch_add(1, Ordering::Relaxed);
    let action = match nudge {
        RecoveryNudge::Output => {
            "The previous completion had no usable assistant output. Continue the existing task, but do not return reasoning or a progress update. Return either exactly one complete tool call or a complete final answer."
        }
        RecoveryNudge::Tool => {
            "The previous tool call was malformed or truncated. Retry the tool now with concise arguments that fit in one completion. Do not return reasoning, a preamble, or a progress update. For apply_patch, send one complete input including both *** Begin Patch and *** End Patch."
        }
    };
    let instruction = format!(
        "nan-harness recovery {process_id}-{recovery_id}: {action} Do not mention this recovery message.",
        process_id = std::process::id()
    );
    let Some(messages) = recovered.get_mut("messages").and_then(Value::as_array_mut) else {
        return recovered;
    };
    messages.push(serde_json::json!({"role": "user", "content": instruction}));
    recovered
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

#[allow(clippy::too_many_lines)]
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
                    for event in apply_choice(
                        &mut state,
                        &mut committed,
                        choice,
                        logical_response,
                    ) {
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
                yield TranslationItem::Recoverable {
                    error,
                    directive,
                    provider_response_id: state.provider_response_id().map(str::to_owned),
                    empty: false,
                    nudge: None,
                };
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
                yield TranslationItem::Recoverable {
                    error,
                    directive,
                    provider_response_id: state.provider_response_id().map(str::to_owned),
                    empty: false,
                    nudge: None,
                };
            }
            return;
        }
        if state.text().is_empty() && state.tools().is_empty() {
            let directive = body.finish(AttemptOutcome::InvalidResponse).await;
            yield TranslationItem::Recoverable {
                error: empty_response_error(),
                directive,
                provider_response_id: state.provider_response_id().map(str::to_owned),
                empty: true,
                nudge: Some(RecoveryNudge::Output),
            };
            return;
        }
        let finishing = match completion::finish_events(&state, tools) {
            Ok(events) => events,
            Err(error) => {
                let directive = body.finish(AttemptOutcome::Terminal).await;
                if committed {
                    yield TranslationItem::Failed(error);
                } else {
                    yield TranslationItem::Recoverable {
                        error,
                        directive,
                        provider_response_id: state.provider_response_id().map(str::to_owned),
                        empty: false,
                        nudge: Some(RecoveryNudge::Tool),
                    };
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

fn repeated_response_id(previous: Option<&str>, current: Option<&str>) -> bool {
    previous
        .zip(current)
        .is_some_and(|(previous, current)| previous == current)
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
    defer_commit: bool,
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
        if defer_commit {
            state.append_text(&content);
        } else {
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
    if !state.text().is_empty() {
        let output_index = state.text_output_index();
        result.push(events::text_item_added(output_index));
        result.push(events::text_content_part_added(output_index));
        result.push(events::text_delta(output_index, state.text()));
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
    cache_replay_detected: bool,
    cache_bypass_attempted: bool,
) {
    let priority = match priority {
        RequestPriority::Foreground => BridgeRequestPriority::Foreground,
        RequestPriority::Background => BridgeRequestPriority::Background,
    };
    let diagnostic = BridgeDiagnostic::from_api_error(error, BridgeEndpoint::Responses)
        .with_recovery(outcome, attempt, priority)
        .with_cache_recovery(cache_replay_detected, cache_bypass_attempted);
    let _ = diagnostics.send(diagnostic);
}
