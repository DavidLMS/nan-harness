use super::chunk::{FxObject, FxToolCallDelta};
use crate::error::ApiError;
use std::collections::BTreeMap;

#[derive(Debug, Default)]
struct FxToolState {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Debug)]
pub(super) struct ParsedFxTool {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) input: serde_json::Value,
}

#[derive(Debug, Default)]
pub(super) struct FxTools {
    states: BTreeMap<usize, FxToolState>,
}

impl FxTools {
    pub(super) fn update(&mut self, call: FxToolCallDelta) {
        let tool = self.states.entry(call.index).or_default();
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

    pub(super) fn parse(&self) -> Result<Vec<ParsedFxTool>, ApiError> {
        self.states
            .values()
            .map(|tool| {
                if tool.id.trim().is_empty() || tool.name.trim().is_empty() {
                    return Err(ApiError::InvalidUpstream(
                        "tool call ended without a valid id or name".to_owned(),
                    ));
                }
                let input = serde_json::from_str::<serde_json::Value>(&tool.arguments)
                    .ok()
                    .filter(serde_json::Value::is_object)
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

    pub(super) fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    pub(super) fn has_named(&self, name: Option<&str>) -> bool {
        self.states
            .values()
            .any(|tool| name.is_some_and(|name| name == tool.name))
    }

    pub(super) fn all_named(&self, name: Option<&str>) -> bool {
        self.states
            .values()
            .all(|tool| name.is_some_and(|name| name == tool.name))
    }
}
