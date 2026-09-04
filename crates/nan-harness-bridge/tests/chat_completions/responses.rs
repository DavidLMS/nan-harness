// Response delivery: streaming chunks reach the harness before the upstream
// finishes and keep their order and request forwarding intact, while a
// non-streaming response keeps the provider fields the harness reads back.

use super::support::{start_servers, usage_for};
use axum::http::{StatusCode, header};
use futures_util::StreamExt;
use nan_harness_bridge::{ModelUsageSnapshot, ProviderUsageSnapshot};
use serde_json::{Value, json};

#[tokio::test]
async fn chat_bridge_forwards_stream_chunks_before_upstream_completion_and_observes_usage() {
    let servers = start_servers().await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({
            "model":"qwen3.6",
            "messages":[{"role":"user","content":"hello"}],
            "stream":true,
            "tools":[{"type":"function","function":{"name":"run","parameters":{"type":"object"}}}],
            "reasoning_effort":"high"
        }))
        .send()
        .await
        .expect("stream request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/event-stream"
    );

    let mut body = response.bytes_stream();
    let first = tokio::time::timeout(std::time::Duration::from_secs(1), body.next())
        .await
        .expect("first chunk should arrive before upstream completes")
        .expect("stream should contain a first chunk")
        .expect("first chunk should be readable");
    assert!(
        first.starts_with(b"data: {\"id\":\"first\",\"choices\""),
        "{first:?}"
    );
    assert_eq!(servers.bridge.usage(), ProviderUsageSnapshot::default());

    servers.state.release_stream.notify_one();
    let mut rest = Vec::new();
    while let Some(chunk) = body.next().await {
        rest.extend_from_slice(&chunk.expect("stream chunk should be readable"));
    }
    assert!(
        rest.windows(b"usage".len())
            .any(|window| window == b"usage")
    );
    assert!(rest.ends_with(b"data: [DONE]\n\n"));
    assert_eq!(
        servers.state.requests.lock().expect("request lock").len(),
        1
    );
    let request = servers.state.requests.lock().expect("request lock")[0]
        .1
        .clone();
    assert_eq!(request["stream_options"]["include_usage"], true);
    assert_eq!(request["tools"][0]["function"]["name"], "run");
    assert_eq!(request["reasoning_effort"], "high");

    assert_eq!(
        servers.bridge.usage(),
        usage_for(ModelUsageSnapshot {
            responses_with_usage: 1,
            responses_without_usage: 0,
            incomplete_responses: 0,
            input_tokens: 17,
            output_tokens: 9,
            reasoning_tokens: 4,
        })
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn chat_bridge_preserves_non_streaming_fields_and_usage() {
    let servers = start_servers().await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({"model":"qwen3.6","messages":[],"stream":false}))
        .send()
        .await
        .expect("non-stream request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.json::<Value>().await.expect("response JSON")["choices"][0]["message"]["content"],
        "hello"
    );
    assert_eq!(
        servers.bridge.usage(),
        usage_for(ModelUsageSnapshot {
            responses_with_usage: 1,
            responses_without_usage: 0,
            incomplete_responses: 0,
            input_tokens: 3,
            output_tokens: 2,
            reasoning_tokens: 0,
        })
    );
    servers.shutdown().await;
}
