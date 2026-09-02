use super::tools::{ToolCatalog, ToolTarget};
use serde_json::{Value, json};

pub(super) fn translate_tool_choice(choice: &Value, catalog: &ToolCatalog) -> Value {
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
