use nan_harness_core::model::ReasoningEffort;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub(crate) struct MessagesRequest {
    pub(crate) model: String,
    #[serde(default)]
    pub(crate) system: Option<SystemPrompt>,
    pub(crate) messages: Vec<Message>,
    #[serde(default)]
    pub(crate) tools: Vec<Tool>,
    #[serde(default)]
    pub(crate) tool_choice: Option<ToolChoice>,
    #[serde(default)]
    pub(crate) max_tokens: Option<u64>,
    #[serde(default)]
    pub(crate) stream: bool,
    #[serde(default)]
    pub(crate) temperature: Option<f64>,
    #[serde(default)]
    pub(crate) top_p: Option<f64>,
    #[serde(default)]
    pub(crate) stop_sequences: Vec<String>,
    #[serde(default)]
    pub(crate) thinking: Option<ThinkingConfig>,
    #[serde(default)]
    pub(crate) output_config: Option<OutputConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ThinkingConfig {
    Disabled,
    Enabled { budget_tokens: u64 },
    Adaptive,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OutputConfig {
    #[serde(default)]
    pub(crate) effort: Option<ReasoningEffort>,
}

impl MessagesRequest {
    pub(crate) fn model(&self) -> &str {
        &self.model
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum SystemPrompt {
    Text(String),
    Blocks(Vec<SystemBlock>),
}

#[derive(Debug, Deserialize)]
pub(crate) struct SystemBlock {
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) text: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Message {
    pub(crate) role: Role,
    pub(crate) content: MessageContent,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Role {
    User,
    Assistant,
    System,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ContentBlock {
    Text {
        text: String,
    },
    Image {
        source: ImageSource,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: ToolResultContent,
        #[serde(default)]
        is_error: bool,
    },
    Thinking {
        thinking: String,
        #[serde(default)]
        signature: String,
    },
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ImageSource {
    Base64 { media_type: String, data: String },
    Url { url: String },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ToolResultContent {
    Text(String),
    Blocks(Vec<ToolResultBlock>),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ToolResultBlock {
    Text {
        text: String,
    },
    Image {
        source: ImageSource,
    },
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum Tool {
    Client(ClientTool),
    Server(ServerTool),
}

#[derive(Debug, Deserialize)]
pub(crate) struct ClientTool {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: Option<String>,
    pub(crate) input_schema: Value,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ServerTool {
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) max_uses: Option<usize>,
    #[serde(default)]
    pub(crate) allowed_domains: Vec<String>,
    #[serde(default)]
    pub(crate) blocked_domains: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ToolChoice {
    #[serde(rename = "type")]
    pub(crate) kind: ToolChoiceKind,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) disable_parallel_tool_use: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolChoiceKind {
    Auto,
    Any,
    Tool,
    None,
}
