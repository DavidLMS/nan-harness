use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::support::{SESSION_TOKEN, start_servers};

#[tokio::test]
async fn count_tokens_needs_no_generation_limit_or_upstream_request() {
    let servers = start_servers().await;
    let client = reqwest::Client::new();
    let hello = client
        .head(format!("{}/api/hello", servers.bridge.base_url()))
        .send()
        .await
        .expect("hello request should complete");
    assert_eq!(hello.status(), StatusCode::NO_CONTENT);

    let response = client
        .post(format!(
            "{}/v1/messages/count_tokens",
            servers.bridge.base_url()
        ))
        .bearer_auth(SESSION_TOKEN)
        .json(&json!({
            "model": "anthropic/nan/qwen3.6",
            "messages": [{"role": "user", "content": "count this prompt"}]
        }))
        .send()
        .await
        .expect("count request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    let response: Value = response.json().await.expect("count should be JSON");
    assert!(
        response["input_tokens"]
            .as_u64()
            .is_some_and(|count| count > 0)
    );
    assert!(
        servers
            .state
            .requests
            .lock()
            .expect("request lock")
            .is_empty()
    );
    servers.shutdown().await;
}
