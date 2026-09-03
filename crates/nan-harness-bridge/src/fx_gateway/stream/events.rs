use crate::error::ApiError;
use axum::response::sse::Event;
use serde_json::{Value, json};

pub(super) fn event(value: &Value) -> Event {
    Event::default().data(value.to_string())
}

pub(super) fn response_metadata(model_id: &str) -> Event {
    event(&json!({
        "type": "response-metadata",
        "modelId": model_id
    }))
}

pub(super) fn api_error(error: &ApiError) -> Event {
    error_message(&format!("{error} [{}]", error.code()))
}

pub(super) fn error_message(message: &str) -> Event {
    event(&json!({"type":"error","error":{"type":"api-error","message":message}}))
}

pub(super) fn reasoning_start() -> Event {
    event(&json!({"type":"reasoning-start","id":"fx_reasoning"}))
}

pub(super) fn reasoning_delta(reasoning: &str) -> Event {
    event(&json!({"type":"reasoning-delta","id":"fx_reasoning","delta":reasoning}))
}

pub(super) fn reasoning_end() -> Event {
    event(&json!({"type":"reasoning-end","id":"fx_reasoning"}))
}

pub(super) fn text_start() -> Event {
    event(&json!({"type":"text-start","id":"fx_text"}))
}

pub(super) fn text_delta(text: &str) -> Event {
    event(&json!({"type":"text-delta","id":"fx_text","delta":text}))
}

pub(super) fn text_end() -> Event {
    event(&json!({"type":"text-end","id":"fx_text"}))
}

pub(super) fn finish(
    model_id: &str,
    finish_reason: &Value,
    input_tokens: u64,
    output_tokens: u64,
) -> Event {
    event(&json!({
        "type":"finish",
        "finishReason":finish_reason,
        "usage": {
            "inputTokens": {"total": input_tokens},
            "outputTokens": {"total": output_tokens}
        },
        "providerMetadata": {"gateway": {"routing": {"canonicalSlug": model_id}}}
    }))
}
