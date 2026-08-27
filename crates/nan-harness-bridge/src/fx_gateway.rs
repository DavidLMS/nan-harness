use crate::auth::is_authorized;
use crate::diagnostics::BridgeDiagnostic;
use crate::error::{ApiError, BridgeError};
use crate::timeouts::{
    STREAM_INACTIVITY_TIMEOUT, map_body_error, map_sse_error, with_inactivity_timeout,
};
use crate::upstream::NanClient;
use crate::{BridgeEndpoint, DiagnosticSender};
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
    CodingModelProfile, ReasoningHint, ReasoningPolicy, ReasoningSelection,
};
use nan_harness_core::{SecretValue, coding_models_from_provider_ids};
use reqwest::Url;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::Arc;

const MAX_REQUEST_BYTES: usize = 32 * 1024 * 1024;
const CHAT_PATH: &str = "/v3/ai/language-model";
const MODELS_PATH: &str = "/coding-agent/v1/models";
const PERMISSION_REVIEW_TOOL: &str = "permission_decision";

#[derive(Debug, Clone)]
struct ProviderSearchTool {
    name: String,
    max_results: usize,
    allowed_domains: Vec<String>,
    blocked_domains: Vec<String>,
}

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
    pub selected_model_id: String,
    pub provider_api_key: Arc<SecretValue>,
    pub session_token: Arc<SecretValue>,
}

#[derive(Clone)]
struct AppState {
    upstream: NanClient,
    models: FxModelCatalog,
    selected_model_id: String,
    session_token: Arc<SecretValue>,
    diagnostics: DiagnosticSender,
}

pub(crate) fn router(
    config: FxGatewayConfig,
    diagnostics: DiagnosticSender,
) -> Result<Router, BridgeError> {
    let state = AppState {
        upstream: NanClient::new(&config.provider_base_url, config.provider_api_key)?,
        models: config.models,
        selected_model_id: config.selected_model_id,
        session_token: config.session_token,
        diagnostics,
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
    let diagnostics = state.diagnostics.clone();
    let result: Result<axum::Json<Value>, ApiError> = async {
        authorize(&headers, &state)?;
        Ok(axum::Json(state.models.api_response()))
    }
    .await;
    emit_diagnostic(&diagnostics, &result, BridgeEndpoint::Models);
    result
}

async fn chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let diagnostics = state.diagnostics.clone();
    let result: Result<Response, ApiError> = async {
        authorize(&headers, &state)?;
        let model_id = headers
            .get("ai-language-model-id")
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| ApiError::InvalidRequest("fx did not provide a model ID".to_owned()))?;
        let request: Value = serde_json::from_slice(&body)
            .map_err(|error| ApiError::InvalidRequest(format!("invalid fx JSON body: {error}")))?;
        let provider_search = provider_search_tool(&request);
        let model = state
            .models
            .resolve(model_id)
            .or_else(|| {
                is_permission_review(&request)
                    .then(|| state.models.resolve(&state.selected_model_id))
                    .flatten()
            })
            .ok_or_else(|| {
                ApiError::InvalidRequest(format!(
                    "model '{model_id}' is not available through this bridge"
                ))
            })?;
        let translated = translate_request(&request, model)?;
        let upstream = ensure_success(state.upstream.send(&translated).await?).await?;
        let events = translate_stream(
            upstream,
            model_id.to_owned(),
            state.upstream.clone(),
            provider_search,
            latest_user_text(&request),
        );
        Ok(Sse::new(events)
            .keep_alive(
                KeepAlive::new()
                    .interval(std::time::Duration::from_secs(15))
                    .text("ping"),
            )
            .into_response())
    }
    .await;
    emit_diagnostic(&diagnostics, &result, BridgeEndpoint::FxGateway);
    result
}

fn emit_diagnostic<T>(
    diagnostics: &DiagnosticSender,
    result: &Result<T, ApiError>,
    endpoint: BridgeEndpoint,
) {
    if let Err(error) = result {
        let _ = diagnostics.send(BridgeDiagnostic::from_api_error(error, endpoint));
    }
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
    let provider_name = tool.get("name").and_then(Value::as_str).unwrap_or("tool");
    let parameters = if tool.get("type").and_then(Value::as_str) == Some("provider") {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"}
            }
        })
    } else {
        tool.get("inputSchema")
            .cloned()
            .unwrap_or_else(|| json!({"type":"object"}))
    };
    json!({
        "type": "function",
        "function": {
            "name": provider_name,
            "description": tool.get("description").and_then(Value::as_str).unwrap_or_default(),
            "parameters": parameters
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

fn is_permission_review(request: &Value) -> bool {
    request
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|tool| tool.get("name").and_then(Value::as_str) == Some(PERMISSION_REVIEW_TOOL))
}

fn provider_search_tool(request: &Value) -> Option<ProviderSearchTool> {
    request
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find_map(|tool| {
            let name = tool.get("name").and_then(Value::as_str)?;
            let id = tool.get("id").and_then(Value::as_str)?;
            let supported = matches!(
                (id, name),
                ("gateway.perplexity_search", "perplexity_search")
                    | ("gateway.parallel_search", "parallel_search")
            );
            if !supported {
                return None;
            }
            let args = tool.get("args").unwrap_or(&Value::Null);
            let max_results = args
                .get("maxResults")
                .and_then(Value::as_u64)
                .unwrap_or(10)
                .clamp(1, 20) as usize;
            let mut allowed_domains = Vec::new();
            let mut blocked_domains = Vec::new();
            if name == "perplexity_search" {
                for domain in args
                    .get("searchDomainFilter")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    if let Some(domain) = domain.strip_prefix('-') {
                        blocked_domains.push(domain.to_owned());
                    } else {
                        allowed_domains.push(domain.to_owned());
                    }
                }
            } else if let Some(source_policy) = args.get("sourcePolicy") {
                allowed_domains = string_array(source_policy.get("includeDomains"));
                blocked_domains = string_array(source_policy.get("excludeDomains"));
            }
            Some(ProviderSearchTool {
                name: name.to_owned(),
                max_results,
                allowed_domains,
                blocked_domains,
            })
        })
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn latest_user_text(request: &Value) -> String {
    request
        .get("prompt")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .rev()
        .find(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        .map(|message| message_text(message.get("content").unwrap_or(&Value::Null)))
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| "web search".to_owned())
}

fn message_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn apply_reasoning(
    body: &mut Value,
    model: &CodingModelProfile,
    effort: &str,
) -> Result<(), ApiError> {
    let hint = match effort {
        "none" => ReasoningHint::Disabled,
        "low" => ReasoningHint::Low,
        "medium" => ReasoningHint::Medium,
        "high" => ReasoningHint::High,
        "xhigh" => ReasoningHint::ExtraHigh,
        other => {
            return Err(ApiError::InvalidRequest(format!(
                "unsupported fx reasoning effort '{other}'"
            )));
        }
    };
    let selection = model.reasoning.resolve_hint(hint).ok_or_else(|| {
        ApiError::InvalidRequest(format!(
            "reasoning effort '{effort}' is incompatible with model policy"
        ))
    })?;
    match selection {
        ReasoningSelection::Toggle(enabled)
            if model.id.starts_with("qwen") || model.id.starts_with("gemma") =>
        {
            body["chat_template_kwargs"] = json!({"enable_thinking": enabled});
        }
        ReasoningSelection::Effort(effort)
            if model.id.starts_with("deepseek") || model.id.starts_with("glm") =>
        {
            body["reasoning_effort"] = serde_json::to_value(effort).expect("effort serializes");
        }
        _ => {}
    }
    Ok(())
}

fn translate_stream(
    response: reqwest::Response,
    model_id: String,
    upstream: NanClient,
    provider_search: Option<ProviderSearchTool>,
    fallback_query: String,
) -> impl Stream<Item = Result<Event, Infallible>> {
    stream! {
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
            for event in stream_end_events(
                &state,
                terminated,
                &upstream,
                provider_search.as_ref(),
                &fallback_query,
            )
            .await
            {
                yield Ok(event);
            }
        }
    }
}

async fn stream_end_events(
    state: &FxStreamState,
    terminated: bool,
    upstream: &NanClient,
    provider_search: Option<&ProviderSearchTool>,
    fallback_query: &str,
) -> Vec<Event> {
    if !terminated {
        return vec![FxStreamState::error_event(
            "stream ended before the [DONE] marker",
        )];
    }
    match state
        .finish_events(upstream, provider_search, fallback_query)
        .await
    {
        Ok(events) => events,
        Err(error) => vec![FxStreamState::error_event(&format!(
            "{error} [{}]",
            error.code()
        ))],
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

    async fn finish_events(
        &self,
        upstream: &NanClient,
        provider_search: Option<&ProviderSearchTool>,
        fallback_query: &str,
    ) -> Result<Vec<Event>, ApiError> {
        let parsed_tools = self
            .tools
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
                Ok((tool, input))
            })
            .collect::<Result<Vec<_>, ApiError>>()?;

        let mut events = Vec::new();
        if self.reasoning_started {
            events.push(Self::event(
                &json!({"type":"reasoning-end","id":"fx_reasoning"}),
            ));
        }
        if self.text_started {
            events.push(Self::event(&json!({"type":"text-end","id":"fx_text"})));
        }
        for (tool, input) in parsed_tools {
            let is_provider_search = provider_search.is_some_and(|search| search.name == tool.name);
            let mut tool_event = json!({
                "type":"tool-call",
                "toolCallId":tool.id,
                "toolName":tool.name,
                "input":input
            });
            if is_provider_search {
                tool_event["providerExecuted"] = json!(true);
            }
            events.push(Self::event(&tool_event));
            if is_provider_search {
                let search = provider_search.expect("provider search is present");
                let query = input
                    .get("query")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or(fallback_query);
                let result = execute_provider_search(upstream, search, query).await;
                events.push(Self::event(&json!({
                    "type":"tool-result",
                    "toolCallId":tool.id,
                    "result":result
                })));
            }
        }
        let has_provider_search = self
            .tools
            .values()
            .any(|tool| provider_search.is_some_and(|search| search.name == tool.name));
        let finish_reason = if has_provider_search
            && self
                .tools
                .values()
                .all(|tool| provider_search.is_some_and(|search| search.name == tool.name))
        {
            json!({"unified":"stop"})
        } else if self.tools.is_empty() {
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
        Ok(events)
    }
}

async fn execute_provider_search(
    upstream: &NanClient,
    provider: &ProviderSearchTool,
    query: &str,
) -> Value {
    let response = upstream
        .search(&json!({
            "query": query,
            "count": provider.max_results,
            "fetch_content": false
        }))
        .await;
    let response = match response {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => {
            return json!({
                "error": {
                    "type": "search_failed",
                    "message": format!("web search returned HTTP {}", response.status())
                }
            });
        }
        Err(_) => {
            return json!({
                "error": {
                    "type": "search_failed",
                    "message": "web search request failed"
                }
            });
        }
    };
    match response.json::<Value>().await {
        Ok(response) => filter_provider_search_response(response, provider),
        Err(_) => json!({
            "error": {
                "type": "search_failed",
                "message": "web search returned invalid JSON"
            }
        }),
    }
}

fn filter_provider_search_response(mut response: Value, provider: &ProviderSearchTool) -> Value {
    let Some(results) = response.get_mut("results").and_then(Value::as_array_mut) else {
        return response;
    };
    if provider.allowed_domains.is_empty() && provider.blocked_domains.is_empty() {
        return response;
    }
    results.retain(|result| {
        let Some(url) = result
            .get("url")
            .and_then(Value::as_str)
            .and_then(|value| Url::parse(value).ok())
        else {
            return false;
        };
        if !matches!(url.scheme(), "http" | "https") {
            return false;
        }
        let allowed = provider.allowed_domains.is_empty()
            || provider
                .allowed_domains
                .iter()
                .any(|domain| matches_domain(&url, domain));
        let blocked = provider
            .blocked_domains
            .iter()
            .any(|domain| matches_domain(&url, domain));
        allowed && !blocked
    });
    response
}

fn matches_domain(url: &Url, domain: &str) -> bool {
    let (hostname, path) = domain
        .split_once('/')
        .map_or((domain, None), |(hostname, path)| (hostname, Some(path)));
    let Some(url_hostname) = url.host_str() else {
        return false;
    };
    let hostname = hostname.to_ascii_lowercase();
    let url_hostname = url_hostname.to_ascii_lowercase();
    let host_matches = url_hostname == hostname || url_hostname.ends_with(&format!(".{hostname}"));
    let path_matches = path.is_none_or(|path| url.path().starts_with(&format!("/{path}")));
    host_matches && path_matches
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, ApiError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.map_err(map_body_error)?;
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
    use super::{FxModelCatalog, NanClient, apply_reasoning, translate_stream};
    use axum::http::Response as HttpResponse;
    use futures_util::StreamExt;
    use nan_harness_core::CodingModelProfile;
    use nan_harness_core::SecretValue;
    use reqwest::Body;
    use serde_json::json;
    use std::sync::Arc;

    fn response(body: &str) -> reqwest::Response {
        reqwest::Response::from(
            HttpResponse::builder()
                .header("content-type", "text/event-stream")
                .body(Body::from(body.to_owned()))
                .expect("test response should build"),
        )
    }

    fn upstream() -> NanClient {
        NanClient::new(
            "http://127.0.0.1",
            Arc::new(SecretValue::new("test-provider-key").expect("test key should be valid")),
        )
        .expect("test upstream should build")
    }

    #[test]
    fn catalog_uses_fx_gateway_shape() {
        let catalog = FxModelCatalog::from_provider_ids(["qwen3.6".to_owned()])
            .expect("catalog should build");
        let model = &catalog.api_response()["data"][0];
        assert_eq!(model["id"], "qwen3.6");
        assert_eq!(model["type"], "language");
        assert_eq!(model["reasoning_options"][0]["values"][0], "none");
    }

    #[test]
    fn reasoning_hints_follow_shared_model_policy_resolution() {
        let qwen = nan_harness_core::coding_model_profile("qwen3.6").expect("known model");
        let mut qwen_body = json!({});
        apply_reasoning(&mut qwen_body, &qwen, "medium")
            .expect("positive effort should enable toggle reasoning");
        assert_eq!(qwen_body["chat_template_kwargs"]["enable_thinking"], true);

        let qwen38 = nan_harness_core::coding_model_profile("qwen3.8-flash").expect("known model");
        let mut qwen38_body = json!({});
        apply_reasoning(&mut qwen38_body, &qwen38, "high")
            .expect("always-on reasoning should be accepted");
        assert_eq!(qwen38_body["chat_template_kwargs"]["enable_thinking"], true);
        assert!(apply_reasoning(&mut qwen38_body, &qwen38, "none").is_err());

        let glm53 = nan_harness_core::coding_model_profile("glm5.3-flash").expect("known model");
        let mut glm53_body = json!({});
        apply_reasoning(&mut glm53_body, &glm53, "low")
            .expect("effort reasoning should be accepted");
        assert_eq!(glm53_body["reasoning_effort"], "low");
        assert!(apply_reasoning(&mut glm53_body, &glm53, "none").is_err());

        let mimo = nan_harness_core::coding_model_profile("mimo-v2.5").expect("known model");
        let mut mimo_body = json!({});
        apply_reasoning(&mut mimo_body, &mimo, "medium")
            .expect("positive effort should preserve always-on reasoning");
        assert_eq!(mimo_body, json!({}));

        let generic = CodingModelProfile::generic("future-coding-model");
        let mut generic_body = json!({});
        apply_reasoning(&mut generic_body, &generic, "medium")
            .expect("unprofiled models should use native reasoning defaults");
        assert_eq!(generic_body, json!({}));
    }

    #[tokio::test]
    async fn rejects_truncated_text_stream() {
        let events = translate_stream(
            response(
                "data: {\"id\":\"chatcmpl_fx\",\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
            ),
            "qwen3.6".to_owned(),
            upstream(),
            None,
            "fallback query".to_owned(),
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
        let events = translate_stream(
            response(
                "data: {\"id\":\"chatcmpl_fx\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_partial\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"README\"}}]}}]}\n\n",
            ),
            "qwen3.6".to_owned(),
            upstream(),
            None,
            "fallback query".to_owned(),
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
        let events = translate_stream(
            response(
                "data: {\"id\":\"chatcmpl_fx\",\"choices\":[{\"delta\":{\"content\":\"complete\"}}]}\n\ndata: [DONE]\n\n",
            ),
            "qwen3.6".to_owned(),
            upstream(),
            None,
            "fallback query".to_owned(),
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
        let events = translate_stream(
            response(
                "data: {\"id\":\"chatcmpl_fx\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_empty_name\",\"function\":{\"name\":\"\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n",
            ),
            "qwen3.6".to_owned(),
            upstream(),
            None,
            "fallback query".to_owned(),
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
        let events = translate_stream(
            response(
                "data: {\"id\":\"chatcmpl_fx\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_invalid_args\",\"function\":{\"name\":\"read_file\",\"arguments\":\"[]\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n",
            ),
            "qwen3.6".to_owned(),
            upstream(),
            None,
            "fallback query".to_owned(),
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
