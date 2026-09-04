mod chunk;
mod completion;
mod events;
mod state;
#[cfg(test)]
mod tests;
mod tools;

use crate::responses::request::ToolCatalog;
use crate::timeouts::{STREAM_INACTIVITY_TIMEOUT, map_sse_error, with_inactivity_timeout};
use crate::upstream::UpstreamResponse;
use crate::usage::RequestUsageGuard;
use async_stream::stream;
use axum::response::sse::Event;
use completion::StreamOutcome;
use eventsource_stream::Eventsource;
use futures_util::{Stream, StreamExt};
use state::StreamState;
use std::convert::Infallible;

pub(crate) fn translate(
    response: UpstreamResponse,
    tools: ToolCatalog,
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
        let mut state = StreamState::default();
        let mut outcome = StreamOutcome::Incomplete;

        while let Some(item) = source.next().await {
            let source_event = match item {
                Ok(event) => event,
                Err(error) => {
                    yield Ok(events::failed(&state, &map_sse_error(error)));
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
            let chunk = match chunk::parse(&source_event.data) {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Ok(events::failed(&state, &error));
                    outcome = StreamOutcome::Failed;
                    break;
                }
            };
            state.update_metadata(&chunk);
            if !state.created() {
                yield Ok(events::created(&state));
                state.mark_created();
            }
            for choice in chunk.choices {
                if let Some(reasoning) = choice.delta.reasoning_content.filter(|content| !content.is_empty()) {
                    if state.reasoning().is_empty() {
                        yield Ok(events::reasoning_item_added());
                        yield Ok(events::reasoning_part_added());
                    }
                    state.append_reasoning(&reasoning);
                    yield Ok(events::reasoning_delta(&reasoning));
                }
                if let Some(content) = choice.delta.content.filter(|content| !content.is_empty()) {
                    if state.text().is_empty() {
                        let output_index = state.text_output_index();
                        yield Ok(events::text_item_added(output_index));
                        yield Ok(events::text_content_part_added(output_index));
                    }
                    state.append_text(&content);
                    yield Ok(events::text_delta(state.text_output_index(), &content));
                }
                for tool_call in choice.delta.tool_calls {
                    state.update_tool(tool_call);
                }
            }
        }

        match outcome {
            StreamOutcome::Failed => {}
            StreamOutcome::Incomplete => yield Ok(completion::truncated_event(&state)),
            StreamOutcome::Complete => {
                if !state.created() {
                    yield Ok(events::created(&state));
                }
                match completion::finish_events(&state, &tools) {
                    Ok(events) => {
                        for event in events {
                            yield Ok(event);
                        }
                        usage_guard.complete(state.usage());
                    }
                    Err(error) => yield Ok(events::failed(&state, &error)),
                }
            }
        }
    }
}
