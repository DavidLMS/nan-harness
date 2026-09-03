use super::state::{StreamState, ToolState};
use crate::anthropic::response::map_finish_reason;
use crate::error::ApiError;
use axum::response::sse::Event;
use serde_json::{Value, json};

pub(super) fn message_start(state: &StreamState, configured_model: &str) -> Event {
    anthropic_event(
        "message_start",
        &json!({
            "type": "message_start",
            "message": {
                "id": state.message_id(),
                "type": "message",
                "role": "assistant",
                "model": configured_model,
                "content": [],
                "stop_reason": null,
                "stop_sequence": null,
                "usage": {"input_tokens": state.input_tokens(), "output_tokens": 0}
            }
        }),
    )
}

pub(super) fn thinking_start(index: usize) -> Event {
    anthropic_event(
        "content_block_start",
        &json!({
            "type": "content_block_start",
            "index": index,
            "content_block": {"type": "thinking", "thinking": "", "signature": ""}
        }),
    )
}

pub(super) fn thinking_delta(index: usize, content: &str) -> Event {
    anthropic_event(
        "content_block_delta",
        &json!({
            "type": "content_block_delta",
            "index": index,
            "delta": {"type": "thinking_delta", "thinking": content}
        }),
    )
}

pub(super) fn text_start(index: usize) -> Event {
    anthropic_event(
        "content_block_start",
        &json!({
            "type": "content_block_start",
            "index": index,
            "content_block": {"type": "text", "text": ""}
        }),
    )
}

pub(super) fn text_delta(index: usize, content: &str) -> Event {
    anthropic_event(
        "content_block_delta",
        &json!({
            "type": "content_block_delta",
            "index": index,
            "delta": {"type": "text_delta", "text": content}
        }),
    )
}

pub(super) fn tool_start(tool: &ToolState) -> Event {
    anthropic_event(
        "content_block_start",
        &json!({
            "type": "content_block_start",
            "index": tool.content_index(),
            "content_block": {
                "type": "tool_use",
                "id": tool.id(),
                "name": tool.name(),
                "input": {}
            }
        }),
    )
}

pub(super) fn tool_delta(index: usize, partial_json: &str) -> Event {
    anthropic_event(
        "content_block_delta",
        &json!({
            "type": "content_block_delta",
            "index": index,
            "delta": {
                "type": "input_json_delta",
                "partial_json": partial_json
            }
        }),
    )
}

pub(super) fn content_stop(index: usize) -> Event {
    anthropic_event(
        "content_block_stop",
        &json!({"type": "content_block_stop", "index": index}),
    )
}

pub(super) fn message_delta(state: &StreamState) -> Event {
    anthropic_event(
        "message_delta",
        &json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": map_finish_reason(state.finish_reason(), state.has_tools()),
                "stop_sequence": null
            },
            "usage": {"output_tokens": state.output_tokens()}
        }),
    )
}

pub(super) fn message_stop() -> Event {
    anthropic_event("message_stop", &json!({"type": "message_stop"}))
}

pub(super) fn error(error: &ApiError) -> Event {
    anthropic_event("error", &error.event_data())
}

fn anthropic_event(name: &'static str, data: &Value) -> Event {
    Event::default().event(name).data(data.to_string())
}
