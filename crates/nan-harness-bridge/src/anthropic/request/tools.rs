use super::wire::{Message, MessageContent, Tool, ToolChoice, ToolChoiceKind};
use crate::error::ApiError;
use serde_json::{Map, Value, json};

const WEB_SEARCH_PROMPT_PREFIX: &str = "Perform a web search for the query: ";
const WEB_SEARCH_TOOL_TYPE: &str = "web_search_20250305";

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct WebSearchInvocation {
    pub(crate) query: String,
    pub(crate) max_results: usize,
    pub(crate) allowed_domains: Vec<String>,
    pub(crate) blocked_domains: Vec<String>,
    pub(crate) stream: bool,
}

pub(super) fn translate_tools(tools: Vec<Tool>) -> Result<Vec<Value>, ApiError> {
    tools.into_iter().map(translate_tool).collect()
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

pub(super) fn translate_tool_choice(
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

pub(super) fn web_search_invocation(
    request: &super::wire::MessagesRequest,
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

fn web_search_query(message: &Message) -> Option<String> {
    if !matches!(message.role, super::wire::Role::User) {
        return None;
    }
    let text = match &message.content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                super::wire::ContentBlock::Text { text } => Some(text.as_str()),
                super::wire::ContentBlock::Image { .. }
                | super::wire::ContentBlock::Thinking { .. }
                | super::wire::ContentBlock::ToolUse { .. }
                | super::wire::ContentBlock::ToolResult { .. }
                | super::wire::ContentBlock::Unsupported => None,
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
