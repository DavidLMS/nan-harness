use axum::http::StatusCode;
use serde_json::json;

use crate::support::{post_messages, start_servers};

#[tokio::test]
async fn bridge_streams_text_and_tool_deltas_in_anthropic_order() {
    let servers = start_servers().await;
    let response = post_messages(
        &servers,
        "/v1/messages",
        &json!({
            "model": "anthropic/nan/qwen3.6",
            "max_tokens": 1024,
            "stream": true,
            "thinking": {"type": "enabled", "budget_tokens": 1024},
            "tools": [{
                "name": "Read",
                "description": "Read a file",
                "input_schema": {"type": "object", "properties": {"file_path": {"type": "string"}}}
            }],
            "messages": [{"role": "user", "content": "Read README.md"}]
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let stream = response.text().await.expect("stream should be readable");

    let message_start = stream.find("message_start").expect("message start event");
    let thinking_delta = stream.find("thinking_delta").expect("thinking delta event");
    let text_delta = stream.find("text_delta").expect("text delta event");
    let tool_start = stream.find("tool_use").expect("tool start event");
    let tool_delta = stream.find("input_json_delta").expect("tool delta event");
    let message_stop = stream.rfind("message_stop").expect("message stop event");
    assert!(message_start < thinking_delta);
    assert!(thinking_delta < text_delta);
    assert!(text_delta < tool_start);
    assert!(tool_start < tool_delta);
    assert!(tool_delta < message_stop);
    assert!(stream.contains("README.md"));
    servers.shutdown().await;
}
