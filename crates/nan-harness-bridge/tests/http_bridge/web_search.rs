use axum::http::StatusCode;
use serde_json::json;

use crate::support::{post_messages, start_servers};

#[tokio::test]
async fn bridge_executes_claude_code_web_search_through_nan() {
    let servers = start_servers().await;
    let response = post_messages(
        &servers,
        "/v1/messages",
        &json!({
            "model": "anthropic/nan/qwen3.6",
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
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let stream = response.text().await.expect("stream should be readable");

    assert!(stream.contains("server_tool_use"), "{stream}");
    assert!(stream.contains("web_search_tool_result"), "{stream}");
    assert!(stream.contains("Tokio project"), "{stream}");
    assert!(stream.contains("https://tokio.rs"), "{stream}");
    assert!(stream.contains("Async runtime for Rust"), "{stream}");
    assert!(stream.contains("message_stop"), "{stream}");

    {
        let requests = servers.state.search_requests.lock().expect("request lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["query"], "best Rust async runtime 2025");
        assert_eq!(requests[0]["count"], 8);
    }
    servers.shutdown().await;
}
