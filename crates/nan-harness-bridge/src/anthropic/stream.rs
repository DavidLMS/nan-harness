use crate::anthropic::response::map_finish_reason;
use crate::error::ApiError;
use crate::stream_common::{StreamChunk, deserialize_error, parse_chunk};
use crate::timeouts::{STREAM_INACTIVITY_TIMEOUT, map_sse_error, with_inactivity_timeout};
use crate::usage::{RequestUsageGuard, UsageValues};
use async_stream::stream;
use axum::response::sse::Event;
use eventsource_stream::Eventsource;
use futures_util::{Stream, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::convert::Infallible;

#[derive(Debug, Deserialize)]
struct Chunk {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
    #[serde(default, deserialize_with = "deserialize_error")]
    error: Option<Value>,
}

impl StreamChunk for Chunk {
    fn stream_error(&self) -> Option<&Value> {
        self.error.as_ref()
    }
}

#[derive(Debug, Deserialize)]
struct Choice {
    #[serde(default)]
    delta: Delta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ToolCallDelta>,
}

#[derive(Debug, Deserialize)]
struct ToolCallDelta {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<FunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct FunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Usage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
    #[serde(default)]
    completion_tokens_details: Option<CompletionTokenDetails>,
}

#[derive(Debug, Default, Deserialize)]
struct CompletionTokenDetails {
    #[serde(default)]
    reasoning_tokens: u64,
}

#[derive(Debug)]
struct ToolState {
    content_index: usize,
    id: String,
    name: String,
    pending_arguments: String,
    started: bool,
}

#[derive(Debug, Default)]
struct StreamState {
    started: bool,
    text_index: Option<usize>,
    thinking_index: Option<usize>,
    tools: BTreeMap<usize, ToolState>,
    next_content_index: usize,
    message_id: Option<String>,
    finish_reason: Option<String>,
    input_tokens: u64,
    output_tokens: u64,
    usage: Option<UsageValues>,
}

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
                    yield Ok(error_event(&map_sse_error(error)));
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

            let chunk = match parse_chunk(&source_event.data) {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Ok(error_event(&error));
                    failed = true;
                    break;
                }
            };

            update_metadata(&mut state, &chunk);
            let mut events = Vec::new();
            if !state.started {
                events.push(message_start(&state, &configured_model));
                state.started = true;
            }
            for choice in chunk.choices {
                if let Some(reasoning) = choice.delta.reasoning_content.filter(|content| !content.is_empty()) {
                    push_thinking_delta(&mut state, &reasoning, &mut events);
                }
                if let Some(content) = choice.delta.content.filter(|content| !content.is_empty()) {
                    push_text_delta(&mut state, &content, &mut events);
                }
                for tool_call in choice.delta.tool_calls {
                    push_tool_delta(&mut state, tool_call, &mut events);
                }
                if choice.finish_reason.is_some() {
                    state.finish_reason = choice.finish_reason;
                }
            }
            for event in events {
                yield Ok(event);
            }
        }

        if !failed && !terminated {
            yield Ok(error_event(&ApiError::InvalidUpstream(
                "stream ended before the [DONE] marker".to_owned(),
            )));
        } else if !failed {
            if !state.started {
                yield Ok(message_start(&state, &configured_model));
            }
            match finish_events(&state) {
                Ok(events) => {
                    for event in events {
                        yield Ok(event);
                    }
                    usage_guard.complete(state.usage);
                }
                Err(error) => yield Ok(error_event(&error)),
            }
        }
    }
}

fn push_thinking_delta(state: &mut StreamState, content: &str, events: &mut Vec<Event>) {
    let index = if let Some(index) = state.thinking_index {
        index
    } else {
        let index = state.next_content_index;
        state.next_content_index += 1;
        state.thinking_index = Some(index);
        events.push(anthropic_event(
            "content_block_start",
            &json!({
                "type": "content_block_start",
                "index": index,
                "content_block": {"type": "thinking", "thinking": "", "signature": ""}
            }),
        ));
        index
    };
    events.push(anthropic_event(
        "content_block_delta",
        &json!({
            "type": "content_block_delta",
            "index": index,
            "delta": {"type": "thinking_delta", "thinking": content}
        }),
    ));
}

fn update_metadata(state: &mut StreamState, chunk: &Chunk) {
    if state.message_id.is_none() {
        state.message_id.clone_from(&chunk.id);
    }
    if let Some(usage) = &chunk.usage {
        state.input_tokens = usage.prompt_tokens;
        state.output_tokens = usage.completion_tokens;
        state.usage = Some(UsageValues {
            input: usage.prompt_tokens,
            output: usage.completion_tokens,
            reasoning: usage
                .completion_tokens_details
                .as_ref()
                .map_or(0, |details| details.reasoning_tokens),
        });
    }
}

fn message_start(state: &StreamState, configured_model: &str) -> Event {
    anthropic_event(
        "message_start",
        &json!({
            "type": "message_start",
            "message": {
                "id": state.message_id.as_deref().unwrap_or("msg_nan_harness"),
                "type": "message",
                "role": "assistant",
                "model": configured_model,
                "content": [],
                "stop_reason": null,
                "stop_sequence": null,
                "usage": {"input_tokens": state.input_tokens, "output_tokens": 0}
            }
        }),
    )
}

fn push_text_delta(state: &mut StreamState, content: &str, events: &mut Vec<Event>) {
    let index = if let Some(index) = state.text_index {
        index
    } else {
        let index = state.next_content_index;
        state.next_content_index += 1;
        state.text_index = Some(index);
        events.push(anthropic_event(
            "content_block_start",
            &json!({
                "type": "content_block_start",
                "index": index,
                "content_block": {"type": "text", "text": ""}
            }),
        ));
        index
    };
    events.push(anthropic_event(
        "content_block_delta",
        &json!({
            "type": "content_block_delta",
            "index": index,
            "delta": {"type": "text_delta", "text": content}
        }),
    ));
}

fn push_tool_delta(state: &mut StreamState, delta: ToolCallDelta, events: &mut Vec<Event>) {
    let tool = state.tools.entry(delta.index).or_insert_with(|| {
        let content_index = state.next_content_index;
        state.next_content_index += 1;
        ToolState {
            content_index,
            id: String::new(),
            name: String::new(),
            pending_arguments: String::new(),
            started: false,
        }
    });
    if let Some(id) = delta.id {
        tool.id.push_str(&id);
    }
    if let Some(function) = delta.function {
        if let Some(name) = function.name {
            tool.name.push_str(&name);
        }
        if let Some(arguments) = function.arguments {
            tool.pending_arguments.push_str(&arguments);
        }
    }

    if !tool.started && !tool.id.is_empty() && !tool.name.is_empty() {
        events.push(anthropic_event(
            "content_block_start",
            &json!({
                "type": "content_block_start",
                "index": tool.content_index,
                "content_block": {
                    "type": "tool_use",
                    "id": tool.id,
                    "name": tool.name,
                    "input": {}
                }
            }),
        ));
        tool.started = true;
    }
    if tool.started && !tool.pending_arguments.is_empty() {
        events.push(anthropic_event(
            "content_block_delta",
            &json!({
                "type": "content_block_delta",
                "index": tool.content_index,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": std::mem::take(&mut tool.pending_arguments)
                }
            }),
        ));
    }
}

fn finish_events(state: &StreamState) -> Result<Vec<Event>, ApiError> {
    if let Some(tool) = state.tools.values().find(|tool| !tool.started) {
        return Err(ApiError::InvalidUpstream(format!(
            "tool call {} ended without an id and name",
            tool.content_index
        )));
    }

    let mut indexes = state
        .thinking_index
        .into_iter()
        .chain(state.text_index)
        .chain(state.tools.values().map(|tool| tool.content_index))
        .collect::<Vec<_>>();
    indexes.sort_unstable();
    let mut events = indexes
        .into_iter()
        .map(|index| {
            anthropic_event(
                "content_block_stop",
                &json!({"type": "content_block_stop", "index": index}),
            )
        })
        .collect::<Vec<_>>();
    events.push(anthropic_event(
        "message_delta",
        &json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": map_finish_reason(
                    state.finish_reason.as_deref(),
                    !state.tools.is_empty()
                ),
                "stop_sequence": null
            },
            "usage": {"output_tokens": state.output_tokens}
        }),
    ));
    events.push(anthropic_event(
        "message_stop",
        &json!({"type": "message_stop"}),
    ));
    Ok(events)
}

fn anthropic_event(name: &'static str, data: &Value) -> Event {
    Event::default().event(name).data(data.to_string())
}

fn error_event(error: &ApiError) -> Event {
    anthropic_event("error", &error.event_data())
}

#[cfg(test)]
mod tests {
    use super::{
        Chunk, StreamState, ToolCallDelta, finish_events, parse_chunk, push_text_delta,
        push_tool_delta, translate,
    };
    use crate::stream_common::test_support::response;
    use crate::usage::{RequestUsageGuard, new_usage};
    use futures_util::StreamExt;
    use serde_json::from_str;

    fn usage_guard() -> RequestUsageGuard {
        RequestUsageGuard::new(&new_usage(), "qwen3.6")
    }

    #[test]
    fn orders_text_and_tool_events() {
        let mut state = StreamState::default();
        let mut events = Vec::new();
        push_text_delta(&mut state, "Reading", &mut events);
        let delta: ToolCallDelta = from_str(
            r#"{"index":0,"id":"call_1","function":{"name":"Read","arguments":"{\"file_path\":"}}"#,
        )
        .expect("tool delta should deserialize");
        push_tool_delta(&mut state, delta, &mut events);
        let delta: ToolCallDelta =
            from_str(r#"{"index":0,"function":{"arguments":"\"README.md\"}"}}"#)
                .expect("tool delta should deserialize");
        push_tool_delta(&mut state, delta, &mut events);

        assert_eq!(events.len(), 5);
        let finished = finish_events(&state).expect("stream should finish");
        assert_eq!(finished.len(), 4);
    }

    #[tokio::test]
    async fn reports_typed_upstream_error_before_processing_deltas() {
        let events = translate(
            response("data: {\"error\":{\"message\":\"typed boom\",\"type\":\"api_error\"}}\n\n"),
            "qwen3.6".to_owned(),
            usage_guard(),
        )
        .collect::<Vec<_>>()
        .await;
        let rendered = format!("{events:?}");

        assert!(rendered.contains("event: error"), "{rendered}");
        assert!(
            rendered.contains("typed boom [NH-BRIDGE-105]"),
            "{rendered}"
        );
        assert!(!rendered.contains("event: message_start"), "{rendered}");
    }

    #[tokio::test]
    async fn reports_fallback_upstream_error_before_processing_deltas() {
        let events = translate(
            response(
                "data: {\"error\":{\"message\":\"fallback boom\",\"type\":\"api_error\"},\"choices\":\"invalid\"}\n\n",
            ),
            "qwen3.6".to_owned(),
            usage_guard(),
        )
        .collect::<Vec<_>>()
        .await;
        let rendered = format!("{events:?}");

        assert!(rendered.contains("event: error"), "{rendered}");
        assert!(
            rendered.contains("fallback boom [NH-BRIDGE-105]"),
            "{rendered}"
        );
        assert!(!rendered.contains("invalid streaming chunk"), "{rendered}");
        assert!(!rendered.contains("event: message_start"), "{rendered}");
    }

    #[tokio::test]
    async fn reports_null_upstream_error_before_processing_deltas() {
        let events = translate(
            response("data: {\"error\":null}\n\n"),
            "qwen3.6".to_owned(),
            usage_guard(),
        )
        .collect::<Vec<_>>()
        .await;
        let rendered = format!("{events:?}");

        assert!(rendered.contains("event: error"), "{rendered}");
        assert!(
            rendered.contains("NaN returned a streaming error [NH-BRIDGE-105]"),
            "{rendered}"
        );
        assert!(!rendered.contains("event: message_start"), "{rendered}");
    }

    #[tokio::test]
    async fn preserves_invalid_streaming_json_error() {
        let events = translate(
            response("data: {not valid json}\n\n"),
            "qwen3.6".to_owned(),
            usage_guard(),
        )
        .collect::<Vec<_>>()
        .await;
        let rendered = format!("{events:?}");

        assert!(rendered.contains("invalid streaming JSON:"), "{rendered}");
        assert!(rendered.contains("NH-BRIDGE-105"), "{rendered}");
        assert!(!rendered.contains("event: message_start"), "{rendered}");
    }

    #[test]
    fn preserves_invalid_streaming_chunk_error() {
        let error =
            parse_chunk::<Chunk>(r#"{"choices":"invalid"}"#).expect_err("chunk should fail");
        assert!(
            error
                .to_string()
                .starts_with("NaN returned an invalid response: invalid streaming chunk:")
        );
        assert_eq!(error.code(), "NH-BRIDGE-105");
    }

    #[tokio::test]
    async fn rejects_truncated_text_stream() {
        let events = translate(
            response(
                "data: {\"id\":\"msg_1\",\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
            ),
            "qwen3.6".to_owned(),
            usage_guard(),
        )
        .collect::<Vec<_>>()
        .await;
        let rendered = format!("{events:?}");

        assert!(rendered.contains("event: error"), "{rendered}");
        assert!(rendered.contains("stream ended before the [DONE] marker"));
        assert!(!rendered.contains("event: message_stop"), "{rendered}");
    }

    #[tokio::test]
    async fn rejects_truncated_tool_stream_even_with_valid_arguments() {
        let events = translate(
            response(
                "data: {\"id\":\"msg_1\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"Read\",\"arguments\":\"{}\"}}]}}]}\n\n",
            ),
            "qwen3.6".to_owned(),
            usage_guard(),
        )
        .collect::<Vec<_>>()
        .await;
        let rendered = format!("{events:?}");

        assert!(rendered.contains("event: error"), "{rendered}");
        assert!(!rendered.contains("event: message_stop"), "{rendered}");
    }

    #[tokio::test]
    async fn completes_stream_after_done_marker() {
        let events = translate(
            response(
                "data: {\"id\":\"msg_1\",\"choices\":[{\"delta\":{\"content\":\"complete\"}}]}\n\ndata: [DONE]\n\n",
            ),
            "qwen3.6".to_owned(),
            usage_guard(),
        )
        .collect::<Vec<_>>()
        .await;
        let rendered = format!("{events:?}");

        assert!(rendered.contains("event: message_stop"), "{rendered}");
        assert!(!rendered.contains("event: error"), "{rendered}");
    }
}
