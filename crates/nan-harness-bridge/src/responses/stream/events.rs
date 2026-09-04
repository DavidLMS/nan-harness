use super::state::StreamState;
use crate::error::ApiError;
use axum::response::sse::Event;
use serde_json::{Value, json};

pub(super) fn created(state: &StreamState) -> Event {
    responses_event(
        "response.created",
        &json!({
            "type": "response.created",
            "response": {"id": state.response_id()}
        }),
    )
}

pub(super) fn in_progress(state: &StreamState) -> Event {
    responses_event(
        "response.in_progress",
        &json!({
            "type": "response.in_progress",
            "response": {"id": state.response_id(), "status": "in_progress"}
        }),
    )
}

pub(super) fn failed(state: &StreamState, error: &ApiError) -> Event {
    responses_event(
        "response.failed",
        &json!({
            "type": "response.failed",
            "response": {
                "id": state.response_id(),
                "error": {
                    "code": "server_error",
                    "message": format!("{error} [{}]", error.code())
                }
            }
        }),
    )
}

pub(super) fn reasoning_item_added() -> Event {
    responses_event(
        "response.output_item.added",
        &json!({
            "type":"response.output_item.added", "output_index":0,
            "item":{"type":"reasoning","id":"reasoning_nan_harness","summary":[]}
        }),
    )
}

pub(super) fn reasoning_part_added() -> Event {
    responses_event(
        "response.reasoning_summary_part.added",
        &json!({
            "type":"response.reasoning_summary_part.added", "item_id":"reasoning_nan_harness",
            "output_index":0, "summary_index":0, "part":{"type":"summary_text","text":""}
        }),
    )
}

pub(super) fn reasoning_delta(reasoning: &str) -> Event {
    responses_event(
        "response.reasoning_summary_text.delta",
        &json!({
            "type": "response.reasoning_summary_text.delta",
            "item_id": "reasoning_nan_harness",
            "output_index": 0,
            "summary_index": 0,
            "delta": reasoning
        }),
    )
}

pub(super) fn text_item_added(output_index: usize) -> Event {
    responses_event(
        "response.output_item.added",
        &json!({
            "type": "response.output_item.added",
            "output_index": output_index,
            "item": {
                "type": "message",
                "id": "msg_nan_harness",
                "status": "in_progress",
                "role": "assistant",
                "content": []
            }
        }),
    )
}

pub(super) fn text_content_part_added(output_index: usize) -> Event {
    responses_event(
        "response.content_part.added",
        &json!({
            "type": "response.content_part.added",
            "item_id": "msg_nan_harness",
            "output_index": output_index,
            "content_index": 0,
            "part": {"type": "output_text", "text": "", "annotations": []}
        }),
    )
}

pub(super) fn text_delta(output_index: usize, content: &str) -> Event {
    responses_event(
        "response.output_text.delta",
        &json!({
            "type": "response.output_text.delta",
            "item_id": "msg_nan_harness",
            "output_index": output_index,
            "content_index": 0,
            "delta": content
        }),
    )
}

pub(super) fn finish_reasoning(state: &StreamState) -> Vec<Event> {
    if state.reasoning().is_empty() {
        return Vec::new();
    }
    vec![
        responses_event(
            "response.reasoning_summary_text.done",
            &json!({
                "type":"response.reasoning_summary_text.done", "item_id":"reasoning_nan_harness",
                "output_index":0, "summary_index":0, "text":state.reasoning()
            }),
        ),
        responses_event(
            "response.reasoning_summary_part.done",
            &json!({
                "type":"response.reasoning_summary_part.done", "item_id":"reasoning_nan_harness",
                "output_index":0, "summary_index":0,
                "part":{"type":"summary_text","text":state.reasoning()}
            }),
        ),
        responses_event(
            "response.output_item.done",
            &json!({
                "type":"response.output_item.done", "output_index":0,
                "item":{"type":"reasoning","id":"reasoning_nan_harness","summary":[{"type":"summary_text","text":state.reasoning()}]}
            }),
        ),
    ]
}

pub(super) fn finish_text(state: &StreamState) -> Vec<Event> {
    if state.text().is_empty() {
        return Vec::new();
    }
    let output_index = state.text_output_index();
    vec![
        responses_event(
            "response.output_text.done",
            &json!({
                "type": "response.output_text.done",
                "item_id": "msg_nan_harness",
                "output_index": output_index,
                "content_index": 0,
                "text": state.text()
            }),
        ),
        responses_event(
            "response.content_part.done",
            &json!({
                "type": "response.content_part.done",
                "item_id": "msg_nan_harness",
                "output_index": output_index,
                "content_index": 0,
                "part": {"type": "output_text", "text": state.text(), "annotations": []}
            }),
        ),
        responses_event(
            "response.output_item.done",
            &json!({
                "type": "response.output_item.done",
                "output_index": output_index,
                "item": {
                    "type": "message",
                    "id": "msg_nan_harness",
                    "status": "completed",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": state.text(), "annotations": []}]
                }
            }),
        ),
    ]
}

pub(super) fn completed(state: &StreamState) -> Event {
    responses_event(
        "response.completed",
        &json!({
            "type": "response.completed",
            "response": {
                "id": state.response_id(),
                "usage": {
                    "input_tokens": state.input_tokens(),
                    "input_tokens_details": null,
                    "output_tokens": state.output_tokens(),
                    "output_tokens_details": {"reasoning_tokens": state.reasoning_tokens()},
                    "total_tokens": state.input_tokens().saturating_add(state.output_tokens())
                }
            }
        }),
    )
}

pub(super) fn responses_event(name: &'static str, data: &Value) -> Event {
    Event::default().event(name).data(data.to_string())
}
