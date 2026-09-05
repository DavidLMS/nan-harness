use super::events;
use super::state::StreamState;
use super::tools;
use crate::error::ApiError;
use crate::responses::request::ToolCatalog;
use axum::response::sse::Event;

pub(super) fn finish_events(
    state: &StreamState,
    catalog: &ToolCatalog,
    allow_incomplete_patch: bool,
) -> Result<Vec<Event>, ApiError> {
    let tool_events = if allow_incomplete_patch {
        tools::finish_events_with_incomplete_patch(state.tools(), catalog)?
    } else {
        tools::finish_events(state.tools(), catalog)?
    };
    let mut events = events::finish_reasoning(state);
    events.extend(events::finish_text(state));
    events.extend(tool_events);
    events.push(events::completed(state));
    Ok(events)
}
