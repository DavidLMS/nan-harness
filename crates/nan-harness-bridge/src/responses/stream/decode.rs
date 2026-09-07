use super::commit::commit_prefix;
use super::state::StreamState;
use super::{chunk, events};
use crate::error::ApiError;
use axum::response::sse::Event;
use eventsource_stream::EventStreamError;

pub(super) const MAX_RECOVERY_BUFFER_BYTES: usize = 8 * 1024 * 1024;

type SourceEvent = Result<eventsource_stream::Event, EventStreamError<ApiError>>;

/// Everything the commit phase needs once the upstream SSE source stops.
pub(super) struct Decoded {
    pub(super) state: StreamState,
    pub(super) committed: bool,
    pub(super) failure: Option<ApiError>,
    pub(super) done: bool,
}

/// Consumes upstream chunks and tracks whether any assistant output has already
/// been committed to the harness, which is what makes a failure unrecoverable.
pub(super) struct Decoder {
    state: StreamState,
    committed: bool,
    failure: Option<ApiError>,
    done: bool,
    defer_commit: bool,
}

impl Decoder {
    pub(super) fn new(logical_response: bool) -> Self {
        let state = if logical_response {
            StreamState::logical_response()
        } else {
            StreamState::default()
        };
        Self {
            state,
            committed: false,
            failure: None,
            done: false,
            defer_commit: logical_response,
        }
    }

    /// Returns the events translated from one upstream chunk, or `None` when
    /// the source must stop because it completed or failed.
    pub(super) fn step(&mut self, item: SourceEvent) -> Option<Vec<Event>> {
        let source_event = match item {
            Ok(event) => event,
            Err(error) => {
                self.failure = Some(crate::timeouts::map_sse_error(error));
                return None;
            }
        };
        if source_event.data.trim() == "[DONE]" {
            self.done = true;
            return None;
        }
        if source_event.data.trim().is_empty() {
            return Some(Vec::new());
        }
        let parsed = match chunk::parse(&source_event.data) {
            Ok(chunk) => chunk,
            Err(error) => {
                self.failure = Some(error);
                return None;
            }
        };
        self.state.update_metadata(&parsed);
        let mut translated = Vec::new();
        for choice in parsed.choices {
            translated.extend(apply_choice(
                &mut self.state,
                &mut self.committed,
                choice,
                self.defer_commit,
            ));
        }
        if !self.committed && self.state.buffered_bytes() > MAX_RECOVERY_BUFFER_BYTES {
            self.failure = Some(ApiError::InvalidUpstream(
                "buffered response exceeded the 8 MiB recovery limit".to_owned(),
            ));
            return None;
        }
        Some(translated)
    }

    pub(super) fn finish(self) -> Decoded {
        Decoded {
            state: self.state,
            committed: self.committed,
            failure: self.failure,
            done: self.done,
        }
    }
}

fn apply_choice(
    state: &mut StreamState,
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
