use super::chunk::FxObject;
use super::completion::{self, StreamOutcome};
use super::events;
use super::state::FxStreamState;
use crate::fx_gateway::request::ProviderSearchTool;
use crate::timeouts::{STREAM_INACTIVITY_TIMEOUT, map_sse_error, with_inactivity_timeout};
use crate::upstream::{NanClient, UpstreamResponse};
use crate::usage::RequestUsageGuard;
use async_stream::stream;
use axum::response::sse::Event;
use eventsource_stream::Eventsource;
use futures_util::{Stream, StreamExt};
use std::convert::Infallible;

pub(super) fn translate(
    response: UpstreamResponse,
    model_id: String,
    upstream: NanClient,
    provider_search: Option<ProviderSearchTool>,
    fallback_query: String,
    usage_guard: RequestUsageGuard,
) -> impl Stream<Item = Result<Event, Infallible>> {
    stream! {
        let mut usage_guard = usage_guard;
        let source = with_inactivity_timeout(
            response.bytes_stream(),
            STREAM_INACTIVITY_TIMEOUT,
        )
        .eventsource();
        futures_util::pin_mut!(source);
        let mut state = FxStreamState::new(model_id);
        let mut outcome = StreamOutcome::Incomplete;

        yield Ok(events::response_metadata(state.model_id()));
        while let Some(item) = source.next().await {
            let source_event = match item {
                Ok(event) => event,
                Err(error) => {
                    yield Ok(events::api_error(&map_sse_error(error)));
                    outcome = StreamOutcome::Failed;
                    break;
                }
            };
            if source_event.data.trim() == "[DONE]" {
                outcome = StreamOutcome::Complete;
                break;
            }
            if source_event.data.trim().is_empty() {
                continue;
            }
            let FxObject(chunk) = match super::chunk::parse(&source_event.data) {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Ok(events::api_error(&error));
                    outcome = StreamOutcome::Failed;
                    break;
                }
            };
            for FxObject(choice) in chunk.choices {
                let FxObject(delta) = choice.delta;
                if let Some(reasoning) = delta.reasoning_content.filter(|text| !text.is_empty()) {
                    if !state.reasoning_started() {
                        yield Ok(events::reasoning_start());
                        state.mark_reasoning_started();
                    }
                    yield Ok(events::reasoning_delta(&reasoning));
                }
                if let Some(text) = delta.content.filter(|text| !text.is_empty()) {
                    if !state.text_started() {
                        yield Ok(events::text_start());
                        state.mark_text_started();
                    }
                    yield Ok(events::text_delta(&text));
                }
                for FxObject(call) in delta.tool_calls {
                    state.update_tool(call);
                }
                state.update_finish_reason(choice.finish_reason);
            }
            if let Some(FxObject(usage)) = chunk.usage {
                state.update_usage(usage);
            }
        }

        match outcome {
            StreamOutcome::Failed => {}
            StreamOutcome::Incomplete => yield Ok(completion::truncated_event()),
            StreamOutcome::Complete => {
                match completion::finish_events(
                    &state,
                    &upstream,
                    provider_search.as_ref(),
                    &fallback_query,
                )
                .await
                {
                    Ok(events) => {
                        for event in events {
                            yield Ok(event);
                        }
                        usage_guard.complete(state.usage());
                    }
                    Err(error) => yield Ok(events::api_error(&error)),
                }
            }
        }
    }
}
