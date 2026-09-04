use super::events;
use super::state::StreamState;
use super::tools;
use crate::error::ApiError;
use crate::responses::request::ToolCatalog;
use axum::response::sse::Event;

pub(super) fn finish_events(
    state: &StreamState,
    catalog: &ToolCatalog,
) -> Result<Vec<Event>, ApiError> {
    let mut events = events::finish_reasoning(state);
    events.extend(events::finish_text(state));
    events.extend(tools::finish_events(state.tools(), catalog)?);
    events.push(events::completed(state));
    Ok(events)
}
