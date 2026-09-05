use crate::common::{responses_request, start_servers};
use axum::http::StatusCode;
use std::sync::atomic::Ordering;

#[tokio::test]
async fn responses_bridge_exposes_upstream_failures_as_diagnostics() {
    let mut servers = start_servers().await;
    // Keep the upstream failing so the bridge exhausts its retries and
    // surfaces the gateway error to the harness call site.
    servers
        .state
        .transient_faults
        .store(u8::MAX, Ordering::Relaxed);
    let mut diagnostics_rx = servers.bridge.take_diagnostics();

    let response = reqwest::Client::new()
        .post(format!("{}/v1/responses", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&responses_request())
        .send()
        .await
        .expect("request should complete with a gateway error");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .text()
        .await
        .expect("failure SSE should be readable");
    assert!(body.contains("event: response.failed"), "{body}");
    assert!(body.contains("NH-BRIDGE-104"), "{body}");

    let diagnostic = diagnostics_rx
        .recv()
        .await
        .expect("bridge should publish a diagnostic");
    assert_eq!(diagnostic.code, "NH-BRIDGE-104");
    assert_eq!(diagnostic.http_status, Some(503));
    assert_eq!(
        diagnostic.endpoint,
        nan_harness_bridge::BridgeEndpoint::Responses
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn responses_bridge_queues_multiple_diagnostics_without_overwriting_them() {
    let mut servers = start_servers().await;
    let mut diagnostics_rx = servers.bridge.take_diagnostics();
    let endpoint = format!("{}/v1/responses", servers.bridge.base_url());
    let client = reqwest::Client::new();

    let unauthorized = client
        .post(&endpoint)
        .json(&responses_request())
        .send()
        .await
        .expect("unauthorized request should complete");
    let invalid = client
        .post(&endpoint)
        .bearer_auth("local-session-token")
        .body("{")
        .send()
        .await
        .expect("invalid request should complete");

    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    let first = diagnostics_rx
        .recv()
        .await
        .expect("first diagnostic should be queued");
    let second = diagnostics_rx
        .recv()
        .await
        .expect("second diagnostic should be queued");
    assert_eq!(
        [first.code, second.code],
        ["NH-BRIDGE-101", "NH-BRIDGE-102"]
    );
    assert_eq!([first.http_status, second.http_status], [None, None]);
    servers.shutdown().await;
}
