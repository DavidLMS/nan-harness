use super::translate;
use crate::anthropic::request::wire::{
    ContentBlock, ImageSource, Message, MessageContent, Role, ToolResultBlock, ToolResultContent,
};
use serde_json::json;

#[test]
fn failed_string_tool_result_gets_exactly_one_error_prefix() {
    let translated = translate(
        None,
        vec![
            Message {
                role: Role::User,
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "call_failed".to_owned(),
                    content: ToolResultContent::Text("synthetic result".to_owned()),
                    is_error: true,
                }]),
            },
            Message {
                role: Role::User,
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "call_succeeded".to_owned(),
                    content: ToolResultContent::Text("synthetic result".to_owned()),
                    is_error: false,
                }]),
            },
            Message {
                role: Role::User,
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "call_empty".to_owned(),
                    content: ToolResultContent::Text(String::new()),
                    is_error: true,
                }]),
            },
        ],
    )
    .expect("tool results should translate");

    assert_eq!(
        serde_json::Value::Array(translated),
        json!([
            {
                "role": "tool",
                "tool_call_id": "call_failed",
                "content": "Tool error: synthetic result"
            },
            {
                "role": "tool",
                "tool_call_id": "call_succeeded",
                "content": "synthetic result"
            },
            {
                "role": "tool",
                "tool_call_id": "call_empty",
                "content": "Tool error: "
            }
        ])
    );
}

#[test]
fn failed_text_blocks_stay_a_single_prefixed_string() {
    let translated = translate(
        None,
        vec![Message {
            role: Role::User,
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "call_text_blocks".to_owned(),
                content: ToolResultContent::Blocks(vec![
                    ToolResultBlock::Text {
                        text: "first line".to_owned(),
                    },
                    ToolResultBlock::Text {
                        text: "second line".to_owned(),
                    },
                ]),
                is_error: true,
            }]),
        }],
    )
    .expect("text-only tool-result blocks should translate");

    assert_eq!(
        serde_json::Value::Array(translated),
        json!([{
            "role": "tool",
            "tool_call_id": "call_text_blocks",
            "content": "Tool error: first line\nsecond line"
        }])
    );
}

#[test]
fn failed_mixed_tool_result_preserves_original_content_and_order() {
    let translated = translate(
        None,
        vec![
            Message {
                role: Role::User,
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "call_failed_mixed".to_owned(),
                    content: ToolResultContent::Blocks(vec![
                        ToolResultBlock::Text {
                            text: "text before image".to_owned(),
                        },
                        ToolResultBlock::Image {
                            source: ImageSource::Url {
                                url: "https://example.test/image.png".to_owned(),
                            },
                        },
                        ToolResultBlock::Text {
                            text: "text between images".to_owned(),
                        },
                        ToolResultBlock::Image {
                            source: ImageSource::Base64 {
                                media_type: "image/png".to_owned(),
                                data: "c3ludGhldGlj".to_owned(),
                            },
                        },
                        ToolResultBlock::Text {
                            text: "text after image".to_owned(),
                        },
                    ]),
                    is_error: true,
                }]),
            },
            Message {
                role: Role::User,
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "call_succeeded_mixed".to_owned(),
                    content: ToolResultContent::Blocks(vec![
                        ToolResultBlock::Text {
                            text: "unchanged text".to_owned(),
                        },
                        ToolResultBlock::Image {
                            source: ImageSource::Url {
                                url: "https://example.test/unchanged.png".to_owned(),
                            },
                        },
                    ]),
                    is_error: false,
                }]),
            },
        ],
    )
    .expect("mixed tool-result content should translate");

    assert_eq!(
        serde_json::Value::Array(translated),
        json!([
            {
                "role": "tool",
                "tool_call_id": "call_failed_mixed",
                "content": [
                    {"type": "text", "text": "Tool error"},
                    {"type": "text", "text": "text before image"},
                    {
                        "type": "image_url",
                        "image_url": {"url": "https://example.test/image.png"}
                    },
                    {"type": "text", "text": "text between images"},
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": "data:image/png;base64,c3ludGhldGlj"
                        }
                    },
                    {"type": "text", "text": "text after image"}
                ]
            },
            {
                "role": "tool",
                "tool_call_id": "call_succeeded_mixed",
                "content": [
                    {"type": "text", "text": "unchanged text"},
                    {
                        "type": "image_url",
                        "image_url": {"url": "https://example.test/unchanged.png"}
                    }
                ]
            }
        ])
    );
}

#[test]
fn failed_tool_result_flushes_adjacent_user_text_in_order() {
    let translated = translate(
        None,
        vec![Message {
            role: Role::User,
            content: MessageContent::Blocks(vec![
                ContentBlock::Text {
                    text: "before the failed result".to_owned(),
                },
                ContentBlock::ToolResult {
                    tool_use_id: "call_failed".to_owned(),
                    content: ToolResultContent::Text("synthetic result".to_owned()),
                    is_error: true,
                },
                ContentBlock::Text {
                    text: "after the failed result".to_owned(),
                },
            ]),
        }],
    )
    .expect("user blocks around a tool result should translate");

    assert_eq!(
        serde_json::Value::Array(translated),
        json!([
            {
                "role": "user",
                "content": "before the failed result"
            },
            {
                "role": "tool",
                "tool_call_id": "call_failed",
                "content": "Tool error: synthetic result"
            },
            {
                "role": "user",
                "content": "after the failed result"
            }
        ])
    );
}
