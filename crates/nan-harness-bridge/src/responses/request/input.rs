use super::tools::{ToolCatalog, ToolTarget};
use super::validation::required_string;
use crate::error::ApiError;
use serde_json::{Value, json};

pub(super) fn translate(
    instructions: &str,
    input: Vec<Value>,
    catalog: &ToolCatalog,
) -> Result<Vec<Value>, ApiError> {
    let mut messages = Vec::new();
    if !instructions.trim().is_empty() {
        messages.push(json!({"role": "system", "content": instructions}));
    }

    let mut pending_reasoning = None;
    for item in input {
        if item.get("type").and_then(Value::as_str) == Some("reasoning") {
            let text = reasoning_text(&item);
            if !text.is_empty() {
                pending_reasoning = Some(text);
            }
            continue;
        }
        let before = messages.len();
        translate_input_item(&item, catalog, &mut messages)?;
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
    Ok(messages)
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
