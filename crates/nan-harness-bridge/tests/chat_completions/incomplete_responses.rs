// Responses that never complete: a harness that stops reading, a response
// still unread when the bridge shuts down, and an upstream body that fails
// mid-stream. Each one is recorded as incomplete and never commits usage.

use super::support::{start_servers, usage_for_model};
use axum::http::StatusCode;
use futures_util::StreamExt;
use nan_harness_bridge::ModelUsageSnapshot;
use serde_json::json;

#[tokio::test]
async fn chat_bridge_marks_a_response_incomplete_when_the_consumer_disconnects() {
    let servers = start_servers().await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({"model":"consumer-disconnect","stream":true}))
        .send()
        .await
        .expect("stream request should complete headers");
    let mut body = response.bytes_stream();
    body.next()
        .await
        .expect("stream should contain a first chunk")
        .expect("first chunk should be readable");
    drop(body);

    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            if servers.bridge.usage().incomplete_responses() == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the disconnected response should be recorded");
    assert_eq!(
        servers.bridge.usage(),
        usage_for_model(
            "consumer-disconnect",
            ModelUsageSnapshot {
                incomplete_responses: 1,
                ..ModelUsageSnapshot::default()
            }
        )
    );
    servers.state.release_stream.notify_one();
    servers.shutdown().await;
}

#[tokio::test]
async fn bridge_waits_for_an_unread_response_to_record_incomplete_usage() {
    let mut servers = start_servers().await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({"model":"consumer-disconnect","stream":true}))
        .send()
        .await
        .expect("stream request should complete headers");
    assert_eq!(response.status(), StatusCode::OK);
    drop(response);

    servers.state.release_stream.notify_one();
    servers.bridge.shutdown();
    servers
        .bridge
        .wait()
        .await
        .expect("bridge should wait for the unread response to be dropped");
    assert_eq!(
        servers.bridge.usage(),
        usage_for_model(
            "consumer-disconnect",
            ModelUsageSnapshot {
                incomplete_responses: 1,
                ..ModelUsageSnapshot::default()
            }
        )
    );
    servers.upstream_task.abort();
}

#[tokio::test]
async fn chat_bridge_does_not_commit_usage_after_a_body_error() {
    let servers = start_servers().await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({"model":"body-error","stream":true}))
        .send()
        .await
        .expect("body error stream should complete headers");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.bytes().await.is_err());
    assert_eq!(
        servers.bridge.usage(),
        usage_for_model(
            "body-error",
            ModelUsageSnapshot {
                incomplete_responses: 1,
                ..ModelUsageSnapshot::default()
            }
        )
    );
    servers.shutdown().await;
}
