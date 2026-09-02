use super::validation::required_string;
use crate::error::ApiError;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

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

    pub(super) fn alias_for(&self, target: &ToolTarget) -> Option<&str> {
        self.aliases
            .iter()
            .find_map(|(alias, candidate)| (candidate == target).then_some(alias.as_str()))
    }
}

pub(super) fn translate_tools(tools: &[Value]) -> Result<(Vec<Value>, ToolCatalog), ApiError> {
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
