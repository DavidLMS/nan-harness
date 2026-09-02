use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub(crate) struct ResponsesRequest {
    pub(crate) model: String,
    #[serde(default)]
    pub(super) instructions: String,
    #[serde(default)]
    pub(super) input: Vec<Value>,
    #[serde(default)]
    pub(super) tools: Vec<Value>,
    #[serde(default = "default_tool_choice")]
    pub(super) tool_choice: Value,
    #[serde(default)]
    pub(super) parallel_tool_calls: bool,
    #[serde(default)]
    pub(super) reasoning: Option<ResponsesReasoning>,
    #[serde(default)]
    pub(crate) stream: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct ResponsesReasoning {
    pub(super) effort: String,
}

fn default_tool_choice() -> Value {
    Value::String("auto".to_owned())
}
