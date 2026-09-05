use axum::http::StatusCode;
use nan_harness_bridge::ModelUsageSnapshot;
use serde_json::{Value, json};

use crate::support::{post_messages, start_servers, usage_for};

#[tokio::test]
async fn bridge_authenticates_locally_and_translates_non_streaming_messages() {
    let servers = start_servers().await;
    let endpoint = format!("{}/v1/messages?beta=true", servers.bridge.base_url());
    let request = json!({
        "model": "anthropic/nan/qwen3.6",
        "max_tokens": 100_000,
        "messages": [{"role": "user", "content": "hello"}]
    });

    let unauthorized = reqwest::Client::new()
        .post(&endpoint)
        .json(&request)
        .send()
        .await
        .expect("local request should complete");
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    let error: Value = unauthorized.json().await.expect("error should be JSON");
    assert_eq!(error["error"]["type"], "authentication_error");

    let response = post_messages(&servers, "/v1/messages?beta=true", &request).await;
    assert_eq!(response.status(), StatusCode::OK);
    let response: Value = response.json().await.expect("response should be JSON");
    assert_eq!(response["type"], "message");
    assert_eq!(response["content"][0]["text"], "hello from NaN");

    {
        let requests = servers.state.requests.lock().expect("request lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["model"], "qwen3.6");
        assert_eq!(requests[0]["max_tokens"], 65_536);
    }
    assert_eq!(
        servers.bridge.usage(),
        usage_for([(
            "qwen3.6",
            ModelUsageSnapshot {
                responses_with_usage: 1,
                input_tokens: 5,
                output_tokens: 4,
                reasoning_tokens: 2,
                ..ModelUsageSnapshot::default()
            },
        )])
    );
    servers.shutdown().await;
}
