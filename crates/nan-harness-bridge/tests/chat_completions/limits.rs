// Bounded bodies: the bridge caps the request it accepts and observes large
// responses without altering the bytes it forwards, and unparseable upstream
// data passes through untouched instead of being repaired.

use super::support::{oversized_response_body, start_servers, usage_for_model};
use axum::http::StatusCode;
use nan_harness_bridge::ModelUsageSnapshot;
use serde_json::json;

#[tokio::test]
async fn chat_bridge_passes_malformed_streams_and_rejects_oversized_requests() {
    let servers = start_servers().await;
    let client = reqwest::Client::new();
    let malformed = client
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({"model":"malformed","stream":true}))
        .send()
        .await
        .expect("malformed stream request should complete");
    assert_eq!(malformed.status(), StatusCode::OK);
    assert_eq!(
        malformed.text().await.expect("malformed stream body"),
        "data: {not-json}\n\n"
    );
    assert_eq!(
        servers.bridge.usage(),
        usage_for_model(
            "malformed",
            ModelUsageSnapshot {
                incomplete_responses: 1,
                ..ModelUsageSnapshot::default()
            }
        )
    );

    let oversized = vec![b'x'; 32 * 1024 * 1024 + 1];
    let response = client
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .body(oversized)
        .send()
        .await
        .expect("oversized request should complete");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        servers.bridge.usage(),
        usage_for_model(
            "malformed",
            ModelUsageSnapshot {
                incomplete_responses: 1,
                ..ModelUsageSnapshot::default()
            }
        )
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn chat_bridge_bounds_observation_without_changing_large_response_bodies() {
    let servers = start_servers().await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({"model":"oversized","stream":false}))
        .send()
        .await
        .expect("oversized response should complete");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.bytes().await.expect("oversized response body");
    let expected = oversized_response_body();
    assert_eq!(body.as_ref(), expected.as_slice());
    assert!(body.len() > 1024 * 1024);
    assert_eq!(
        servers.bridge.usage(),
        usage_for_model(
            "oversized",
            ModelUsageSnapshot {
                responses_without_usage: 1,
                ..ModelUsageSnapshot::default()
            }
        )
    );
    servers.shutdown().await;
}
