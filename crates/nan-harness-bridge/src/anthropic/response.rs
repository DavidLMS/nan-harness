use crate::error::ApiError;
use crate::usage::UsageValues;
use serde::Deserialize;
use serde_json::{Map, Value, json};

#[derive(Debug, Deserialize)]
struct Completion {
    id: String,
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Usage,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: AssistantMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AssistantMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Deserialize)]
struct ToolCall {
    id: String,
    function: FunctionCall,
}

#[derive(Debug, Deserialize)]
struct FunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Default, Deserialize)]
struct Usage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}

pub(crate) fn translate(value: Value, configured_model: &str) -> Result<Value, ApiError> {
    let completion: Completion = serde_json::from_value(value)
        .map_err(|error| ApiError::InvalidUpstream(error.to_string()))?;
    let choice = completion.choices.into_iter().next().ok_or_else(|| {
        ApiError::InvalidUpstream("response did not contain a completion choice".to_owned())
    })?;
    let mut content = Vec::new();
    if let Some(thinking) = choice
        .message
        .reasoning_content
        .filter(|thinking| !thinking.is_empty())
    {
        content.push(json!({
            "type": "thinking",
            "thinking": thinking,
            "signature": "nan-harness"
        }));
    }
    if let Some(text) = choice.message.content.filter(|text| !text.is_empty()) {
        content.push(json!({"type": "text", "text": text}));
    }
    for tool_call in choice.message.tool_calls {
        let input: Value =
            serde_json::from_str(&tool_call.function.arguments).map_err(|error| {
                ApiError::InvalidUpstream(format!(
                    "tool '{}' returned invalid JSON arguments: {error}",
                    tool_call.function.name
                ))
            })?;
        if !input.is_object() {
            return Err(ApiError::InvalidUpstream(format!(
                "tool '{}' arguments were not a JSON object",
                tool_call.function.name
            )));
        }
        content.push(json!({
            "type": "tool_use",
            "id": tool_call.id,
            "name": tool_call.function.name,
            "input": input
        }));
    }

    let stop_reason = map_finish_reason(
        choice.finish_reason.as_deref(),
        content.iter().any(|block| block["type"] == "tool_use"),
    );
    let mut response = Map::from_iter([
        ("id".to_owned(), Value::String(completion.id)),
        ("type".to_owned(), Value::String("message".to_owned())),
        ("role".to_owned(), Value::String("assistant".to_owned())),
        (
            "model".to_owned(),
            Value::String(configured_model.to_owned()),
        ),
        ("content".to_owned(), Value::Array(content)),
        (
            "stop_reason".to_owned(),
            Value::String(stop_reason.to_owned()),
        ),
        ("stop_sequence".to_owned(), Value::Null),
        (
            "usage".to_owned(),
            json!({
                "input_tokens": completion.usage.prompt_tokens,
                "output_tokens": completion.usage.completion_tokens
            }),
        ),
    ]);
    response.insert("container".to_owned(), Value::Null);
    Ok(Value::Object(response))
}

pub(crate) fn provider_usage(value: &Value) -> Option<UsageValues> {
    let usage = value.get("usage")?.as_object()?;
    Some(UsageValues {
        input: usage.get("prompt_tokens")?.as_u64()?,
        output: usage.get("completion_tokens")?.as_u64()?,
        reasoning: usage
            .get("completion_tokens_details")
            .and_then(Value::as_object)
            .and_then(|details| details.get("reasoning_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
    })
}

pub(crate) fn map_finish_reason(reason: Option<&str>, has_tools: bool) -> &'static str {
    if has_tools || reason == Some("tool_calls") || reason == Some("function_call") {
        "tool_use"
    } else {
        match reason {
            Some("length") => "max_tokens",
            Some("content_filter") => "refusal",
            _ => "end_turn",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::translate;
    use serde_json::json;

    #[test]
    fn translates_text_and_tool_calls() {
        let response = translate(
            json!({
                "id": "chat_123",
                "model": "qwen3.6",
                "choices": [{
                    "message": {
                        "content": "I'll read it.",
                        "tool_calls": [{
                            "id": "call_1",
                            "type": "function",
                            "function": {"name": "Read", "arguments": "{\"file_path\":\"README.md\"}"}
                        }]
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": {"prompt_tokens": 20, "completion_tokens": 8}
            }),
            "qwen3.6",
        )
        .expect("translation should work");

        assert_eq!(response["content"][1]["type"], "tool_use");
        assert_eq!(response["content"][1]["input"]["file_path"], "README.md");
        assert_eq!(response["stop_reason"], "tool_use");
        assert_eq!(response["usage"]["output_tokens"], 8);
    }
}
