use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::support::{post_messages, start_servers};

#[tokio::test]
async fn bridge_preserves_images_inside_tool_results() {
    let servers = start_servers().await;
    let response = post_messages(
        &servers,
        "/v1/messages",
        &json!({
            "model": "anthropic/nan/qwen3.6",
            "max_tokens": 1_024,
            "tools": [{
                "name": "screenshot",
                "description": "Capture the current screen",
                "input_schema": {"type": "object", "properties": {}}
            }],
            "messages": [
                {"role": "user", "content": "Inspect the screen"},
                {"role": "assistant", "content": [{
                    "type": "tool_use",
                    "id": "tool_screenshot_1",
                    "name": "screenshot",
                    "input": {}
                }]},
                {"role": "user", "content": [{
                    "type": "tool_result",
                    "tool_use_id": "tool_screenshot_1",
                    "content": [
                        {"type": "text", "text": "Screenshot captured"},
                        {"type": "image", "source": {
                            "type": "base64",
                            "media_type": "image/png",
                            "data": "AA=="
                        }}
                    ]
                }]}
            ]
        }),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    {
        let requests = servers.state.requests.lock().expect("request lock");
        let tool_result = &requests[0]["messages"][2];
        assert_eq!(tool_result["role"], "tool");
        assert_eq!(tool_result["tool_call_id"], "tool_screenshot_1");
        assert_eq!(tool_result["content"][0]["type"], "text");
        assert_eq!(tool_result["content"][0]["text"], "Screenshot captured");
        assert_eq!(tool_result["content"][1]["type"], "image_url");
        assert_eq!(
            tool_result["content"][1]["image_url"]["url"],
            "data:image/png;base64,AA=="
        );
    }
    servers.shutdown().await;
}

#[tokio::test]
async fn bridge_keeps_regular_compatibility_alias_requests_untuned() {
    let servers = start_servers().await;
    let response = post_messages(
        &servers,
        "/v1/messages",
        &json!({
            "model": "opus",
            "max_tokens": 1_024,
            "temperature": 0.7,
            "messages": [{"role": "user", "content": "hello"}]
        }),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let response: Value = response.json().await.expect("response should be JSON");
    assert_eq!(response["model"], "anthropic/nan/qwen3.6");
    {
        let requests = servers.state.requests.lock().expect("request lock");
        assert_eq!(requests[0]["max_tokens"], 1_024);
        assert_eq!(requests[0]["temperature"], 0.7);
        assert!(requests[0].get("chat_template_kwargs").is_none());
    }
    servers.shutdown().await;
}
