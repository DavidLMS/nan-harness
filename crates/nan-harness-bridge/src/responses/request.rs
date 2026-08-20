use crate::error::ApiError;
use nan_harness_core::model::{
    CodingModelProfile, ReasoningEffort, ReasoningPolicy, ReasoningSelection,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Deserialize)]
pub(crate) struct ResponsesRequest {
    pub(crate) model: String,
    #[serde(default)]
    instructions: String,
    #[serde(default)]
    input: Vec<Value>,
    #[serde(default)]
    tools: Vec<Value>,
    #[serde(default = "default_tool_choice")]
    tool_choice: Value,
    #[serde(default)]
    parallel_tool_calls: bool,
    #[serde(default)]
    reasoning: Option<ResponsesReasoning>,
    #[serde(default)]
    pub(crate) stream: bool,
}

#[derive(Debug, Deserialize)]
struct ResponsesReasoning {
    effort: String,
}

#[derive(Debug)]
pub(crate) struct TranslatedRequest {
    pub(crate) body: Value,
    pub(crate) tools: ToolCatalog,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToolTarget {
    Function {
        name: String,
        namespace: Option<String>,
    },
    Custom {
        name: String,
    },
    ToolSearch,
}

#[derive(Debug, Default)]
pub(crate) struct ToolCatalog {
    aliases: BTreeMap<String, ToolTarget>,
}

impl ToolCatalog {
    pub(crate) fn target(&self, alias: &str) -> Option<&ToolTarget> {
        self.aliases.get(alias)
    }

    fn alias_for(&self, target: &ToolTarget) -> Option<&str> {
        self.aliases
            .iter()
            .find_map(|(alias, candidate)| (candidate == target).then_some(alias.as_str()))
    }
}

pub(crate) fn translate(
    request: ResponsesRequest,
    model: &CodingModelProfile,
) -> Result<TranslatedRequest, ApiError> {
    if request.model.trim().is_empty() {
        return Err(ApiError::InvalidRequest("model cannot be empty".to_owned()));
    }
    if !request.stream {
        return Err(ApiError::InvalidRequest(
            "Codex must request a streaming response".to_owned(),
        ));
    }

    let (tools, catalog) = translate_tools(&request.tools)?;
    let mut messages = Vec::new();
    if !request.instructions.trim().is_empty() {
        messages.push(json!({"role": "system", "content": request.instructions}));
    }
    let reasoning = validate_reasoning(request.reasoning.as_ref(), model.reasoning)?;
    let mut pending_reasoning = None;
    for item in request.input {
        if item.get("type").and_then(Value::as_str) == Some("reasoning") {
            let text = reasoning_text(&item);
            if !text.is_empty() {
                pending_reasoning = Some(text);
            }
            continue;
        }
        let before = messages.len();
        translate_input_item(&item, &catalog, &mut messages)?;
        if messages.len() > before
            && matches!(
                item.get("type").and_then(Value::as_str),
                Some("function_call" | "custom_tool_call" | "agent_message")
            )
            && let Some(text) = pending_reasoning.take()
        {
            messages.last_mut().expect("new message")["reasoning_content"] = Value::String(text);
        }
    }
    if messages.is_empty() {
        return Err(ApiError::InvalidRequest(
            "input must contain at least one message".to_owned(),
        ));
    }

    let mut body = Map::from_iter([
        ("model".to_owned(), Value::String(model.id.clone())),
        ("messages".to_owned(), Value::Array(messages)),
        ("stream".to_owned(), Value::Bool(true)),
        ("stream_options".to_owned(), json!({"include_usage": true})),
        (
            "max_tokens".to_owned(),
            Value::Number(model.max_output_tokens.into()),
        ),
    ]);
    apply_reasoning_parameter(&mut body, &model.id, reasoning);
    if !tools.is_empty() {
        body.insert("tools".to_owned(), Value::Array(tools));
        body.insert(
            "tool_choice".to_owned(),
            translate_tool_choice(&request.tool_choice, &catalog),
        );
        body.insert(
            "parallel_tool_calls".to_owned(),
            Value::Bool(request.parallel_tool_calls),
        );
    }

    Ok(TranslatedRequest {
        body: Value::Object(body),
        tools: catalog,
    })
}

fn validate_reasoning(
    request: Option<&ResponsesReasoning>,
    policy: ReasoningPolicy,
) -> Result<ReasoningSelection, ApiError> {
    let Some(request) = request else {
        return Ok(ReasoningSelection::Auto);
    };
    let selection = match request.effort.as_str() {
        "none" => ReasoningSelection::Toggle(false),
        "low" => ReasoningSelection::Effort(ReasoningEffort::Low),
        "medium" => ReasoningSelection::Effort(ReasoningEffort::Medium),
        "high" | "xhigh" => match policy {
            ReasoningPolicy::Toggle { .. } | ReasoningPolicy::AlwaysOn => {
                ReasoningSelection::Toggle(true)
            }
            _ => ReasoningSelection::Effort(ReasoningEffort::High),
        },
        other => {
            return Err(ApiError::InvalidRequest(format!(
                "unsupported reasoning effort '{other}'"
            )));
        }
    };
    if policy.accepts(selection) {
        Ok(selection)
    } else {
        Err(ApiError::InvalidRequest(format!(
            "reasoning effort '{}' is incompatible with model policy",
            request.effort
        )))
    }
}

fn apply_reasoning_parameter(
    body: &mut Map<String, Value>,
    model_id: &str,
    selection: ReasoningSelection,
) {
    match selection {
        ReasoningSelection::Toggle(enabled)
            if model_id.starts_with("qwen") || model_id.starts_with("gemma") =>
        {
            body.insert(
                "chat_template_kwargs".to_owned(),
                json!({"enable_thinking": enabled}),
            );
        }
        ReasoningSelection::Effort(effort) if model_id.starts_with("deepseek") => {
            body.insert(
                "reasoning_effort".to_owned(),
                serde_json::to_value(effort).expect("effort serializes"),
            );
        }
        ReasoningSelection::Auto
        | ReasoningSelection::Toggle(_)
        | ReasoningSelection::Effort(_) => {}
    }
}

fn reasoning_text(item: &Value) -> String {
    item.get("summary")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

fn translate_input_item(
    item: &Value,
    catalog: &ToolCatalog,
    messages: &mut Vec<Value>,
) -> Result<(), ApiError> {
    let kind = item.get("type").and_then(Value::as_str).unwrap_or_default();
    match kind {
        "message" => translate_message(item, messages),
        "agent_message" => translate_agent_message(item, messages),
        "function_call" => translate_function_call(item, catalog, messages),
        "custom_tool_call" => translate_custom_tool_call(item, catalog, messages),
        "function_call_output" | "custom_tool_call_output" => translate_tool_output(item, messages),
        "reasoning"
        | "web_search_call"
        | "tool_search_call"
        | "tool_search_output"
        | "local_shell_call"
        | "computer_call"
        | "computer_call_output"
        | "compaction" => Ok(()),
        "" if item.get("role").and_then(Value::as_str).is_some() => {
            translate_message(item, messages)
        }
        "" => Err(ApiError::InvalidRequest(
            "input item is missing its type".to_owned(),
        )),
        other => Err(ApiError::InvalidRequest(format!(
            "unsupported Responses input item '{other}'"
        ))),
    }
}

fn translate_message(item: &Value, messages: &mut Vec<Value>) -> Result<(), ApiError> {
    let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
    let role = match role {
        "assistant" => "assistant",
        "system" | "developer" => "system",
        _ => "user",
    };
    let content = translate_content(item.get("content"))?;
    messages.push(json!({"role": role, "content": content}));
    Ok(())
}

fn translate_agent_message(item: &Value, messages: &mut Vec<Value>) -> Result<(), ApiError> {
    let content = translate_content(item.get("content"))?;
    messages.push(json!({"role": "assistant", "content": content}));
    Ok(())
}

fn translate_content(content: Option<&Value>) -> Result<Value, ApiError> {
    let Some(content) = content.and_then(Value::as_array) else {
        return Err(ApiError::InvalidRequest(
            "message content must be an array".to_owned(),
        ));
    };
    let mut parts = Vec::new();
    for part in content {
        match part.get("type").and_then(Value::as_str) {
            Some("input_text" | "output_text") => {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    parts.push(json!({"type": "text", "text": text}));
                }
            }
            Some("input_image") => {
                if let Some(url) = part.get("image_url").and_then(Value::as_str) {
                    parts.push(json!({"type": "image_url", "image_url": {"url": url}}));
                }
            }
            Some("input_audio" | "encrypted_content") | None => {}
            Some(other) => {
                return Err(ApiError::InvalidRequest(format!(
                    "unsupported Responses content item '{other}'"
                )));
            }
        }
    }
    if parts.len() == 1 && parts[0].get("type").and_then(Value::as_str) == Some("text") {
        Ok(parts.remove(0).get("text").cloned().unwrap_or_default())
    } else {
        Ok(Value::Array(parts))
    }
}

fn translate_function_call(
    item: &Value,
    catalog: &ToolCatalog,
    messages: &mut Vec<Value>,
) -> Result<(), ApiError> {
    let name = required_string(item, "name")?;
    let namespace = item
        .get("namespace")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let target = ToolTarget::Function {
        name: name.to_owned(),
        namespace,
    };
    let alias = catalog.alias_for(&target).unwrap_or(name);
    push_assistant_tool_call(item, alias, required_string(item, "arguments")?, messages)
}

fn translate_custom_tool_call(
    item: &Value,
    catalog: &ToolCatalog,
    messages: &mut Vec<Value>,
) -> Result<(), ApiError> {
    let name = required_string(item, "name")?;
    let target = ToolTarget::Custom {
        name: name.to_owned(),
    };
    let alias = catalog.alias_for(&target).unwrap_or(name);
    let input = required_string(item, "input")?;
    let arguments = json!({"input": input}).to_string();
    push_assistant_tool_call(item, alias, &arguments, messages)
}

fn push_assistant_tool_call(
    item: &Value,
    name: &str,
    arguments: &str,
    messages: &mut Vec<Value>,
) -> Result<(), ApiError> {
    let call_id = required_string(item, "call_id")?;
    messages.push(json!({
        "role": "assistant",
        "content": null,
        "tool_calls": [{
            "id": call_id,
            "type": "function",
            "function": {"name": name, "arguments": arguments}
        }]
    }));
    Ok(())
}

fn translate_tool_output(item: &Value, messages: &mut Vec<Value>) -> Result<(), ApiError> {
    let call_id = required_string(item, "call_id")?;
    let output = item.get("output").map_or_else(String::new, output_text);
    messages.push(json!({
        "role": "tool",
        "tool_call_id": call_id,
        "content": output
    }));
    Ok(())
}

fn output_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        value => value.to_string(),
    }
}

fn translate_tools(tools: &[Value]) -> Result<(Vec<Value>, ToolCatalog), ApiError> {
    let mut translated = Vec::new();
    let mut catalog = ToolCatalog::default();
    let mut used_aliases = BTreeSet::new();
    for tool in tools {
        match tool.get("type").and_then(Value::as_str) {
            Some("function") => {
                add_function_tool(tool, None, &mut translated, &mut catalog, &mut used_aliases)?;
            }
            Some("namespace") => {
                let namespace = required_string(tool, "name")?;
                for child in tool
                    .get("tools")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    add_function_tool(
                        child,
                        Some(namespace),
                        &mut translated,
                        &mut catalog,
                        &mut used_aliases,
                    )?;
                }
            }
            Some("custom") => {
                let name = required_string(tool, "name")?;
                let target = ToolTarget::Custom {
                    name: name.to_owned(),
                };
                let alias = unique_alias(name, &mut used_aliases);
                let description = tool
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("Run a freeform tool");
                translated.push(chat_tool(
                    &alias,
                    description,
                    &json!({
                        "type": "object",
                        "properties": {"input": {"type": "string"}},
                        "required": ["input"],
                        "additionalProperties": false
                    }),
                ));
                catalog.aliases.insert(alias, target);
            }
            Some("tool_search") => {
                let alias = unique_alias("tool_search", &mut used_aliases);
                translated.push(chat_tool(
                    &alias,
                    tool.get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("Search for deferred tools"),
                    &schema(tool),
                ));
                catalog.aliases.insert(alias, ToolTarget::ToolSearch);
            }
            Some("web_search") => {}
            Some(other) => {
                return Err(ApiError::InvalidRequest(format!(
                    "unsupported Responses tool type '{other}'"
                )));
            }
            None => {
                return Err(ApiError::InvalidRequest(
                    "tool definition is missing its type".to_owned(),
                ));
            }
        }
    }
    Ok((translated, catalog))
}

fn add_function_tool(
    tool: &Value,
    namespace: Option<&str>,
    translated: &mut Vec<Value>,
    catalog: &mut ToolCatalog,
    used_aliases: &mut BTreeSet<String>,
) -> Result<(), ApiError> {
    let name = required_string(tool, "name")?;
    let preferred_alias =
        namespace.map_or_else(|| name.to_owned(), |value| format!("{value}__{name}"));
    let alias = unique_alias(&preferred_alias, used_aliases);
    let description = tool
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("Run a Codex tool");
    let description = namespace.map_or_else(
        || description.to_owned(),
        |namespace| format!("Codex namespace `{namespace}` tool `{name}`. {description}"),
    );
    translated.push(chat_tool(&alias, &description, &schema(tool)));
    catalog.aliases.insert(
        alias,
        ToolTarget::Function {
            name: name.to_owned(),
            namespace: namespace.map(str::to_owned),
        },
    );
    Ok(())
}

fn schema(tool: &Value) -> Value {
    tool.get("parameters")
        .cloned()
        .unwrap_or_else(|| json!({"type": "object", "properties": {}}))
}

fn chat_tool(name: &str, description: &str, parameters: &Value) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters
        }
    })
}

fn unique_alias(preferred: &str, used: &mut BTreeSet<String>) -> String {
    let mut base = preferred
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .take(56)
        .collect::<String>();
    if base.is_empty() {
        base.push_str("nan_tool");
    }
    if used.insert(base.clone()) {
        return base;
    }
    for index in 2..10_000 {
        let suffix = format!("_{index}");
        let maximum = 64_usize.saturating_sub(suffix.len());
        let candidate = format!("{}{suffix}", base.chars().take(maximum).collect::<String>());
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!("tool alias space should not be exhausted")
}

fn translate_tool_choice(choice: &Value, catalog: &ToolCatalog) -> Value {
    if let Some(choice) = choice.as_str() {
        return match choice {
            "none" | "required" | "auto" => Value::String(choice.to_owned()),
            _ => Value::String("auto".to_owned()),
        };
    }
    let name = choice.get("name").and_then(Value::as_str);
    let namespace = choice.get("namespace").and_then(Value::as_str);
    let target = name.map(|name| ToolTarget::Function {
        name: name.to_owned(),
        namespace: namespace.map(str::to_owned),
    });
    target
        .as_ref()
        .and_then(|target| catalog.alias_for(target))
        .map_or_else(
            || Value::String("auto".to_owned()),
            |alias| json!({"type": "function", "function": {"name": alias}}),
        )
}

fn required_string<'a>(value: &'a Value, field: &str) -> Result<&'a str, ApiError> {
    value.get(field).and_then(Value::as_str).ok_or_else(|| {
        ApiError::InvalidRequest(format!("Responses item requires string field '{field}'"))
    })
}

fn default_tool_choice() -> Value {
    Value::String("auto".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{ResponsesRequest, ToolTarget, translate};
    use serde_json::json;

    #[test]
    fn flattens_namespaces_and_preserves_reverse_routing() {
        let request: ResponsesRequest = serde_json::from_value(json!({
            "model": "qwen3.6",
            "stream": true,
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": "search"}]
            }],
            "tools": [{
                "type": "namespace",
                "name": "web",
                "description": "Web tools",
                "tools": [{
                    "type": "function",
                    "name": "run",
                    "description": "Search",
                    "parameters": {"type": "object", "properties": {}}
                }]
            }]
        }))
        .expect("request should deserialize");

        let model = nan_harness_core::coding_model_profile("qwen3.6").expect("known model");
        let translated = translate(request, &model).expect("request should translate");
        assert_eq!(translated.body["tools"][0]["function"]["name"], "web__run");
        assert_eq!(
            translated.tools.target("web__run"),
            Some(&ToolTarget::Function {
                name: "run".to_owned(),
                namespace: Some("web".to_owned())
            })
        );
    }

    #[test]
    fn converts_freeform_calls_to_chat_function_arguments() {
        let request: ResponsesRequest = serde_json::from_value(json!({
            "model": "qwen3.6",
            "stream": true,
            "input": [
                {"type":"message","role":"user","content":[{"type":"input_text","text":"edit"}]},
                {"type":"custom_tool_call","name":"apply_patch","call_id":"call_1","input":"*** Begin Patch"},
                {"type":"custom_tool_call_output","call_id":"call_1","output":"Done!"}
            ],
            "tools": [{
                "type":"custom",
                "name":"apply_patch",
                "description":"Edit files",
                "format":{"type":"grammar","syntax":"lark","definition":"start: patch"}
            }]
        }))
        .expect("request should deserialize");

        let model = nan_harness_core::coding_model_profile("qwen3.6").expect("known model");
        let translated = translate(request, &model).expect("request should translate");
        assert_eq!(
            translated.body["messages"][1]["tool_calls"][0]["function"]["name"],
            "apply_patch"
        );
        assert_eq!(translated.body["messages"][2]["role"], "tool");
    }

    #[test]
    fn accepts_messages_without_an_explicit_type() {
        let request: ResponsesRequest = serde_json::from_value(json!({
            "model": "qwen3.6",
            "stream": true,
            "input": [{
                "role": "user",
                "content": [{"type": "input_text", "text": "inspect the workspace"}]
            }]
        }))
        .expect("request should deserialize");

        let model = nan_harness_core::coding_model_profile("qwen3.6").expect("known model");
        let translated = translate(request, &model).expect("request should translate");
        assert_eq!(translated.body["messages"][0]["role"], "user");
        assert_eq!(
            translated.body["messages"][0]["content"],
            "inspect the workspace"
        );
    }

    #[test]
    fn translates_and_validates_native_reasoning_effort() {
        let request = |model: &str, effort: &str| {
            serde_json::from_value(json!({
                "model": model, "stream": true, "reasoning": {"effort": effort},
                "input": [{"role":"user","content":[{"type":"input_text","text":"think"}]}]
            }))
            .expect("request")
        };

        let qwen = nan_harness_core::coding_model_profile("qwen3.6").expect("model");
        let translated = translate(request("qwen3.6", "none"), &qwen).expect("toggle accepted");
        assert_eq!(
            translated.body["chat_template_kwargs"]["enable_thinking"],
            false
        );

        let deepseek = nan_harness_core::coding_model_profile("deepseek-v4-flash").expect("model");
        let translated =
            translate(request("deepseek-v4-flash", "low"), &deepseek).expect("effort accepted");
        assert_eq!(translated.body["reasoning_effort"], "low");
        let translated = translate(request("deepseek-v4-flash", "xhigh"), &deepseek)
            .expect("extended effort should use the strongest provider effort");
        assert_eq!(translated.body["reasoning_effort"], "high");
        assert!(translate(request("deepseek-v4-flash", "none"), &deepseek).is_err());

        let translated =
            translate(request("qwen3.6", "xhigh"), &qwen).expect("extended toggle accepted");
        assert_eq!(
            translated.body["chat_template_kwargs"]["enable_thinking"],
            true
        );

        let mimo = nan_harness_core::coding_model_profile("mimo-v2.5").expect("model");
        let translated =
            translate(request("mimo-v2.5", "high"), &mimo).expect("always-on state accepted");
        assert!(translated.body.get("reasoning_effort").is_none());
        assert!(translated.body.get("chat_template_kwargs").is_none());
    }

    #[test]
    fn replays_reasoning_content_with_a_tool_call() {
        let request: ResponsesRequest = serde_json::from_value(json!({
            "model":"qwen3.6", "stream":true,
            "input":[
                {"type":"message","role":"user","content":[{"type":"input_text","text":"inspect"}]},
                {"type":"reasoning","summary":[{"type":"summary_text","text":"I should inspect first."}]},
                {"type":"function_call","name":"run","call_id":"call_1","arguments":"{}"}
            ],
            "tools":[{"type":"function","name":"run","parameters":{"type":"object"}}]
        })).expect("request");
        let model = nan_harness_core::coding_model_profile("qwen3.6").expect("model");
        let translated = translate(request, &model).expect("translation");
        assert_eq!(
            translated.body["messages"][1]["reasoning_content"],
            "I should inspect first."
        );
        assert_eq!(
            translated.body["messages"][1]["tool_calls"][0]["id"],
            "call_1"
        );
    }
}
