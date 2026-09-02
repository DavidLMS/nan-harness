use super::request::ProviderSearchTool;
use crate::error::ApiError;
use crate::search_service::{self, SearchRequest};
use crate::stream_common::{StreamChunk, deserialize_error, parse_chunk};
use crate::timeouts::{STREAM_INACTIVITY_TIMEOUT, map_sse_error, with_inactivity_timeout};
use crate::upstream::NanClient;
use crate::usage::{RequestUsageGuard, UsageValues};
use async_stream::stream;
use axum::response::sse::Event;
use eventsource_stream::Eventsource;
use futures_util::{Stream, StreamExt};
use serde::Deserialize;
use serde::de::value::MapAccessDeserializer;
use serde::de::{MapAccess, Visitor};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::marker::PhantomData;

pub(super) fn translate(
    response: reqwest::Response,
    model_id: String,
    upstream: NanClient,
    provider_search: Option<ProviderSearchTool>,
    fallback_query: String,
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
        let mut state = FxStreamState::new(model_id);
        let mut failed = false;
        let mut terminated = false;
        yield Ok(FxStreamState::event(&json!({
            "type": "response-metadata",
            "modelId": state.model_id.clone()
        })));
        while let Some(item) = source.next().await {
            let event = match item {
                Ok(event) => event,
                Err(error) => {
                    let error = map_sse_error(error);
                    yield Ok(FxStreamState::error_event(&format!(
                        "{error} [{}]", error.code()
                    )));
                    failed = true;
                    break;
                }
            };
            if event.data.trim() == "[DONE]" {
                terminated = true;
                break;
            }
            if event.data.trim().is_empty() {
                continue;
            }
            let chunk = match parse_chunk::<FxObject<FxChunk>>(&event.data) {
                Ok(FxObject(chunk)) => chunk,
                Err(error) => {
                    yield Ok(FxStreamState::error_event(&format!(
                        "{error} [{}]", error.code()
                    )));
                    failed = true;
                    break;
                }
            };
            for FxObject(choice) in chunk.choices {
                let FxObject(delta) = choice.delta;
                if let Some(reasoning) = delta.reasoning_content.filter(|text| !text.is_empty()) {
                    if !state.reasoning_started {
                        yield Ok(FxStreamState::event(&json!({"type":"reasoning-start","id":"fx_reasoning"})));
                        state.reasoning_started = true;
                    }
                    yield Ok(FxStreamState::event(&json!({"type":"reasoning-delta","id":"fx_reasoning","delta":reasoning})));
                }
                if let Some(text) = delta.content.filter(|text| !text.is_empty()) {
                    if !state.text_started {
                        yield Ok(FxStreamState::event(&json!({"type":"text-start","id":"fx_text"})));
                        state.text_started = true;
                    }
                    yield Ok(FxStreamState::event(&json!({"type":"text-delta","id":"fx_text","delta":text})));
                }
                for FxObject(call) in delta.tool_calls {
                    state.update_tool(call);
                }
                if choice.finish_reason.is_some() {
                    state.finish_reason = choice.finish_reason;
                }
            }
            if let Some(FxObject(usage)) = chunk.usage {
                state.update_usage(usage);
            }
        }
        if !failed {
            if terminated {
                match state
                    .finish_events(&upstream, provider_search.as_ref(), &fallback_query)
                    .await
                {
                    Ok(events) => {
                        for event in events {
                            yield Ok(event);
                        }
                        usage_guard.complete(state.usage);
                    }
                    Err(error) => yield Ok(FxStreamState::error_event(&format!(
                        "{error} [{}]",
                        error.code()
                    ))),
                }
            } else {
                yield Ok(FxStreamState::error_event(
                    "stream ended before the [DONE] marker",
                ));
            }
        }
    }
}

#[derive(Debug, Default)]
struct FxObject<T>(T);

struct FxObjectVisitor<T>(PhantomData<T>);

impl<'de, T> Visitor<'de> for FxObjectVisitor<T>
where
    T: Deserialize<'de>,
{
    type Value = FxObject<T>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON object")
    }

    fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        T::deserialize(MapAccessDeserializer::new(map)).map(FxObject)
    }
}

impl<'de, T> Deserialize<'de> for FxObject<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(FxObjectVisitor(PhantomData))
    }
}

#[derive(Debug, Deserialize)]
struct FxChunk {
    #[serde(default)]
    choices: Vec<FxObject<FxChoice>>,
    #[serde(default)]
    usage: Option<FxObject<FxUsage>>,
    #[serde(default, deserialize_with = "deserialize_error")]
    error: Option<Value>,
}

impl StreamChunk for FxObject<FxChunk> {
    fn stream_error(&self) -> Option<&Value> {
        self.0.error.as_ref()
    }
}

#[derive(Debug, Deserialize)]
struct FxChoice {
    #[serde(default)]
    delta: FxObject<FxDelta>,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct FxDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<FxObject<FxToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct FxToolCallDelta {
    #[serde(default)]
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<FxObject<FxFunctionDelta>>,
}

#[derive(Debug, Deserialize)]
struct FxFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FxUsage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
    #[serde(default)]
    completion_tokens_details: Option<FxObject<FxCompletionTokenDetails>>,
}

#[derive(Debug, Deserialize)]
struct FxCompletionTokenDetails {
    #[serde(default)]
    reasoning_tokens: u64,
}

struct FxToolState {
    id: String,
    name: String,
    arguments: String,
}

struct ParsedFxTool {
    id: String,
    name: String,
    input: Value,
}

struct FxStreamState {
    model_id: String,
    text_started: bool,
    reasoning_started: bool,
    tools: BTreeMap<usize, FxToolState>,
    finish_reason: Option<String>,
    input_tokens: u64,
    output_tokens: u64,
    usage: Option<UsageValues>,
}

impl FxStreamState {
    fn new(model_id: String) -> Self {
        Self {
            model_id,
            text_started: false,
            reasoning_started: false,
            tools: BTreeMap::new(),
            finish_reason: None,
            input_tokens: 0,
            output_tokens: 0,
            usage: None,
        }
    }

    fn event(value: &Value) -> Event {
        Event::default().data(value.to_string())
    }

    fn error_event(message: &str) -> Event {
        Self::event(&json!({"type":"error","error":{"type":"api-error","message":message}}))
    }

    fn update_tool(&mut self, call: FxToolCallDelta) {
        let tool = self.tools.entry(call.index).or_insert_with(|| FxToolState {
            id: String::new(),
            name: String::new(),
            arguments: String::new(),
        });
        if let Some(id) = call.id {
            tool.id.push_str(&id);
        }
        if let Some(FxObject(function)) = call.function {
            if let Some(name) = function.name {
                tool.name.push_str(&name);
            }
            if let Some(arguments) = function.arguments {
                tool.arguments.push_str(&arguments);
            }
        }
    }

    fn update_usage(&mut self, usage: FxUsage) {
        let usage = UsageValues {
            input: usage.prompt_tokens,
            output: usage.completion_tokens,
            reasoning: usage
                .completion_tokens_details
                .map_or(0, |FxObject(details)| details.reasoning_tokens),
        };
        self.input_tokens = usage.input;
        self.output_tokens = usage.output;
        self.usage = Some(usage);
    }

    async fn finish_events(
        &self,
        upstream: &NanClient,
        provider_search: Option<&ProviderSearchTool>,
        fallback_query: &str,
    ) -> Result<Vec<Event>, ApiError> {
        let parsed_tools = self.parse_tools()?;
        let mut events = self.reasoning_events();
        events.extend(self.text_events());
        events.extend(
            self.tool_events(upstream, provider_search, fallback_query, parsed_tools)
                .await,
        );
        events.push(self.finish_event(provider_search));
        Ok(events)
    }

    fn parse_tools(&self) -> Result<Vec<ParsedFxTool>, ApiError> {
        self.tools
            .values()
            .map(|tool| {
                if tool.id.trim().is_empty() || tool.name.trim().is_empty() {
                    return Err(ApiError::InvalidUpstream(
                        "tool call ended without a valid id or name".to_owned(),
                    ));
                }
                let input = serde_json::from_str::<Value>(&tool.arguments)
                    .ok()
                    .filter(Value::is_object)
                    .ok_or_else(|| {
                        ApiError::InvalidUpstream(
                            "tool call ended with invalid JSON object arguments".to_owned(),
                        )
                    })?;
                Ok(ParsedFxTool {
                    id: tool.id.clone(),
                    name: tool.name.clone(),
                    input,
                })
            })
            .collect()
    }

    fn reasoning_events(&self) -> Vec<Event> {
        if self.reasoning_started {
            vec![Self::event(
                &json!({"type":"reasoning-end","id":"fx_reasoning"}),
            )]
        } else {
            Vec::new()
        }
    }

    fn text_events(&self) -> Vec<Event> {
        if self.text_started {
            vec![Self::event(&json!({"type":"text-end","id":"fx_text"}))]
        } else {
            Vec::new()
        }
    }

    async fn tool_events(
        &self,
        upstream: &NanClient,
        provider_search: Option<&ProviderSearchTool>,
        fallback_query: &str,
        parsed_tools: Vec<ParsedFxTool>,
    ) -> Vec<Event> {
        let mut events = Vec::new();
        for tool in parsed_tools {
            let provider_search = provider_search.filter(|search| search.name == tool.name);
            let mut tool_event = json!({
                "type":"tool-call",
                "toolCallId":tool.id,
                "toolName":tool.name,
                "input":tool.input
            });
            if provider_search.is_some() {
                tool_event["providerExecuted"] = json!(true);
            }
            events.push(Self::event(&tool_event));
            if let Some(search) = provider_search {
                let query = provider_search_query(&tool_event["input"], fallback_query);
                let result = execute_provider_search(upstream, search, query).await;
                events.push(Self::event(&json!({
                    "type":"tool-result",
                    "toolCallId":tool_event["toolCallId"],
                    "result":result
                })));
            }
        }
        events
    }

    fn finish_event(&self, provider_search: Option<&ProviderSearchTool>) -> Event {
        Self::event(&json!({
            "type":"finish",
            "finishReason":self.finish_reason(provider_search),
            "usage": {
                "inputTokens": {"total": self.input_tokens},
                "outputTokens": {"total": self.output_tokens}
            },
            "providerMetadata": {"gateway": {"routing": {"canonicalSlug": self.model_id}}}
        }))
    }

    fn finish_reason(&self, provider_search: Option<&ProviderSearchTool>) -> Value {
        let has_provider_search = self.has_provider_search(provider_search);
        if has_provider_search && self.all_tools_are_provider_search(provider_search) {
            json!({"unified":"stop"})
        } else if self.tools.is_empty() {
            match self.finish_reason.as_deref() {
                Some("length") => json!({"unified":"length"}),
                _ => json!({"unified":"stop"}),
            }
        } else {
            json!({"unified":"tool-calls"})
        }
    }

    fn has_provider_search(&self, provider_search: Option<&ProviderSearchTool>) -> bool {
        self.tools
            .values()
            .any(|tool| provider_search.is_some_and(|search| search.name == tool.name))
    }

    fn all_tools_are_provider_search(&self, provider_search: Option<&ProviderSearchTool>) -> bool {
        self.tools
            .values()
            .all(|tool| provider_search.is_some_and(|search| search.name == tool.name))
    }
}

fn provider_search_query<'a>(input: &'a Value, fallback_query: &'a str) -> &'a str {
    input
        .get("query")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback_query)
}

async fn execute_provider_search(
    upstream: &NanClient,
    provider: &ProviderSearchTool,
    query: &str,
) -> Value {
    match search_service::execute(
        upstream,
        SearchRequest {
            query: query.to_owned(),
            max_results: provider.max_results,
            allowed_domains: provider.allowed_domains.clone(),
            blocked_domains: provider.blocked_domains.clone(),
        },
    )
    .await
    {
        Ok(results) => json!({"results": results}),
        Err(error) => {
            json!({
                "error": {
                    "type": "search_failed",
                    "message": format!("web search request failed [{}]", error.code())
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::translate;
    use crate::stream_common::test_support::response;
    use crate::upstream::NanClient;
    use crate::usage::{ProviderUsageSnapshot, RequestUsageGuard, new_usage, snapshot};
    use futures_util::StreamExt;
    use nan_harness_core::SecretValue;
    use serde_json::json;
    use std::sync::Arc;

    fn upstream() -> NanClient {
        NanClient::new(
            "http://127.0.0.1",
            Arc::new(SecretValue::new("test-provider-key").expect("test key should be valid")),
        )
        .expect("test upstream should build")
    }

    fn usage_guard() -> RequestUsageGuard {
        RequestUsageGuard::new(&new_usage(), "qwen3.6")
    }

    async fn render_stream(body: &str) -> (String, ProviderUsageSnapshot) {
        let usage = new_usage();
        let events = translate(
            response(body),
            "qwen3.6".to_owned(),
            upstream(),
            None,
            "fallback query".to_owned(),
            RequestUsageGuard::new(&usage, "qwen3.6"),
        )
        .collect::<Vec<_>>()
        .await;
        (format!("{events:?}"), snapshot(&usage))
    }

    fn assert_failed_stream(label: &str, rendered: &str, usage: &ProviderUsageSnapshot) {
        assert!(rendered.contains("api-error"), "{label}: {rendered}");
        assert!(rendered.contains("NH-BRIDGE-105"), "{label}: {rendered}");
        assert!(!rendered.contains("finishReason"), "{label}: {rendered}");
        assert_eq!(usage.completed_requests(), 0, "{label}: {usage:?}");
    }

    #[tokio::test]
    async fn typed_stream_preserves_reasoning_text_usage_and_event_order() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"think\",",
            "\"content\":\"answer\"},\"finish_reason\":\"stop\"}],",
            "\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":5,",
            "\"completion_tokens_details\":{\"reasoning_tokens\":2}}}\n\n",
            "data: [DONE]\n\n"
        );
        let (rendered, usage) = render_stream(body).await;
        let mut cursor = 0;
        for marker in [
            "reasoning-start",
            "reasoning-delta",
            "text-start",
            "text-delta",
            "reasoning-end",
            "text-end",
            "finishReason",
        ] {
            let offset = rendered[cursor..]
                .find(marker)
                .unwrap_or_else(|| panic!("missing {marker}: {rendered}"));
            cursor += offset + marker.len();
        }
        assert!(!rendered.contains("api-error"), "{rendered}");
        assert_eq!(usage.completed_requests(), 1);
        assert_eq!(usage.input_tokens(), 3);
        assert_eq!(usage.output_tokens(), 5);
        assert_eq!(usage.reasoning_tokens(), 2);
    }

    #[tokio::test]
    async fn typed_stream_reconstructs_fragmented_tools_exactly_once() {
        let first = json!({
            "choices": [{"delta": {"tool_calls": [{
                "index": 0,
                "id": "call_",
                "function": {"name": "read_", "arguments": r#"{"path":""#}
            }]}}]
        });
        let second = json!({
            "choices": [{"delta": {"tool_calls": [{
                "id": "1",
                "function": {"name": "file", "arguments": "README.md\"}"}
            }]}, "finish_reason": "tool_calls"}]
        });
        let body = format!("data: {first}\n\ndata: {second}\n\ndata: [DONE]\n\n");
        let (rendered, usage) = render_stream(&body).await;

        assert_eq!(rendered.matches("call_1").count(), 1, "{rendered}");
        assert_eq!(rendered.matches("read_file").count(), 1, "{rendered}");
        assert_eq!(rendered.matches("README.md").count(), 1, "{rendered}");
        assert!(rendered.contains("tool-calls"), "{rendered}");
        assert_eq!(usage.completed_requests(), 1);
    }

    #[tokio::test]
    async fn typed_stream_accepts_missing_optional_fields() {
        let (rendered, usage) =
            render_stream("data: {}\n\ndata: {\"choices\":[{}],\"usage\":{}}\n\ndata: [DONE]\n\n")
                .await;

        assert!(rendered.contains("finishReason"), "{rendered}");
        assert!(!rendered.contains("api-error"), "{rendered}");
        assert_eq!(usage.completed_requests(), 1);
        assert_eq!(usage.total_tokens(), 0);
    }

    #[tokio::test]
    async fn typed_stream_rejects_malformed_json() {
        let (rendered, usage) = render_stream("data: {not valid json}\n\n").await;

        assert!(rendered.contains("invalid streaming JSON:"), "{rendered}");
        assert_failed_stream("malformed JSON", &rendered, &usage);
    }

    #[tokio::test]
    async fn typed_stream_rejects_incompatible_chunk_shapes() {
        for (label, chunk) in [
            ("choices", r#"{"choices":"invalid"}"#),
            ("choice", r#"{"choices":[[]]}"#),
            ("delta", r#"{"choices":[{"delta":[]}] }"#),
            ("usage", r#"{"usage":[]}"#),
            (
                "tool calls",
                r#"{"choices":[{"delta":{"tool_calls":"invalid"}}]}"#,
            ),
            (
                "tool call",
                r#"{"choices":[{"delta":{"tool_calls":[[]]}}]}"#,
            ),
            (
                "tool function",
                r#"{"choices":[{"delta":{"tool_calls":[{"function":"invalid"}]}}]}"#,
            ),
            (
                "tool index",
                r#"{"choices":[{"delta":{"tool_calls":[{"index":18446744073709551616}]}}]}"#,
            ),
        ] {
            let body = format!("data: {chunk}\n\ndata: [DONE]\n\n");
            let (rendered, usage) = render_stream(&body).await;
            assert!(
                rendered.contains("invalid streaming chunk:"),
                "{label}: {rendered}"
            );
            assert_failed_stream(label, &rendered, &usage);
        }
    }

    #[tokio::test]
    async fn typed_stream_reports_embedded_errors_with_or_without_messages() {
        for (label, chunk, message) in [
            (
                "message",
                r#"{"error":{"message":"fx boom"},"choices":"invalid"}"#,
                "fx boom",
            ),
            (
                "fallback",
                r#"{"error":{"type":"api_error"}}"#,
                "NaN returned a streaming error",
            ),
        ] {
            let body = format!("data: {chunk}\n\ndata: [DONE]\n\n");
            let (rendered, usage) = render_stream(&body).await;
            assert!(rendered.contains(message), "{label}: {rendered}");
            assert_failed_stream(label, &rendered, &usage);
        }
    }

    #[tokio::test]
    async fn rejects_truncated_text_stream() {
        let events = translate(
            response(
                "data: {\"id\":\"chatcmpl_fx\",\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
            ),
            "qwen3.6".to_owned(),
            upstream(),
            None,
            "fallback query".to_owned(),
            usage_guard(),
        )
        .collect::<Vec<_>>()
        .await;
        let rendered = format!("{events:?}");

        assert!(
            rendered.contains("stream ended before the [DONE] marker"),
            "{rendered}"
        );
        assert!(!rendered.contains("finishReason"), "{rendered}");
    }

    #[tokio::test]
    async fn rejects_truncated_tool_stream_without_emitting_tool_call() {
        let events = translate(
            response(
                "data: {\"id\":\"chatcmpl_fx\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_partial\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"README\"}}]}}]}\n\n",
            ),
            "qwen3.6".to_owned(),
            upstream(),
            None,
            "fallback query".to_owned(),
            usage_guard(),
        )
        .collect::<Vec<_>>()
        .await;
        let rendered = format!("{events:?}");

        assert!(
            rendered.contains("stream ended before the [DONE] marker"),
            "{rendered}"
        );
        assert!(!rendered.contains("toolCallId"), "{rendered}");
        assert!(!rendered.contains("finishReason"), "{rendered}");
        assert!(!rendered.contains("call_partial"), "{rendered}");
    }

    #[tokio::test]
    async fn completes_stream_after_done_marker() {
        let events = translate(
            response(
                "data: {\"id\":\"chatcmpl_fx\",\"choices\":[{\"delta\":{\"content\":\"complete\"}}]}\n\ndata: [DONE]\n\n",
            ),
            "qwen3.6".to_owned(),
            upstream(),
            None,
            "fallback query".to_owned(),
            usage_guard(),
        )
        .collect::<Vec<_>>()
        .await;
        let rendered = format!("{events:?}");

        assert!(rendered.contains("response-metadata"), "{rendered}");
        assert!(rendered.contains("text-start"), "{rendered}");
        assert!(rendered.contains("text-delta"), "{rendered}");
        assert!(rendered.contains("text-end"), "{rendered}");
        assert!(rendered.contains("finishReason"), "{rendered}");
        assert!(!rendered.contains("api-error"), "{rendered}");
    }

    #[tokio::test]
    async fn rejects_empty_tool_name_without_emitting_tool_call() {
        let events = translate(
            response(
                "data: {\"id\":\"chatcmpl_fx\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_empty_name\",\"function\":{\"name\":\"\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n",
            ),
            "qwen3.6".to_owned(),
            upstream(),
            None,
            "fallback query".to_owned(),
            usage_guard(),
        )
        .collect::<Vec<_>>()
        .await;
        let rendered = format!("{events:?}");

        assert!(
            rendered.contains("tool call ended without a valid id or name"),
            "{rendered}"
        );
        assert!(!rendered.contains("toolCallId"), "{rendered}");
        assert!(!rendered.contains("finishReason"), "{rendered}");
        assert!(!rendered.contains("call_empty_name"), "{rendered}");
    }

    #[tokio::test]
    async fn rejects_non_object_tool_arguments_after_done_marker() {
        let events = translate(
            response(
                "data: {\"id\":\"chatcmpl_fx\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_invalid_args\",\"function\":{\"name\":\"read_file\",\"arguments\":\"[]\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n",
            ),
            "qwen3.6".to_owned(),
            upstream(),
            None,
            "fallback query".to_owned(),
            usage_guard(),
        )
        .collect::<Vec<_>>()
        .await;
        let rendered = format!("{events:?}");

        assert!(
            rendered.contains("tool call ended with invalid JSON object arguments"),
            "{rendered}"
        );
        assert!(!rendered.contains("toolCallId"), "{rendered}");
        assert!(!rendered.contains("finishReason"), "{rendered}");
        assert!(!rendered.contains("call_invalid_args"), "{rendered}");
    }
}
