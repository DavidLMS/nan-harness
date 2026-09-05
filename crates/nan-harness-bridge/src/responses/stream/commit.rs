use super::TranslationItem;
use super::completion;
use super::decode::Decoded;
use super::events;
use super::recovery::{RecoverableFailure, RecoveryNudge};
use super::state::StreamState;
use crate::error::ApiError;
use crate::responses::request::ToolCatalog;
use crate::upstream::CoordinatedBody;
use crate::usage::RequestUsageGuard;
use axum::response::sse::Event;
use nan_harness_coordinator::AttemptOutcome;

/// Settles one upstream attempt after its SSE source stopped. The commit flag
/// is the boundary: before it, a failure can still be replaced by a fresh
/// attempt; after it, the harness has already seen output and the request can
/// only fail or complete.
pub(super) struct CommitPhase<'a> {
    pub(super) body: &'a mut CoordinatedBody,
    pub(super) decoded: Decoded,
    pub(super) tools: &'a ToolCatalog,
    pub(super) usage_guard: &'a mut RequestUsageGuard,
    pub(super) allow_incomplete_patch: bool,
}

impl CommitPhase<'_> {
    pub(super) async fn settle(mut self) -> Vec<TranslationItem> {
        if let Some(error) = self.decoded.failure.take() {
            return self.stream_failure(error).await;
        }
        if !self.decoded.done {
            return self.truncated_stream().await;
        }
        if self.decoded.state.text().is_empty() && self.decoded.state.tools().is_empty() {
            return self.empty_response().await;
        }
        self.complete().await
    }

    async fn stream_failure(&mut self, error: ApiError) -> Vec<TranslationItem> {
        let directive = self.body.finish(stream_failure_outcome(&error)).await;
        if !self.decoded.committed && is_recoverable_stream_failure(&error) {
            return vec![TranslationItem::Recoverable(RecoverableFailure {
                error,
                directive,
                provider_response_id: self.provider_response_id(),
                empty: false,
                nudge: None,
            })];
        }
        vec![TranslationItem::Failed(error)]
    }

    async fn truncated_stream(&mut self) -> Vec<TranslationItem> {
        let directive = self.body.finish(AttemptOutcome::InvalidResponse).await;
        let error = ApiError::InvalidUpstream("stream ended before the [DONE] marker".to_owned());
        if self.decoded.committed {
            return vec![TranslationItem::Failed(error)];
        }
        vec![TranslationItem::Recoverable(RecoverableFailure {
            error,
            directive,
            provider_response_id: self.provider_response_id(),
            empty: false,
            nudge: None,
        })]
    }

    async fn empty_response(&mut self) -> Vec<TranslationItem> {
        let directive = self.body.finish(AttemptOutcome::Terminal).await;
        vec![TranslationItem::Recoverable(RecoverableFailure {
            error: empty_response_error(),
            directive,
            provider_response_id: self.provider_response_id(),
            empty: true,
            nudge: Some(RecoveryNudge::Output),
        })]
    }

    async fn complete(&mut self) -> Vec<TranslationItem> {
        let error = match completion::finish_events(&self.decoded.state, self.tools, false) {
            Ok(finishing) => {
                let _ = self.body.finish(AttemptOutcome::Success).await;
                self.usage_guard.complete(self.decoded.state.usage());
                let mut items = Vec::new();
                if !self.decoded.committed {
                    items.extend(self.commit_prefix_items());
                }
                items.extend(finishing.into_iter().map(TranslationItem::Event));
                items.push(TranslationItem::Complete);
                return items;
            }
            Err(error) => error,
        };
        // Codex's patch executor rejects malformed input atomically. On the final
        // recovery attempt, returning that rejection to Codex is safer than ending
        // the whole session, but this escape hatch must not apply to arbitrary tools.
        if self.allow_incomplete_patch
            && !self.decoded.committed
            && let Ok(finishing) = completion::finish_events(&self.decoded.state, self.tools, true)
        {
            let _ = self.body.finish(AttemptOutcome::Terminal).await;
            self.usage_guard.complete(self.decoded.state.usage());
            let mut items = vec![TranslationItem::Delegated(error)];
            items.extend(self.commit_prefix_items());
            items.extend(finishing.into_iter().map(TranslationItem::Event));
            items.push(TranslationItem::Complete);
            return items;
        }
        let directive = self.body.finish(AttemptOutcome::Terminal).await;
        if self.decoded.committed {
            return vec![TranslationItem::Failed(error)];
        }
        vec![TranslationItem::Recoverable(RecoverableFailure {
            error,
            directive,
            provider_response_id: self.provider_response_id(),
            empty: false,
            nudge: Some(RecoveryNudge::Tool),
        })]
    }

    fn commit_prefix_items(&mut self) -> Vec<TranslationItem> {
        commit_prefix(&mut self.decoded.state)
            .into_iter()
            .map(TranslationItem::Event)
            .collect()
    }

    fn provider_response_id(&self) -> Option<String> {
        self.decoded.state.provider_response_id().map(str::to_owned)
    }
}

/// Replays the output buffered before the commit boundary so a recovered
/// attempt starts the harness-visible response from a consistent prefix.
pub(super) fn commit_prefix(state: &mut StreamState) -> Vec<Event> {
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

pub(super) fn empty_response_error() -> ApiError {
    ApiError::InvalidUpstream("stream completed without visible content or a tool call".to_owned())
}

pub(super) const fn stream_failure_outcome(error: &ApiError) -> AttemptOutcome {
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
