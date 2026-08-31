use crate::anthropic::auto_mode;
use crate::error::ApiError;
use nan_harness_core::model::{ReasoningEffort, ReasoningPolicy, ReasoningSelection};
use serde::Deserialize;
use serde_json::{Map, Value, json};

#[derive(Debug, Deserialize)]
pub(crate) struct MessagesRequest {
    model: String,
    #[serde(default)]
    system: Option<SystemPrompt>,
    messages: Vec<Message>,
    #[serde(default)]
    tools: Vec<Tool>,
    #[serde(default)]
    tool_choice: Option<ToolChoice>,
    #[serde(default)]
    max_tokens: Option<u64>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    temperature: Option<f64>,
    #[serde(default)]
    top_p: Option<f64>,
    #[serde(default)]
    stop_sequences: Vec<String>,
    #[serde(default)]
    thinking: Option<ThinkingConfig>,
    #[serde(default)]
    output_config: Option<OutputConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ThinkingConfig {
    Disabled,
    Enabled { budget_tokens: u64 },
    Adaptive,
}

#[derive(Debug, Deserialize)]
struct OutputConfig {
    #[serde(default)]
    effort: Option<ReasoningEffort>,
}

impl MessagesRequest {
    pub(crate) fn model(&self) -> &str {
        &self.model
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SystemPrompt {
    Text(String),
    Blocks(Vec<SystemBlock>),
}

#[derive(Debug, Deserialize)]
struct SystemBlock {
    #[serde(rename = "type")]
    kind: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct Message {
    role: Role,
    content: MessageContent,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Role {
    User,
    Assistant,
    System,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentBlock {
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
enum ImageSource {
    Base64 { media_type: String, data: String },
    Url { url: String },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ToolResultContent {
    Text(String),
    Blocks(Vec<ToolResultBlock>),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ToolResultBlock {
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
enum Tool {
    Client(ClientTool),
    Server(ServerTool),
}

#[derive(Debug, Deserialize)]
struct ClientTool {
    name: String,
    #[serde(default)]
    description: Option<String>,
    input_schema: Value,
}

#[derive(Debug, Deserialize)]
struct ServerTool {
    #[serde(rename = "type")]
    kind: String,
    name: String,
    #[serde(default)]
    max_uses: Option<usize>,
    #[serde(default)]
    allowed_domains: Vec<String>,
    #[serde(default)]
    blocked_domains: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ToolChoice {
    #[serde(rename = "type")]
    kind: ToolChoiceKind,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    disable_parallel_tool_use: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ToolChoiceKind {
    Auto,
    Any,
    Tool,
    None,
}

#[derive(Debug)]
pub(crate) struct TranslatedRequest {
    pub(crate) body: Value,
    pub(crate) stream: bool,
    pub(crate) auto_mode_stage: Option<auto_mode::ClassifierStage>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct WebSearchInvocation {
    pub(crate) query: String,
    pub(crate) max_results: usize,
    pub(crate) allowed_domains: Vec<String>,
    pub(crate) blocked_domains: Vec<String>,
    pub(crate) stream: bool,
}

const WEB_SEARCH_PROMPT_PREFIX: &str = "Perform a web search for the query: ";
const WEB_SEARCH_TOOL_TYPE: &str = "web_search_20250305";

pub(crate) fn translate(
    request: MessagesRequest,
    model: &str,
    max_output_tokens: u64,
    reasoning_policy: ReasoningPolicy,
) -> Result<TranslatedRequest, ApiError> {
    if request.messages.is_empty() {
        return Err(ApiError::InvalidRequest(
            "messages must contain at least one message".to_owned(),
        ));
    }
    let max_tokens = request.max_tokens.ok_or_else(|| {
        ApiError::InvalidRequest("max_tokens is required for message generation".to_owned())
    })?;
    if max_tokens == 0 {
        return Err(ApiError::InvalidRequest(
            "max_tokens must be greater than zero".to_owned(),
        ));
    }
    let classifier_stage = classifier_stage(&request)?;

    let mut messages = Vec::new();
    let mut system_parts = Vec::new();
    if let Some(system) = request.system {
        system_parts.push(system_text(system)?);
    }
    for message in request.messages {
        match message {
            Message {
                role: Role::System,
                content,
            } => system_parts.push(system_content_text(content)?),
            other => translate_message(other, &mut messages)?,
        }
    }
    if !system_parts.is_empty() {
        messages.insert(
            0,
            json!({"role": "system", "content": system_parts.join("\n\n")}),
        );
    }

    let mut body = Map::from_iter([
        ("model".to_owned(), Value::String(model.to_owned())),
        ("messages".to_owned(), Value::Array(messages)),
        (
            "max_tokens".to_owned(),
            Value::Number(max_tokens.min(max_output_tokens).into()),
        ),
        ("stream".to_owned(), Value::Bool(request.stream)),
    ]);
    if request.stream {
        body.insert("stream_options".to_owned(), json!({"include_usage": true}));
    }
    if let Some(temperature) = request.temperature {
        insert_number(&mut body, "temperature", temperature)?;
    }
    if let Some(top_p) = request.top_p {
        insert_number(&mut body, "top_p", top_p)?;
    }
    if !request.stop_sequences.is_empty() {
        body.insert("stop".to_owned(), json!(request.stop_sequences));
    }
    if !request.tools.is_empty() {
        body.insert(
            "tools".to_owned(),
            Value::Array(
                request
                    .tools
                    .into_iter()
                    .map(translate_tool)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        );
    }
    if let Some(choice) = request.tool_choice {
        translate_tool_choice(choice, &mut body)?;
    }
    translate_thinking(
        request.thinking,
        request.output_config.and_then(|config| config.effort),
        reasoning_policy,
        &mut body,
    )?;
    if let Some(stage) = classifier_stage {
        auto_mode::tune_for_qwen(stage, &mut body);
    }

    Ok(TranslatedRequest {
        body: Value::Object(body),
        stream: request.stream,
        auto_mode_stage: classifier_stage,
    })
}

fn translate_thinking(
    thinking: Option<ThinkingConfig>,
    effort: Option<ReasoningEffort>,
    policy: ReasoningPolicy,
    body: &mut Map<String, Value>,
) -> Result<(), ApiError> {
    let selection = match (thinking, effort) {
        (None, None) => ReasoningSelection::Auto,
        (None | Some(ThinkingConfig::Adaptive), effort) => {
            adaptive_reasoning_selection(policy, effort)
        }
        (Some(ThinkingConfig::Disabled), None) => ReasoningSelection::Toggle(false),
        (Some(ThinkingConfig::Enabled { budget_tokens }), None) => {
            if budget_tokens < 1_024 {
                return Err(ApiError::InvalidRequest(
                    "thinking.budget_tokens must be at least 1024".to_owned(),
                ));
            }
            ReasoningSelection::Toggle(true)
        }
        (Some(ThinkingConfig::Disabled | ThinkingConfig::Enabled { .. }), Some(_)) => {
            return Err(ApiError::InvalidRequest(
                "output_config.effort requires thinking.type 'adaptive'".to_owned(),
            ));
        }
    };
    if selection == ReasoningSelection::Auto {
        return Ok(());
    }
    if !policy.accepts(selection) {
        return Err(ApiError::InvalidRequest(
            "requested thinking configuration is not supported by this model's reasoning policy"
                .to_owned(),
        ));
    }
    match selection {
        ReasoningSelection::Toggle(enabled) => {
            body.insert(
                "chat_template_kwargs".to_owned(),
                json!({"enable_thinking": enabled}),
            );
        }
        ReasoningSelection::Effort(effort) => {
            body.insert(
                "reasoning_effort".to_owned(),
                serde_json::to_value(effort).expect("reasoning effort serializes"),
            );
        }
        ReasoningSelection::Auto => {}
    }
    Ok(())
}

fn adaptive_reasoning_selection(
    policy: ReasoningPolicy,
    effort: Option<ReasoningEffort>,
) -> ReasoningSelection {
    match (policy, effort) {
        (ReasoningPolicy::Effort { .. }, Some(effort)) => ReasoningSelection::Effort(effort),
        (ReasoningPolicy::Toggle { .. } | ReasoningPolicy::AlwaysOn, Some(_)) => {
            ReasoningSelection::Toggle(true)
        }
        (_, None) => policy.default_selection(),
        (ReasoningPolicy::Unsupported | ReasoningPolicy::Unknown, Some(_)) => {
            ReasoningSelection::Auto
        }
    }
}

fn classifier_stage(
    request: &MessagesRequest,
) -> Result<Option<auto_mode::ClassifierStage>, ApiError> {
    let has_qualified_policy = auto_mode::policy_markers().into_iter().all(|marker| {
        request
            .system
            .as_ref()
            .is_some_and(|system| system_contains(system, marker))
    });
    let final_message = request.messages.last();
    let stage_marker = if final_message
        .is_some_and(|message| message_contains(message, auto_mode::stage_one_marker()))
    {
        Some(auto_mode::ClassifierStage::One)
    } else if final_message
        .is_some_and(|message| message_contains(message, auto_mode::stage_two_marker()))
    {
        Some(auto_mode::ClassifierStage::Two)
    } else {
        None
    };
    auto_mode::detect(&auto_mode::RequestFingerprint {
        model: &request.model,
        max_tokens: request.max_tokens,
        shape: if request.stream || !request.tools.is_empty() {
            auto_mode::RequestShape::Other
        } else {
            auto_mode::RequestShape::ClassifierCandidate
        },
        policy: if has_qualified_policy {
            auto_mode::PolicyFingerprint::Qualified
        } else {
            auto_mode::PolicyFingerprint::Unknown
        },
        stage_marker,
    })
}

fn system_contains(system: &SystemPrompt, needle: &str) -> bool {
    match system {
        SystemPrompt::Text(text) => text.contains(needle),
        SystemPrompt::Blocks(blocks) => blocks.iter().any(|block| block.text.contains(needle)),
    }
}

fn message_contains(message: &Message, needle: &str) -> bool {
    match &message.content {
        MessageContent::Text(text) => text.contains(needle),
        MessageContent::Blocks(blocks) => blocks.iter().any(|block| match block {
            ContentBlock::Text { text } => text.contains(needle),
            ContentBlock::Image { .. }
            | ContentBlock::Thinking { .. }
            | ContentBlock::ToolUse { .. }
            | ContentBlock::ToolResult { .. }
            | ContentBlock::Unsupported => false,
        }),
    }
}

pub(crate) fn web_search_invocation(
    request: &MessagesRequest,
) -> Result<Option<WebSearchInvocation>, ApiError> {
    let server_tools = request
        .tools
        .iter()
        .filter_map(|tool| match tool {
            Tool::Client(_) => None,
            Tool::Server(tool) => Some(tool),
        })
        .collect::<Vec<_>>();
    if server_tools.is_empty() {
        return Ok(None);
    }
    if request.tools.len() != 1 || server_tools.len() != 1 {
        return Err(ApiError::InvalidRequest(
            "server tools cannot be mixed with client tools in this release".to_owned(),
        ));
    }

    let tool = server_tools[0];
    if tool.kind != WEB_SEARCH_TOOL_TYPE || tool.name != "web_search" {
        return Err(ApiError::InvalidRequest(format!(
            "unsupported Anthropic server tool '{}'",
            tool.kind
        )));
    }
    if !tool.allowed_domains.is_empty() && !tool.blocked_domains.is_empty() {
        return Err(ApiError::InvalidRequest(
            "web search cannot combine allowed_domains and blocked_domains".to_owned(),
        ));
    }
    validate_domains(&tool.allowed_domains)?;
    validate_domains(&tool.blocked_domains)?;
    let query = request
        .messages
        .iter()
        .rev()
        .find_map(web_search_query)
        .ok_or_else(|| {
            ApiError::InvalidRequest(
                "web search request did not contain Claude Code's search query".to_owned(),
            )
        })?;
    let max_results = tool.max_uses.unwrap_or(5);
    if max_results == 0 {
        return Err(ApiError::InvalidRequest(
            "web search max_uses must be greater than zero".to_owned(),
        ));
    }

    Ok(Some(WebSearchInvocation {
        query,
        max_results: max_results.min(20),
        allowed_domains: tool.allowed_domains.clone(),
        blocked_domains: tool.blocked_domains.clone(),
        stream: request.stream,
    }))
}

pub(crate) fn estimate_input_tokens(request: &MessagesRequest) -> u64 {
    let serialized_bytes = serde_json::to_vec(&json!({
        "system": request.system.as_ref().map(system_size),
        "message_count": request.messages.len(),
        "messages": request.messages.iter().map(message_size).collect::<Vec<_>>(),
        "tools": request.tools.iter().map(tool_size).collect::<Vec<_>>(),
    }))
    .map_or(0, |value| value.len());
    u64::try_from(serialized_bytes.div_ceil(3).saturating_add(16)).unwrap_or(u64::MAX)
}

fn system_text(system: SystemPrompt) -> Result<String, ApiError> {
    match system {
        SystemPrompt::Text(text) => Ok(text),
        SystemPrompt::Blocks(blocks) => blocks
            .into_iter()
            .map(|block| {
                if block.kind == "text" {
                    Ok(block.text)
                } else {
                    Err(ApiError::InvalidRequest(format!(
                        "unsupported system content block '{}'",
                        block.kind
                    )))
                }
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|parts| parts.join("\n\n")),
    }
}

fn translate_message(message: Message, output: &mut Vec<Value>) -> Result<(), ApiError> {
    match (message.role, message.content) {
        (Role::User, MessageContent::Text(text)) => {
            output.push(json!({"role": "user", "content": text}));
        }
        (Role::Assistant, MessageContent::Text(text)) => {
            output.push(json!({"role": "assistant", "content": text}));
        }
        (Role::User, MessageContent::Blocks(blocks)) => translate_user_blocks(blocks, output)?,
        (Role::Assistant, MessageContent::Blocks(blocks)) => {
            output.push(translate_assistant_blocks(blocks)?);
        }
        (Role::System, _) => unreachable!("system messages are normalized before translation"),
    }
    Ok(())
}

fn system_content_text(content: MessageContent) -> Result<String, ApiError> {
    match content {
        MessageContent::Text(text) => Ok(text),
        MessageContent::Blocks(blocks) => blocks
            .into_iter()
            .map(|block| match block {
                ContentBlock::Text { text } => Ok(text),
                ContentBlock::Image { .. }
                | ContentBlock::Thinking { .. }
                | ContentBlock::ToolUse { .. }
                | ContentBlock::ToolResult { .. }
                | ContentBlock::Unsupported => Err(ApiError::InvalidRequest(
                    "system messages may only contain text blocks".to_owned(),
                )),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|parts| parts.join("\n\n")),
    }
}

fn translate_user_blocks(
    blocks: Vec<ContentBlock>,
    output: &mut Vec<Value>,
) -> Result<(), ApiError> {
    let mut user_content = Vec::new();
    for block in blocks {
        match block {
            ContentBlock::Text { text } => {
                user_content.push(json!({"type": "text", "text": text}));
            }
            ContentBlock::Image { source } => {
                user_content.push(translate_image(source));
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                flush_user_content(&mut user_content, output);
                let mut content = translate_tool_result_content(content)?;
                if is_error {
                    prefix_tool_error(&mut content);
                }
                output.push(json!({
                    "role": "tool",
                    "tool_call_id": tool_use_id,
                    "content": content
                }));
            }
            ContentBlock::ToolUse { .. } => {
                return Err(ApiError::InvalidRequest(
                    "tool_use blocks are only valid in assistant messages".to_owned(),
                ));
            }
            ContentBlock::Thinking { .. } => {
                return Err(ApiError::InvalidRequest(
                    "thinking blocks are only valid in assistant messages".to_owned(),
                ));
            }
            ContentBlock::Unsupported => return unsupported_content(),
        }
    }
    flush_user_content(&mut user_content, output);
    Ok(())
}

fn translate_assistant_blocks(blocks: Vec<ContentBlock>) -> Result<Value, ApiError> {
    let mut text = Vec::new();
    let mut reasoning = Vec::new();
    let mut tool_calls = Vec::new();
    for block in blocks {
        match block {
            ContentBlock::Text { text: value } => text.push(value),
            ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                let _ = signature;
                reasoning.push(thinking);
            }
            ContentBlock::ToolUse { id, name, input } => {
                tool_calls.push(json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": input.to_string()
                    }
                }));
            }
            ContentBlock::Image { .. } | ContentBlock::ToolResult { .. } => {
                return Err(ApiError::InvalidRequest(
                    "assistant messages may only contain text and tool_use blocks".to_owned(),
                ));
            }
            ContentBlock::Unsupported => return unsupported_content(),
        }
    }

    let mut message = Map::from_iter([
        ("role".to_owned(), Value::String("assistant".to_owned())),
        ("content".to_owned(), Value::String(text.join(""))),
    ]);
    // NaN's Chat Completions dialect uses reasoning_content for replayed
    // assistant reasoning. Keep it distinct from visible assistant content.
    if !reasoning.is_empty() {
        message.insert(
            "reasoning_content".to_owned(),
            Value::String(reasoning.join("")),
        );
    }
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_owned(), Value::Array(tool_calls));
    }
    Ok(Value::Object(message))
}

fn translate_image(source: ImageSource) -> Value {
    let url = match source {
        ImageSource::Base64 { media_type, data } => format!("data:{media_type};base64,{data}"),
        ImageSource::Url { url } => url,
    };
    json!({"type": "image_url", "image_url": {"url": url}})
}

fn translate_tool_result_content(content: ToolResultContent) -> Result<Value, ApiError> {
    match content {
        ToolResultContent::Text(text) => Ok(Value::String(text)),
        ToolResultContent::Blocks(blocks) => {
            let mut translated = Vec::with_capacity(blocks.len());
            let mut contains_image = false;
            for block in blocks {
                match block {
                    ToolResultBlock::Text { text } => {
                        translated.push(json!({"type": "text", "text": text}));
                    }
                    ToolResultBlock::Image { source } => {
                        contains_image = true;
                        translated.push(translate_image(source));
                    }
                    ToolResultBlock::Unsupported => {
                        return Err(ApiError::InvalidRequest(
                            "tool_result content only supports text and image blocks in this release"
                                .to_owned(),
                        ));
                    }
                }
            }
            if contains_image {
                Ok(Value::Array(translated))
            } else {
                Ok(Value::String(
                    translated
                        .iter()
                        .filter_map(|part| part.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("\n"),
                ))
            }
        }
    }
}

fn prefix_tool_error(content: &mut Value) {
    match content {
        Value::String(text) => text.insert_str(0, "Tool error: "),
        Value::Array(parts) => parts.insert(0, json!({"type": "text", "text": "Tool error"})),
        _ => unreachable!("translated tool results are strings or content arrays"),
    }
}

fn flush_user_content(content: &mut Vec<Value>, output: &mut Vec<Value>) {
    if content.is_empty() {
        return;
    }
    let value = if content.len() == 1 && content[0].get("type") == Some(&json!("text")) {
        content[0]
            .get("text")
            .cloned()
            .unwrap_or(Value::String(String::new()))
    } else {
        Value::Array(std::mem::take(content))
    };
    content.clear();
    output.push(json!({"role": "user", "content": value}));
}

fn translate_tool(tool: Tool) -> Result<Value, ApiError> {
    let Tool::Client(tool) = tool else {
        return Err(ApiError::InvalidRequest(
            "Anthropic server tool requests require the dedicated bridge handler".to_owned(),
        ));
    };
    let mut function = Map::from_iter([
        ("name".to_owned(), Value::String(tool.name)),
        ("parameters".to_owned(), tool.input_schema),
    ]);
    if let Some(description) = tool.description {
        function.insert("description".to_owned(), Value::String(description));
    }
    Ok(json!({"type": "function", "function": function}))
}

fn translate_tool_choice(
    choice: ToolChoice,
    body: &mut Map<String, Value>,
) -> Result<(), ApiError> {
    let value = match choice.kind {
        ToolChoiceKind::Auto => Value::String("auto".to_owned()),
        ToolChoiceKind::Any => Value::String("required".to_owned()),
        ToolChoiceKind::None => Value::String("none".to_owned()),
        ToolChoiceKind::Tool => {
            let name = choice.name.ok_or_else(|| {
                ApiError::InvalidRequest("tool_choice.type 'tool' requires a name".to_owned())
            })?;
            json!({"type": "function", "function": {"name": name}})
        }
    };
    body.insert("tool_choice".to_owned(), value);
    body.insert(
        "parallel_tool_calls".to_owned(),
        Value::Bool(!choice.disable_parallel_tool_use),
    );
    Ok(())
}

fn insert_number(body: &mut Map<String, Value>, name: &str, value: f64) -> Result<(), ApiError> {
    let number = serde_json::Number::from_f64(value)
        .ok_or_else(|| ApiError::InvalidRequest(format!("{name} must be a finite number")))?;
    body.insert(name.to_owned(), Value::Number(number));
    Ok(())
}

fn unsupported_content<T>() -> Result<T, ApiError> {
    Err(ApiError::InvalidRequest(
        "request contains an unsupported content block".to_owned(),
    ))
}

fn system_size(system: &SystemPrompt) -> usize {
    match system {
        SystemPrompt::Text(text) => text.len(),
        SystemPrompt::Blocks(blocks) => blocks.iter().map(|block| block.text.len()).sum(),
    }
}

fn message_size(message: &Message) -> usize {
    match &message.content {
        MessageContent::Text(text) => text.len(),
        MessageContent::Blocks(blocks) => blocks.len().saturating_mul(32),
    }
}

fn tool_size(tool: &Tool) -> usize {
    match tool {
        Tool::Client(tool) => {
            tool.name.len()
                + tool.description.as_ref().map_or(0, String::len)
                + tool.input_schema.to_string().len()
        }
        Tool::Server(tool) => {
            tool.kind.len()
                + tool.name.len()
                + tool.allowed_domains.iter().map(String::len).sum::<usize>()
                + tool.blocked_domains.iter().map(String::len).sum::<usize>()
        }
    }
}

fn web_search_query(message: &Message) -> Option<String> {
    if !matches!(message.role, Role::User) {
        return None;
    }
    let text = match &message.content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                ContentBlock::Image { .. }
                | ContentBlock::Thinking { .. }
                | ContentBlock::ToolUse { .. }
                | ContentBlock::ToolResult { .. }
                | ContentBlock::Unsupported => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    };
    text.strip_prefix(WEB_SEARCH_PROMPT_PREFIX)
        .map(str::trim)
        .filter(|query| query.len() >= 2)
        .map(ToOwned::to_owned)
}

fn validate_domains(domains: &[String]) -> Result<(), ApiError> {
    if domains.iter().any(|domain| {
        domain.is_empty()
            || domain.contains("://")
            || domain.chars().any(char::is_whitespace)
            || domain.starts_with('.')
    }) {
        return Err(ApiError::InvalidRequest(
            "web search domains must be bare hostnames with optional paths".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        MessagesRequest, WebSearchInvocation, estimate_input_tokens, translate,
        web_search_invocation,
    };
    use nan_harness_core::model::ReasoningPolicy;
    use serde_json::{Value, json};

    #[test]
    fn translates_claude_tool_round_trip() {
        let request: MessagesRequest = serde_json::from_value(json!({
            "model": "ignored-by-bridge",
            "max_tokens": 100_000,
            "stream": true,
            "system": [{"type": "text", "text": "Be precise"}],
            "tools": [{
                "name": "Read",
                "description": "Reads a file",
                "input_schema": {"type": "object", "properties": {"file_path": {"type": "string"}}}
            }],
            "messages": [
                {"role": "user", "content": "Read the file"},
                {"role": "assistant", "content": [{
                    "type": "tool_use", "id": "tool_1", "name": "Read",
                    "input": {"file_path": "/tmp/probe.txt"}
                }]},
                {"role": "user", "content": [{
                    "type": "tool_result", "tool_use_id": "tool_1", "content": "hello"
                }]}
            ]
        }))
        .expect("fixture should deserialize");

        let translated = translate(
            request,
            "qwen3.6",
            65_536,
            ReasoningPolicy::Toggle {
                default_enabled: true,
            },
        )
        .expect("translation should work");
        assert!(translated.stream);
        assert_eq!(translated.body["model"], "qwen3.6");
        assert_eq!(translated.body["max_tokens"], 65_536);
        assert_eq!(
            translated.body["messages"][2]["tool_calls"][0]["function"]["name"],
            "Read"
        );
        assert_eq!(translated.body["messages"][3]["role"], "tool");
        assert_eq!(translated.body["stream_options"]["include_usage"], true);
    }

    #[test]
    fn estimates_tokens_without_contacting_nan() {
        let request: MessagesRequest = serde_json::from_value(json!({
            "model": "qwen3.6",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "A moderately sized prompt"}]
        }))
        .expect("fixture should deserialize");

        assert!(estimate_input_tokens(&request) > 16);
    }

    #[test]
    fn rejects_unknown_content_blocks() {
        let request: MessagesRequest = serde_json::from_value(json!({
            "model": "qwen3.6",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": [{"type": "document"}]}]
        }))
        .expect("unknown variants should deserialize");

        let error = translate(
            request,
            "qwen3.6",
            100,
            ReasoningPolicy::Toggle {
                default_enabled: true,
            },
        )
        .expect_err("translation must fail");
        assert_eq!(error.code(), "NH-BRIDGE-102");
    }

    #[test]
    fn emits_image_urls_for_base64_images() {
        let request: MessagesRequest = serde_json::from_value(json!({
            "model": "qwen3.6",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": [{
                "type": "image",
                "source": {"type": "base64", "media_type": "image/png", "data": "AA=="}
            }]}]
        }))
        .expect("fixture should deserialize");

        let translated = translate(
            request,
            "qwen3.6",
            100,
            ReasoningPolicy::Toggle {
                default_enabled: true,
            },
        )
        .expect("translation should work");
        let url: &Value = &translated.body["messages"][0]["content"][0]["image_url"]["url"];
        assert_eq!(url, "data:image/png;base64,AA==");
    }

    #[test]
    fn forwards_images_for_the_new_nan_models_without_profile_gating() {
        for model_id in ["qwen3.8-flash", "glm5.3-flash"] {
            let request: MessagesRequest = serde_json::from_value(json!({
                "model": model_id,
                "max_tokens": 100,
                "messages": [{"role": "user", "content": [{
                    "type": "image",
                    "source": {"type": "base64", "media_type": "image/png", "data": "AA=="}
                }]}]
            }))
            .expect("fixture should deserialize");
            let model = nan_harness_core::coding_model_profile(model_id)
                .expect("new NaN model should be profiled");

            let translated = translate(request, model_id, model.max_output_tokens, model.reasoning)
                .expect("translation should work");
            let url = &translated.body["messages"][0]["content"][0]["image_url"]["url"];
            assert_eq!(url, "data:image/png;base64,AA==");
        }
    }

    #[test]
    fn moves_claude_system_messages_to_the_first_upstream_position() {
        let request: MessagesRequest = serde_json::from_value(json!({
            "model": "qwen3.6",
            "max_tokens": 100,
            "system": "primary system prompt",
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "system", "content": [{"type": "text", "text": "runtime context"}]}
            ]
        }))
        .expect("fixture should deserialize");

        let translated = translate(
            request,
            "qwen3.6",
            100,
            ReasoningPolicy::Toggle {
                default_enabled: true,
            },
        )
        .expect("translation should work");
        assert_eq!(translated.body["messages"][0]["role"], "system");
        assert_eq!(
            translated.body["messages"][0]["content"],
            "primary system prompt\n\nruntime context"
        );
        assert_eq!(translated.body["messages"][1]["role"], "user");
    }

    #[test]
    fn recognizes_claude_code_web_search_server_requests() {
        let request: MessagesRequest = serde_json::from_value(json!({
            "model": "qwen3.6",
            "max_tokens": 32_000,
            "stream": true,
            "tools": [{
                "type": "web_search_20250305",
                "name": "web_search",
                "max_uses": 8
            }],
            "tool_choice": {"type": "tool", "name": "web_search"},
            "messages": [{
                "role": "user",
                "content": "Perform a web search for the query: best Rust async runtime 2025"
            }]
        }))
        .expect("server tool fixture should deserialize");

        let invocation = web_search_invocation(&request)
            .expect("server tool should be valid")
            .expect("web search should be recognized");
        assert_eq!(
            invocation,
            WebSearchInvocation {
                query: "best Rust async runtime 2025".to_owned(),
                max_results: 8,
                allowed_domains: Vec::new(),
                blocked_domains: Vec::new(),
                stream: true,
            }
        );
    }
}
