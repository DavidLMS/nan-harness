use super::chunk::ToolCallDelta;
use super::events::responses_event;
use crate::error::ApiError;
use crate::responses::request::{ToolCatalog, ToolTarget};
use axum::response::sse::Event;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub(super) struct ToolState {
    id: String,
    name: String,
    arguments: String,
}

impl ToolState {
    pub(super) fn apply(&mut self, delta: ToolCallDelta) {
        if let Some(id) = delta.id {
            self.id.push_str(&id);
        }
        if let Some(function) = delta.function {
            if let Some(name) = function.name {
                self.name.push_str(&name);
            }
            if let Some(arguments) = function.arguments {
                self.arguments.push_str(&arguments);
            }
        }
    }

    pub(super) fn buffered_bytes(&self) -> usize {
        self.id
            .len()
            .saturating_add(self.name.len())
            .saturating_add(self.arguments.len())
    }
}

pub(super) fn finish_events(
    states: &BTreeMap<usize, ToolState>,
    tools: &ToolCatalog,
) -> Result<Vec<Event>, ApiError> {
    states
        .values()
        .map(|tool| {
            if tool.id.is_empty() || tool.name.is_empty() {
                return Err(ApiError::InvalidUpstream(
                    "tool call ended without an id and name".to_owned(),
                ));
            }
            tool_event(tool, tools)
        })
        .collect()
}

fn tool_event(tool: &ToolState, tools: &ToolCatalog) -> Result<Event, ApiError> {
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
        Some(ToolTarget::Custom { name }) => {
            let input = custom_input(name, &tool.arguments)?;
            json!({
                "type": "custom_tool_call",
                "call_id": tool.id,
                "name": name,
                "input": input
            })
        }
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
    Ok(responses_event(
        "response.output_item.done",
        &json!({"type": "response.output_item.done", "item": item}),
    ))
}

pub(super) fn normalized_arguments(arguments: &str) -> String {
    if serde_json::from_str::<Value>(arguments).is_ok() {
        arguments.to_owned()
    } else {
        json!({"input": arguments}).to_string()
    }
}

pub(super) fn parsed_arguments(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|_| json!({"input": arguments}))
}

pub(super) fn custom_input(name: &str, arguments: &str) -> Result<String, ApiError> {
    let trimmed = arguments.trim();
    let input = match serde_json::from_str::<Value>(trimmed) {
        Ok(value) => value
            .get("input")
            .and_then(Value::as_str)
            .filter(|input| !input.trim().is_empty())
            .map(str::to_owned)
            .ok_or_else(|| invalid_custom_input(name))?,
        Err(_) if looks_like_json_fragment(trimmed) => return Err(invalid_custom_input(name)),
        Err(_) if !trimmed.is_empty() => arguments.to_owned(),
        Err(_) => return Err(invalid_custom_input(name)),
    };
    if name == "apply_patch" {
        let patch = input.trim();
        if !patch.starts_with("*** Begin Patch") || !patch.ends_with("*** End Patch") {
            return Err(invalid_custom_input(name));
        }
    }
    Ok(input)
}

fn looks_like_json_fragment(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|character| matches!(character, '{' | '[' | '"'))
        || value
            .chars()
            .last()
            .is_some_and(|character| matches!(character, '}' | ']' | '"'))
}

fn invalid_custom_input(name: &str) -> ApiError {
    ApiError::InvalidUpstream(format!(
        "custom tool call '{name}' ended without complete input"
    ))
}
