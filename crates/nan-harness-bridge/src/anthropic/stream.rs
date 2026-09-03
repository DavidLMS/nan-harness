mod chunk;
mod events;
mod processing;
mod state;
#[cfg(test)]
mod tests;

use crate::timeouts::{STREAM_INACTIVITY_TIMEOUT, map_sse_error, with_inactivity_timeout};
use crate::usage::RequestUsageGuard;
use async_stream::stream;
use axum::response::sse::Event;
use eventsource_stream::Eventsource;
use futures_util::{Stream, StreamExt};
use state::StreamState;
use std::convert::Infallible;

pub(crate) fn translate(
    response: reqwest::Response,
    configured_model: String,
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
        let mut failed = false;
        let mut terminated = false;

        while let Some(item) = source.next().await {
            let source_event = match item {
                Ok(event) => event,
                Err(error) => {
                    yield Ok(events::error(&map_sse_error(error)));
                    failed = true;
                    break;
                }
            };
            if source_event.data.trim() == "[DONE]" {
                terminated = true;
                break;
            }
            if source_event.data.trim().is_empty() {
                continue;
            }

            let chunk = match chunk::parse(&source_event.data) {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Ok(events::error(&error));
                    failed = true;
                    break;
                }
            };

            state.update_metadata(&chunk);
            let mut output = Vec::new();
            if !state.started() {
                output.push(events::message_start(&state, &configured_model));
                state.mark_started();
            }
            for choice in chunk.choices {
                processing::process_choice(&mut state, choice, &mut output);
            }
            for event in output {
                yield Ok(event);
            }
        }

        if !failed && !terminated {
            yield Ok(events::error(&processing::truncated_error()));
        } else if !failed {
            if !state.started() {
                yield Ok(events::message_start(&state, &configured_model));
            }
            match processing::finish_events(&state) {
                Ok(events) => {
                    for event in events {
                        yield Ok(event);
                    }
                    usage_guard.complete(state.usage());
                }
                Err(error) => yield Ok(events::error(&error)),
            }
        }
    }
}
