use super::validation::unsupported_content;
use super::wire::{
    ContentBlock, ImageSource, Message, MessageContent, Role, SystemPrompt, ToolResultBlock,
    ToolResultContent,
};
use crate::error::ApiError;
use serde_json::{Map, Value, json};

pub(super) fn translate(
    system: Option<SystemPrompt>,
    request_messages: Vec<Message>,
) -> Result<Vec<Value>, ApiError> {
    let mut messages = Vec::new();
    let mut system_parts = Vec::new();
    if let Some(system) = system {
        system_parts.push(system_text(system)?);
    }
    for message in request_messages {
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
    Ok(messages)
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

#[cfg(test)]
mod tests {
    use super::translate;
    use crate::anthropic::request::wire::{
        ContentBlock, ImageSource, Message, MessageContent, Role, ToolResultContent,
    };
    use serde_json::json;

    #[test]
    fn translates_mixed_content_to_the_chat_messages_golden() {
        let translated = translate(
            None,
            vec![
                Message {
                    role: Role::User,
                    content: MessageContent::Blocks(vec![
                        ContentBlock::Text {
                            text: "Look at this".to_owned(),
                        },
                        ContentBlock::Image {
                            source: ImageSource::Url {
                                url: "https://example.test/image.png".to_owned(),
                            },
                        },
                    ]),
                },
                Message {
                    role: Role::Assistant,
                    content: MessageContent::Blocks(vec![
                        ContentBlock::Thinking {
                            thinking: "brief reasoning".to_owned(),
                            signature: "synthetic-signature".to_owned(),
                        },
                        ContentBlock::Text {
                            text: "Done".to_owned(),
                        },
                        ContentBlock::ToolUse {
                            id: "call_1".to_owned(),
                            name: "lookup".to_owned(),
                            input: json!({"term": "rust"}),
                        },
                    ]),
                },
                Message {
                    role: Role::User,
                    content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                        tool_use_id: "call_1".to_owned(),
                        content: ToolResultContent::Text("result".to_owned()),
                        is_error: false,
                    }]),
                },
            ],
        )
        .expect("content should translate");

        assert_eq!(
            serde_json::Value::Array(translated),
            json!([
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "Look at this"},
                        {"type": "image_url", "image_url": {"url": "https://example.test/image.png"}}
                    ]
                },
                {
                    "role": "assistant",
                    "content": "Done",
                    "reasoning_content": "brief reasoning",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "lookup", "arguments": "{\"term\":\"rust\"}"}
                    }]
                },
                {"role": "tool", "tool_call_id": "call_1", "content": "result"}
            ])
        );
    }
}
