use super::chunk::{Choice, ToolCallDelta};
use super::events;
use super::state::StreamState;
use crate::error::ApiError;
use axum::response::sse::Event;

pub(super) fn process_choice(state: &mut StreamState, choice: Choice, output: &mut Vec<Event>) {
    if let Some(reasoning) = choice
        .delta
        .reasoning_content
        .filter(|content| !content.is_empty())
    {
        push_thinking_delta(state, &reasoning, output);
    }
    if let Some(content) = choice.delta.content.filter(|content| !content.is_empty()) {
        push_text_delta(state, &content, output);
    }
    for tool_call in choice.delta.tool_calls {
        push_tool_delta(state, tool_call, output);
    }
    state.update_finish_reason(choice.finish_reason);
}

pub(super) fn push_thinking_delta(state: &mut StreamState, content: &str, output: &mut Vec<Event>) {
    let (index, started) = state.thinking_content_index();
    if started {
        output.push(events::thinking_start(index));
    }
    output.push(events::thinking_delta(index, content));
}

pub(super) fn push_text_delta(state: &mut StreamState, content: &str, output: &mut Vec<Event>) {
    let (index, started) = state.text_content_index();
    if started {
        output.push(events::text_start(index));
    }
    output.push(events::text_delta(index, content));
}

pub(super) fn push_tool_delta(
    state: &mut StreamState,
    delta: ToolCallDelta,
    output: &mut Vec<Event>,
) {
    let tool = state.apply_tool_delta(delta);
    if !tool.started() && tool.ready_to_start() {
        output.push(events::tool_start(tool));
        tool.mark_started();
    }
    if tool.started() {
        let index = tool.content_index();
        let pending_arguments = tool.take_pending_arguments();
        if !pending_arguments.is_empty() {
            output.push(events::tool_delta(index, &pending_arguments));
        }
    }
}

pub(super) fn finish_events(state: &StreamState) -> Result<Vec<Event>, ApiError> {
    if let Some(tool) = state.unfinished_tool() {
        return Err(ApiError::InvalidUpstream(format!(
            "tool call {} ended without an id and name",
            tool.content_index()
        )));
    }

    let mut output = state
        .content_indexes()
        .into_iter()
        .map(events::content_stop)
        .collect::<Vec<_>>();
    output.push(events::message_delta(state));
    output.push(events::message_stop());
    Ok(output)
}

pub(super) fn truncated_error() -> ApiError {
    ApiError::InvalidUpstream("stream ended before the [DONE] marker".to_owned())
}
