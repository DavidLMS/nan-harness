use crate::auth::is_authorized;
use crate::error::{ApiError, BridgeError};
use crate::upstream::NanClient;
use async_stream::stream;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use eventsource_stream::Eventsource;
use futures_util::{Stream, StreamExt};
use nan_harness_core::model::{
    CodingModelProfile, ReasoningEffort, ReasoningPolicy, ReasoningSelection,
};
use nan_harness_core::{SecretValue, coding_models_from_provider_ids};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::Arc;

const MAX_REQUEST_BYTES: usize = 32 * 1024 * 1024;
const CHAT_PATH: &str = "/v3/ai/language-model";
const MODELS_PATH: &str = "/coding-agent/v1/models";

#[derive(Debug, Clone)]
pub struct FxModelCatalog {
    models: Vec<CodingModelProfile>,
    by_id: BTreeMap<String, usize>,
}

impl FxModelCatalog {
    /// Builds a catalog from the dynamically discovered NaN profiles.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError::NoCompatibleModels`] when the discovery result is empty.
    pub fn from_models(models: Vec<CodingModelProfile>) -> Result<Self, BridgeError> {
        if models.is_empty() {
            return Err(BridgeError::NoCompatibleModels);
        }
        let by_id = models
            .iter()
            .enumerate()
            .map(|(index, model)| (model.id.clone(), index))
            .collect();
        Ok(Self { models, by_id })
    }

    /// Builds a catalog from provider model IDs after applying NaN's coding-model policy.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError::NoCompatibleModels`] when no coding model is available.
    pub fn from_provider_ids(
        provider_ids: impl IntoIterator<Item = String>,
    ) -> Result<Self, BridgeError> {
        Self::from_models(coding_models_from_provider_ids(provider_ids))
    }

    #[must_use]
    pub fn resolve(&self, id: &str) -> Option<&CodingModelProfile> {
        self.by_id.get(id).map(|index| &self.models[*index])
    }

    pub fn api_response(&self) -> Value {
        json!({
            "object": "list",
            "data": self.models.iter().map(api_model).collect::<Vec<_>>()
        })
    }
}

#[derive(Debug)]
pub struct FxGatewayConfig {
    pub provider_base_url: String,
    pub models: FxModelCatalog,
    pub provider_api_key: Arc<SecretValue>,
    pub session_token: Arc<SecretValue>,
}

#[derive(Clone)]
struct AppState {
    upstream: NanClient,
    models: FxModelCatalog,
    session_token: Arc<SecretValue>,
}

pub(crate) fn router(config: FxGatewayConfig) -> Result<Router, BridgeError> {
    let state = AppState {
        upstream: NanClient::new(&config.provider_base_url, config.provider_api_key)?,
        models: config.models,
        session_token: config.session_token,
    };
    Ok(Router::new()
        .route(MODELS_PATH, get(models))
        .route(CHAT_PATH, post(chat))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(state))
}

async fn models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<axum::Json<Value>, ApiError> {
    authorize(&headers, &state)?;
    Ok(axum::Json(state.models.api_response()))
}

async fn chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    authorize(&headers, &state)?;
    let model_id = headers
        .get("ai-language-model-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::InvalidRequest("fx did not provide a model ID".to_owned()))?;
    let model = state.models.resolve(model_id).ok_or_else(|| {
        ApiError::InvalidRequest(format!(
            "model '{model_id}' is not available through this bridge"
        ))
    })?;
    let request: Value = serde_json::from_slice(&body)
        .map_err(|error| ApiError::InvalidRequest(format!("invalid fx JSON body: {error}")))?;
    let translated = translate_request(&request, model)?;
    let upstream = ensure_success(state.upstream.send(&translated).await?).await?;
    let events = translate_stream(upstream, model_id.to_owned());
    Ok(Sse::new(events)
        .keep_alive(
            KeepAlive::new()
                .interval(std::time::Duration::from_secs(15))
                .text("ping"),
        )
        .into_response())
}

fn authorize(headers: &HeaderMap, state: &AppState) -> Result<(), ApiError> {
    if is_authorized(headers, &state.session_token) {
        Ok(())
    } else {
        Err(ApiError::Unauthorized)
    }
}

fn api_model(model: &CodingModelProfile) -> Value {
    let mut tags = vec!["tool-use"];
    if model.image_input {
        tags.push("vision");
    }
    let reasoning_options = match model.reasoning {
        ReasoningPolicy::Toggle { .. } => {
            tags.push("reasoning");
            json!([{"type":"effort","values":["none","high"]}])
        }
        ReasoningPolicy::Effort { .. } => {
            tags.push("reasoning");
            json!([{"type":"effort","values":["low","medium","high"]}])
        }
        ReasoningPolicy::AlwaysOn => {
            tags.push("reasoning");
            json!([{"type":"effort","values":["high"]}])
        }
        ReasoningPolicy::Unsupported | ReasoningPolicy::Unknown => Value::Null,
    };
    json!({
        "id": model.id,
        "type": "language",
        "released": 0,
        "tags": tags,
        "reasoning_options": reasoning_options,
        "context_window": model.context_window,
        "max_tokens": model.max_output_tokens
    })
}

fn translate_request(request: &Value, model: &CodingModelProfile) -> Result<Value, ApiError> {
    let prompt = request
        .get("prompt")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::InvalidRequest("fx request is missing prompt".to_owned()))?;
    let mut messages = Vec::new();
    for message in prompt {
        translate_message(message, &mut messages)?;
    }

    let mut body = json!({
        "model": model.id,
        "messages": messages,
        "stream": true,
        "stream_options": {"include_usage": true}
    });
    if let Some(max_tokens) = request.get("maxOutputTokens").and_then(Value::as_u64) {
        body["max_tokens"] = json!(max_tokens);
    }
    if let Some(tools) = request.get("tools").and_then(Value::as_array) {
        body["tools"] = Value::Array(tools.iter().map(translate_tool).collect());
    }
    if let Some(choice) = request.get("toolChoice") {
        body["tool_choice"] = translate_tool_choice(choice);
    }
    if let Some(reasoning) = request.get("reasoning").and_then(Value::as_str) {
        apply_reasoning(&mut body, model, reasoning)?;
    }
    Ok(body)
}

fn translate_message(message: &Value, output: &mut Vec<Value>) -> Result<(), ApiError> {
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::InvalidRequest("fx prompt message has no role".to_owned()))?;
    let content = message.get("content").cloned().unwrap_or_else(|| json!(""));
    match role {
        "system" | "user" => output.push(json!({
            "role": role,
            "content": content_for_chat(&content)
        })),
        "assistant" => {
            let parts = content.as_array().cloned().unwrap_or_default();
            let text = parts
                .iter()
                .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<String>();
            let tool_calls = parts
                .iter()
                .filter(|part| part.get("type").and_then(Value::as_str) == Some("tool-call"))
                .map(|part| {
                    json!({
                        "id": part.get("toolCallId").and_then(Value::as_str).unwrap_or("fx_tool_call"),
                        "type": "function",
                        "function": {
                            "name": part.get("toolName").and_then(Value::as_str).unwrap_or("tool"),
                            "arguments": serde_json::to_string(part.get("input").unwrap_or(&Value::Null)).unwrap_or_else(|_| "{}".to_owned())
                        }
                    })
                })
                .collect::<Vec<_>>();
            let mut translated = json!({"role":"assistant","content":text});
            if !tool_calls.is_empty() {
                translated["tool_calls"] = Value::Array(tool_calls);
            }
            output.push(translated);
        }
        "tool" => {
            for part in content.as_array().into_iter().flatten() {
                if part.get("type").and_then(Value::as_str) != Some("tool-result") {
                    continue;
                }
                output.push(json!({
                    "role": "tool",
                    "tool_call_id": part.get("toolCallId").and_then(Value::as_str).unwrap_or("fx_tool_call"),
                    "content": tool_result_text(part.get("output"))
                }));
            }
        }
        other => {
            return Err(ApiError::InvalidRequest(format!(
                "unsupported fx prompt role '{other}'"
            )));
        }
    }
    Ok(())
}

fn content_for_chat(content: &Value) -> Value {
    match content {
        Value::String(value) => Value::String(value.clone()),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| match part.get("type").and_then(Value::as_str) {
                Some("text") => Some(json!({
                    "type": "text",
                    "text": part.get("text").and_then(Value::as_str).unwrap_or_default()
                })),
                Some("file") => {
                    let data = part.get("data").and_then(Value::as_str)?;
                    let media_type = part
                        .get("mediaType")
                        .and_then(Value::as_str)
                        .unwrap_or("application/octet-stream");
                    Some(json!({
                        "type": "image_url",
                        "image_url": {"url": format!("data:{media_type};base64,{data}")}
                    }))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .into(),
        _ => Value::String(content.to_string()),
    }
}

fn tool_result_text(output: Option<&Value>) -> String {
    match output {
        Some(Value::Object(value)) if value.get("type").and_then(Value::as_str) == Some("text") => {
            value
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        }
        Some(value) => value.to_string(),
        None => String::new(),
    }
}

fn translate_tool(tool: &Value) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.get("name").and_then(Value::as_str).unwrap_or("tool"),
            "description": tool.get("description").and_then(Value::as_str).unwrap_or_default(),
            "parameters": tool.get("inputSchema").cloned().unwrap_or_else(|| json!({"type":"object"}))
        }
    })
}

fn translate_tool_choice(choice: &Value) -> Value {
    match choice.get("type").and_then(Value::as_str).unwrap_or("auto") {
        "required" => json!("required"),
        "none" => json!("none"),
        _ => json!("auto"),
    }
}

fn apply_reasoning(
    body: &mut Value,
    model: &CodingModelProfile,
    effort: &str,
) -> Result<(), ApiError> {
    let selection = match effort {
        "none" => ReasoningSelection::Toggle(false),
        "low" => ReasoningSelection::Effort(ReasoningEffort::Low),
        "medium" => ReasoningSelection::Effort(ReasoningEffort::Medium),
        "high" => match model.reasoning {
            ReasoningPolicy::Toggle { .. } | ReasoningPolicy::AlwaysOn => {
                ReasoningSelection::Toggle(true)
            }
            _ => ReasoningSelection::Effort(ReasoningEffort::High),
        },
        other => {
            return Err(ApiError::InvalidRequest(format!(
                "unsupported fx reasoning effort '{other}'"
            )));
        }
    };
    if !model.reasoning.accepts(selection) {
        return Err(ApiError::InvalidRequest(format!(
            "reasoning effort '{effort}' is incompatible with model policy"
        )));
    }
    match selection {
        ReasoningSelection::Toggle(enabled)
            if model.id.starts_with("qwen") || model.id.starts_with("gemma") =>
        {
            body["chat_template_kwargs"] = json!({"enable_thinking": enabled});
        }
        ReasoningSelection::Effort(effort) if model.id.starts_with("deepseek") => {
            body["reasoning_effort"] = serde_json::to_value(effort).expect("effort serializes");
        }
        _ => {}
    }
    Ok(())
}

fn translate_stream(
    response: reqwest::Response,
    model_id: String,
) -> impl Stream<Item = Result<Event, Infallible>> {
    stream! {
        let mut source = response.bytes_stream().eventsource();
        let mut state = FxStreamState::new(model_id);
        let mut failed = false;
        yield Ok(FxStreamState::event(&json!({
            "type": "response-metadata",
            "modelId": state.model_id.clone()
        })));
        while let Some(item) = source.next().await {
            let event = match item {
                Ok(event) => event,
                Err(error) => {
                    yield Ok(FxStreamState::error_event(&format!(
                        "invalid NaN SSE stream: {error}"
                    )));
                    failed = true;
                    break;
                }
            };
            if event.data.trim() == "[DONE]" || event.data.trim().is_empty() {
                continue;
            }
            let value: Value = match serde_json::from_str(&event.data) {
                Ok(value) => value,
                Err(error) => {
                    yield Ok(FxStreamState::error_event(&format!(
                        "invalid NaN streaming JSON: {error}"
                    )));
                    failed = true;
                    break;
                }
            };
            if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
                yield Ok(FxStreamState::error_event(message));
                failed = true;
                break;
            }
            let choices = value.get("choices").and_then(Value::as_array).cloned().unwrap_or_default();
            for choice in choices {
                let delta = choice.get("delta").cloned().unwrap_or_default();
                if let Some(reasoning) = delta.get("reasoning_content").and_then(Value::as_str).filter(|text| !text.is_empty()) {
                    if !state.reasoning_started {
                        yield Ok(FxStreamState::event(&json!({"type":"reasoning-start","id":"fx_reasoning"})));
                        state.reasoning_started = true;
                    }
                    yield Ok(FxStreamState::event(&json!({"type":"reasoning-delta","id":"fx_reasoning","delta":reasoning})));
                    state.reasoning.push_str(reasoning);
                }
                if let Some(text) = delta.get("content").and_then(Value::as_str).filter(|text| !text.is_empty()) {
                    if !state.text_started {
                        yield Ok(FxStreamState::event(&json!({"type":"text-start","id":"fx_text"})));
                        state.text_started = true;
                    }
                    yield Ok(FxStreamState::event(&json!({"type":"text-delta","id":"fx_text","delta":text})));
                    state.text.push_str(text);
                }
                if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                    for call in tool_calls {
                        state.update_tool(call);
                    }
                }
                if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                    state.finish_reason = Some(reason.to_owned());
                }
            }
            if let Some(usage) = value.get("usage") {
                state.input_tokens = usage.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0);
                state.output_tokens = usage.get("completion_tokens").and_then(Value::as_u64).unwrap_or(0);
            }
        }
        if !failed {
            for event in state.finish_events() {
                yield Ok(event);
            }
        }
    }
}

struct FxToolState {
    id: String,
    name: String,
    arguments: String,
}

struct FxStreamState {
    model_id: String,
    text: String,
    reasoning: String,
    text_started: bool,
    reasoning_started: bool,
    tools: BTreeMap<usize, FxToolState>,
    finish_reason: Option<String>,
    input_tokens: u64,
    output_tokens: u64,
}

impl FxStreamState {
    fn new(model_id: String) -> Self {
        Self {
            model_id,
            text: String::new(),
            reasoning: String::new(),
            text_started: false,
            reasoning_started: false,
            tools: BTreeMap::new(),
            finish_reason: None,
            input_tokens: 0,
            output_tokens: 0,
        }
    }

    fn event(value: &Value) -> Event {
        Event::default().data(value.to_string())
    }

    fn error_event(message: &str) -> Event {
        Self::event(&json!({"type":"error","error":{"type":"api-error","message":message}}))
    }

    fn update_tool(&mut self, call: &Value) {
        let index =
            usize::try_from(call.get("index").and_then(Value::as_u64).unwrap_or(0)).unwrap_or(0);
        let tool = self.tools.entry(index).or_insert_with(|| FxToolState {
            id: String::new(),
            name: String::new(),
            arguments: String::new(),
        });
        if let Some(id) = call.get("id").and_then(Value::as_str) {
            tool.id.push_str(id);
        }
        if let Some(function) = call.get("function") {
            if let Some(name) = function.get("name").and_then(Value::as_str) {
                tool.name.push_str(name);
            }
            if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                tool.arguments.push_str(arguments);
            }
        }
    }

    fn finish_events(&self) -> Vec<Event> {
        let mut events = Vec::new();
        if self.reasoning_started {
            events.push(Self::event(
                &json!({"type":"reasoning-end","id":"fx_reasoning"}),
            ));
        }
        if self.text_started {
            events.push(Self::event(&json!({"type":"text-end","id":"fx_text"})));
        }
        for tool in self.tools.values() {
            if tool.id.is_empty() || tool.name.is_empty() {
                continue;
            }
            let input =
                serde_json::from_str::<Value>(&tool.arguments).unwrap_or_else(|_| json!({}));
            events.push(Self::event(&json!({
                "type":"tool-call",
                "toolCallId":tool.id,
                "toolName":tool.name,
                "input":input
            })));
        }
        let finish_reason = if self.tools.is_empty() {
            match self.finish_reason.as_deref() {
                Some("length") => json!({"unified":"length"}),
                _ => json!({"unified":"stop"}),
            }
        } else {
            json!({"unified":"tool-calls"})
        };
        events.push(Self::event(&json!({
            "type":"finish",
            "finishReason":finish_reason,
            "usage": {
                "inputTokens": {"total": self.input_tokens},
                "outputTokens": {"total": self.output_tokens}
            },
            "providerMetadata": {"gateway": {"routing": {"canonicalSlug": self.model_id}}}
        })));
        events
    }
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, ApiError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    let parsed: Value = serde_json::from_str(&body).unwrap_or_default();
    let message = parsed
        .pointer("/error/message")
        .or_else(|| parsed.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("NaN request failed")
        .replace(['\r', '\n'], " ")
        .chars()
        .take(300)
        .collect();
    Err(ApiError::UpstreamStatus { status, message })
}

#[cfg(test)]
mod tests {
    use super::FxModelCatalog;

    #[test]
    fn catalog_uses_fx_gateway_shape() {
        let catalog = FxModelCatalog::from_provider_ids(["qwen3.6".to_owned()])
            .expect("catalog should build");
        let model = &catalog.api_response()["data"][0];
        assert_eq!(model["id"], "qwen3.6");
        assert_eq!(model["type"], "language");
        assert_eq!(model["reasoning_options"][0]["values"][0], "none");
    }
}
