use crate::error::ApiError;
use crate::responses::request::{ToolCatalog, ToolTarget};
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

#[derive(Debug, Default)]
struct ToolState {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Debug, Default)]
struct StreamState {
    response_id: Option<String>,
    created: bool,
    text: String,
    reasoning: String,
    tools: BTreeMap<usize, ToolState>,
    input_tokens: u64,
    output_tokens: u64,
    reasoning_tokens: u64,
    usage: Option<UsageValues>,
}

pub(crate) fn translate(
    response: reqwest::Response,
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
        let mut failed = false;
        let mut terminated = false;

        while let Some(item) = source.next().await {
            let source_event = match item {
                Ok(event) => event,
                Err(error) => {
                    yield Ok(failed_event(&state, &map_sse_error(error)));
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
                    yield Ok(failed_event(&state, &error));
                    failed = true;
                    break;
                }
            };
            update_metadata(&mut state, &chunk);
            if !state.created {
                yield Ok(created_event(&state));
                state.created = true;
            }
            for choice in chunk.choices {
                if let Some(reasoning) = choice.delta.reasoning_content.filter(|content| !content.is_empty()) {
                    if state.reasoning.is_empty() {
                        yield Ok(reasoning_item_added_event());
                        yield Ok(reasoning_part_added_event());
                    }
                    state.reasoning.push_str(&reasoning);
                    yield Ok(responses_event("response.reasoning_summary_text.delta", &json!({
                        "type": "response.reasoning_summary_text.delta",
                        "item_id": "reasoning_nan_harness",
                        "output_index": 0,
                        "summary_index": 0,
                        "delta": reasoning
                    })));
                }
                if let Some(content) = choice.delta.content.filter(|content| !content.is_empty()) {
                    if state.text.is_empty() {
                        let output_index = usize::from(!state.reasoning.is_empty());
                        yield Ok(text_item_added_event(output_index));
                        yield Ok(text_content_part_added_event(output_index));
                    }
                    state.text.push_str(&content);
                    yield Ok(responses_event("response.output_text.delta", &json!({
                        "type": "response.output_text.delta",
                        "item_id": "msg_nan_harness",
                        "output_index": usize::from(!state.reasoning.is_empty()),
                        "content_index": 0,
                        "delta": content
                    })));
                }
                for tool_call in choice.delta.tool_calls {
                    update_tool(&mut state, tool_call);
                }
            }
        }

        if !failed && !terminated {
            yield Ok(failed_event(
                &state,
                &ApiError::InvalidUpstream(
                    "stream ended before the [DONE] marker".to_owned(),
                ),
            ));
        } else if !failed {
            if !state.created {
                yield Ok(created_event(&state));
            }
            match finish_events(&state, &tools) {
                Ok(events) => {
                    for event in events {
                        yield Ok(event);
                    }
                    usage_guard.complete(state.usage);
                }
                Err(error) => yield Ok(failed_event(&state, &error)),
            }
        }
    }
}

fn update_metadata(state: &mut StreamState, chunk: &Chunk) {
    if state.response_id.is_none() {
        state.response_id.clone_from(&chunk.id);
    }
    if let Some(usage) = &chunk.usage {
        state.input_tokens = usage.prompt_tokens;
        state.output_tokens = usage.completion_tokens;
        state.reasoning_tokens = usage
            .completion_tokens_details
            .as_ref()
            .map_or(0, |details| details.reasoning_tokens);
        state.usage = Some(UsageValues {
            input: state.input_tokens,
            output: state.output_tokens,
            reasoning: state.reasoning_tokens,
        });
    }
}

fn update_tool(state: &mut StreamState, delta: ToolCallDelta) {
    let tool = state.tools.entry(delta.index).or_default();
    if let Some(id) = delta.id {
        tool.id.push_str(&id);
    }
    if let Some(function) = delta.function {
        if let Some(name) = function.name {
            tool.name.push_str(&name);
        }
        if let Some(arguments) = function.arguments {
            tool.arguments.push_str(&arguments);
        }
    }
}

fn finish_events(state: &StreamState, tools: &ToolCatalog) -> Result<Vec<Event>, ApiError> {
    let mut events = finish_reasoning_events(state);
    events.extend(finish_text_events(state));
    events.extend(finish_tool_events(state, tools)?);
    events.push(completed_event(state));
    Ok(events)
}

fn finish_reasoning_events(state: &StreamState) -> Vec<Event> {
    if state.reasoning.is_empty() {
        return Vec::new();
    }
    vec![
        responses_event(
            "response.reasoning_summary_text.done",
            &json!({
                "type":"response.reasoning_summary_text.done", "item_id":"reasoning_nan_harness",
                "output_index":0, "summary_index":0, "text":state.reasoning
            }),
        ),
        responses_event(
            "response.reasoning_summary_part.done",
            &json!({
                "type":"response.reasoning_summary_part.done", "item_id":"reasoning_nan_harness",
                "output_index":0, "summary_index":0,
                "part":{"type":"summary_text","text":state.reasoning}
            }),
        ),
        responses_event(
            "response.output_item.done",
            &json!({
                "type":"response.output_item.done", "output_index":0,
                "item":{"type":"reasoning","id":"reasoning_nan_harness","summary":[{"type":"summary_text","text":state.reasoning}]}
            }),
        ),
    ]
}

fn finish_text_events(state: &StreamState) -> Vec<Event> {
    if state.text.is_empty() {
        return Vec::new();
    }
    let output_index = text_output_index(state);
    vec![
        responses_event(
            "response.output_text.done",
            &json!({
                "type": "response.output_text.done",
                "item_id": "msg_nan_harness",
                "output_index": output_index,
                "content_index": 0,
                "text": state.text
            }),
        ),
        responses_event(
            "response.content_part.done",
            &json!({
                "type": "response.content_part.done",
                "item_id": "msg_nan_harness",
                "output_index": output_index,
                "content_index": 0,
                "part": {"type": "output_text", "text": state.text, "annotations": []}
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
                    "content": [{"type": "output_text", "text": state.text, "annotations": []}]
                }
            }),
        ),
    ]
}

fn finish_tool_events(state: &StreamState, tools: &ToolCatalog) -> Result<Vec<Event>, ApiError> {
    state
        .tools
        .values()
        .map(|tool| {
            if tool.id.is_empty() || tool.name.is_empty() {
                return Err(ApiError::InvalidUpstream(
                    "tool call ended without an id and name".to_owned(),
                ));
            }
            Ok(tool_event(tool, tools))
        })
        .collect()
}

fn text_output_index(state: &StreamState) -> usize {
    usize::from(!state.reasoning.is_empty())
}

fn completed_event(state: &StreamState) -> Event {
    responses_event(
        "response.completed",
        &json!({
            "type": "response.completed",
            "response": {
                "id": response_id(state),
                "usage": {
                    "input_tokens": state.input_tokens,
                    "input_tokens_details": null,
                    "output_tokens": state.output_tokens,
                    "output_tokens_details": {"reasoning_tokens": state.reasoning_tokens},
                    "total_tokens": state.input_tokens.saturating_add(state.output_tokens)
                }
            }
        }),
    )
}

fn reasoning_item_added_event() -> Event {
    responses_event(
        "response.output_item.added",
        &json!({
            "type":"response.output_item.added", "output_index":0,
            "item":{"type":"reasoning","id":"reasoning_nan_harness","summary":[]}
        }),
    )
}

fn reasoning_part_added_event() -> Event {
    responses_event(
        "response.reasoning_summary_part.added",
        &json!({
            "type":"response.reasoning_summary_part.added", "item_id":"reasoning_nan_harness",
            "output_index":0, "summary_index":0, "part":{"type":"summary_text","text":""}
        }),
    )
}

fn text_item_added_event(output_index: usize) -> Event {
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

fn text_content_part_added_event(output_index: usize) -> Event {
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

fn tool_event(tool: &ToolState, tools: &ToolCatalog) -> Event {
    let item = match tools.target(&tool.name) {
        Some(ToolTarget::Function { name, namespace }) => {
            let mut item = json!({
                "type": "function_call",
                "call_id": tool.id,
                "name": name,
                "arguments": normalized_arguments(&tool.arguments)
            });
            if let Some(namespace) = namespace {
                item["namespace"] = Value::String(namespace.clone());
            }
            item
        }
        Some(ToolTarget::Custom { name }) => json!({
            "type": "custom_tool_call",
            "call_id": tool.id,
            "name": name,
            "input": custom_input(&tool.arguments)
        }),
        Some(ToolTarget::ToolSearch) => json!({
            "type": "tool_search_call",
            "call_id": tool.id,
            "execution": "client",
            "arguments": parsed_arguments(&tool.arguments)
        }),
        None => json!({
            "type": "function_call",
            "call_id": tool.id,
            "name": tool.name,
            "arguments": normalized_arguments(&tool.arguments)
        }),
    };
    responses_event(
        "response.output_item.done",
        &json!({"type": "response.output_item.done", "item": item}),
    )
}

fn normalized_arguments(arguments: &str) -> String {
    if serde_json::from_str::<Value>(arguments).is_ok() {
        arguments.to_owned()
    } else {
        json!({"input": arguments}).to_string()
    }
}

fn parsed_arguments(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|_| json!({"input": arguments}))
}

fn custom_input(arguments: &str) -> String {
    serde_json::from_str::<Value>(arguments)
        .ok()
        .and_then(|value| {
            value
                .get("input")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| arguments.to_owned())
}

fn created_event(state: &StreamState) -> Event {
    responses_event(
        "response.created",
        &json!({
            "type": "response.created",
            "response": {"id": response_id(state)}
        }),
    )
}

fn failed_event(state: &StreamState, error: &ApiError) -> Event {
    responses_event(
        "response.failed",
        &json!({
            "type": "response.failed",
            "response": {
                "id": response_id(state),
                "error": {
                    "code": "server_error",
                    "message": format!("{error} [{}]", error.code())
                }
            }
        }),
    )
}

fn response_id(state: &StreamState) -> &str {
    state.response_id.as_deref().unwrap_or("resp_nan_harness")
}

fn responses_event(name: &'static str, data: &Value) -> Event {
    Event::default().event(name).data(data.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        Chunk, StreamState, ToolState, custom_input, finish_events, parse_chunk, translate,
    };
    use crate::responses::request::ToolCatalog;
    use crate::stream_common::test_support::response;
    use crate::usage::{RequestUsageGuard, new_usage};
    use futures_util::StreamExt;

    fn usage_guard() -> RequestUsageGuard {
        RequestUsageGuard::new(&new_usage(), "qwen3.6")
    }

    #[test]
    fn extracts_freeform_input_from_chat_arguments() {
        assert_eq!(
            custom_input(r#"{"input":"*** Begin Patch"}"#),
            "*** Begin Patch"
        );
        assert_eq!(custom_input("raw patch"), "raw patch");
    }

    #[test]
    fn rejects_incomplete_tool_calls() {
        let mut state = StreamState::default();
        state.tools.insert(0, ToolState::default());
        assert!(finish_events(&state, &ToolCatalog::default()).is_err());
    }

    #[test]
    fn completes_reasoning_as_a_responses_reasoning_item() {
        let state = StreamState {
            reasoning: "Inspect before editing.".to_owned(),
            ..StreamState::default()
        };
        let events = finish_events(&state, &ToolCatalog::default()).expect("events");
        let rendered = format!("{events:?}");
        assert!(rendered.contains("response.reasoning_summary_text.done"));
        assert!(rendered.contains("Inspect before editing."));
        assert!(rendered.contains("\\\"type\\\":\\\"reasoning\\\""));
    }

    #[tokio::test]
    async fn reports_typed_upstream_error_before_processing_deltas() {
        let events = translate(
            response("data: {\"error\":{\"message\":\"typed boom\",\"type\":\"api_error\"}}\n\n"),
            ToolCatalog::default(),
            usage_guard(),
        )
        .collect::<Vec<_>>()
        .await;
        let rendered = format!("{events:?}");

        assert!(rendered.contains("event: response.failed"), "{rendered}");
        assert!(
            rendered.contains("typed boom [NH-BRIDGE-105]"),
            "{rendered}"
        );
        assert!(!rendered.contains("event: response.created"), "{rendered}");
    }

    #[tokio::test]
    async fn reports_fallback_upstream_error_before_processing_deltas() {
        let events = translate(
            response(
                "data: {\"error\":{\"message\":\"fallback boom\",\"type\":\"api_error\"},\"choices\":\"invalid\"}\n\n",
            ),
            ToolCatalog::default(),
            usage_guard(),
        )
        .collect::<Vec<_>>()
        .await;
        let rendered = format!("{events:?}");

        assert!(rendered.contains("event: response.failed"), "{rendered}");
        assert!(
            rendered.contains("fallback boom [NH-BRIDGE-105]"),
            "{rendered}"
        );
        assert!(!rendered.contains("invalid streaming chunk"), "{rendered}");
        assert!(!rendered.contains("event: response.created"), "{rendered}");
    }

    #[tokio::test]
    async fn reports_null_upstream_error_before_processing_deltas() {
        let events = translate(
            response("data: {\"error\":null}\n\n"),
            ToolCatalog::default(),
            usage_guard(),
        )
        .collect::<Vec<_>>()
        .await;
        let rendered = format!("{events:?}");

        assert!(rendered.contains("event: response.failed"), "{rendered}");
        assert!(
            rendered.contains("NaN returned a streaming error [NH-BRIDGE-105]"),
            "{rendered}"
        );
        assert!(!rendered.contains("event: response.created"), "{rendered}");
    }

    #[tokio::test]
    async fn preserves_invalid_streaming_json_error() {
        let events = translate(
            response("data: {not valid json}\n\n"),
            ToolCatalog::default(),
            usage_guard(),
        )
        .collect::<Vec<_>>()
        .await;
        let rendered = format!("{events:?}");

        assert!(rendered.contains("invalid streaming JSON:"), "{rendered}");
        assert!(rendered.contains("NH-BRIDGE-105"), "{rendered}");
        assert!(!rendered.contains("event: response.created"), "{rendered}");
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
                "data: {\"id\":\"resp_1\",\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
            ),
            ToolCatalog::default(),
            usage_guard(),
        )
        .collect::<Vec<_>>()
        .await;
        let rendered = format!("{events:?}");

        assert!(rendered.contains("event: response.failed"), "{rendered}");
        assert!(rendered.contains("stream ended before the [DONE] marker"));
        assert!(
            !rendered.contains("event: response.completed"),
            "{rendered}"
        );
    }

    #[tokio::test]
    async fn rejects_truncated_tool_stream_even_with_valid_arguments() {
        let events = translate(
            response(
                "data: {\"id\":\"resp_1\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"Read\",\"arguments\":\"{}\"}}]}}]}\n\n",
            ),
            ToolCatalog::default(),
            usage_guard(),
        )
        .collect::<Vec<_>>()
        .await;
        let rendered = format!("{events:?}");

        assert!(rendered.contains("event: response.failed"), "{rendered}");
        assert!(
            !rendered.contains("event: response.completed"),
            "{rendered}"
        );
    }

    #[tokio::test]
    async fn completes_stream_after_done_marker() {
        let events = translate(
            response(
                "data: {\"id\":\"resp_1\",\"choices\":[{\"delta\":{\"content\":\"complete\"}}]}\n\ndata: [DONE]\n\n",
            ),
            ToolCatalog::default(),
            usage_guard(),
        )
        .collect::<Vec<_>>()
        .await;
        let rendered = format!("{events:?}");

        assert!(rendered.contains("event: response.completed"), "{rendered}");
        assert!(!rendered.contains("event: response.failed"), "{rendered}");
    }
}
