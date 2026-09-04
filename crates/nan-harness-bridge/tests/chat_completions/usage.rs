// Usage accounting: which response shapes the bridge commits, when it may
// commit them, and how it attributes them per requested model. A completed
// stream only commits usage once `[DONE]` arrives; a stream that ends without
// usage is still counted as completed, just without tokens.

use super::support::{start_servers, usage_for_model, usage_for_models};
use axum::http::StatusCode;
use nan_harness_bridge::ModelUsageSnapshot;
use serde_json::json;

#[tokio::test]
async fn chat_bridge_attributes_usage_to_each_requested_model() {
    let servers = start_servers().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({"model":"qwen3.6","messages":[],"stream":false}))
        .send()
        .await
        .expect("non-stream request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    response
        .bytes()
        .await
        .expect("non-stream response should be readable");

    let response = client
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({"model":"qwen3.8-flash","messages":[],"stream":true}))
        .send()
        .await
        .expect("stream request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    servers.state.release_stream.notify_one();
    response
        .bytes()
        .await
        .expect("stream response should be readable");

    assert_eq!(
        servers.bridge.usage(),
        usage_for_models([
            (
                "qwen3.6",
                ModelUsageSnapshot {
                    responses_with_usage: 1,
                    input_tokens: 3,
                    output_tokens: 2,
                    ..ModelUsageSnapshot::default()
                },
            ),
            (
                "qwen3.8-flash",
                ModelUsageSnapshot {
                    responses_with_usage: 1,
                    input_tokens: 17,
                    output_tokens: 9,
                    reasoning_tokens: 4,
                    ..ModelUsageSnapshot::default()
                },
            ),
        ])
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn chat_bridge_requires_done_before_committing_stream_usage() {
    let servers = start_servers().await;
    let client = reqwest::Client::new();

    let truncated = client
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({"model":"usage-before-truncated","stream":true}))
        .send()
        .await
        .expect("truncated usage stream should complete headers");
    assert_eq!(truncated.status(), StatusCode::OK);
    assert!(
        truncated
            .text()
            .await
            .expect("truncated usage body")
            .contains("usage")
    );
    assert_eq!(
        servers.bridge.usage(),
        usage_for_model(
            "usage-before-truncated",
            ModelUsageSnapshot {
                incomplete_responses: 1,
                ..ModelUsageSnapshot::default()
            }
        )
    );

    let split = client
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({"model":"split-usage","stream":true}))
        .send()
        .await
        .expect("split usage stream should complete headers");
    assert_eq!(split.status(), StatusCode::OK);
    assert!(
        split
            .text()
            .await
            .expect("split usage body")
            .ends_with("[DONE]\n\n")
    );
    assert_eq!(
        servers.bridge.usage(),
        usage_for_models([
            (
                "split-usage",
                ModelUsageSnapshot {
                    responses_with_usage: 1,
                    input_tokens: 5,
                    output_tokens: 7,
                    reasoning_tokens: 2,
                    ..ModelUsageSnapshot::default()
                },
            ),
            (
                "usage-before-truncated",
                ModelUsageSnapshot {
                    incomplete_responses: 1,
                    ..ModelUsageSnapshot::default()
                },
            ),
        ])
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn chat_bridge_counts_a_done_stream_without_usage_as_completed() {
    let servers = start_servers().await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({"model":"done-without-usage","stream":true}))
        .send()
        .await
        .expect("done stream should complete headers");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .text()
            .await
            .expect("done stream body")
            .ends_with("[DONE]\n\n")
    );
    assert_eq!(
        servers.bridge.usage(),
        usage_for_model(
            "done-without-usage",
            ModelUsageSnapshot {
                responses_without_usage: 1,
                ..ModelUsageSnapshot::default()
            }
        )
    );
    servers.shutdown().await;
}
