use crate::common::{send_budget_request, start_servers};
use nan_harness_bridge::BridgeRecoveryOutcome;
use std::sync::atomic::Ordering;

#[tokio::test]
async fn responses_bridge_shared_send_budget_exhausts_on_semantic_failure() {
    let mut servers = start_servers().await;
    *servers
        .state
        .transient_fault_sends
        .lock()
        .expect("schedule") = vec![1, 3, 5, 7];
    servers.state.empty_completions.store(4, Ordering::Relaxed);
    let mut diagnostics = servers.bridge.take_diagnostics();

    let body = send_budget_request(&servers).await;

    assert_eq!(servers.state.chat_attempts.load(Ordering::Relaxed), 8);
    assert!(body.contains("response.failed"), "{body}");
    assert!(body.contains("NH-BRIDGE-105"), "{body}");
    assert!(!body.contains("response.completed"), "{body}");
    assert!(!body.contains("unfinished"), "{body}");
    for _ in 0..3 {
        assert_eq!(
            diagnostics
                .recv()
                .await
                .expect("diagnostic")
                .recovery_outcome,
            Some(BridgeRecoveryOutcome::Retrying)
        );
    }
    assert_eq!(
        diagnostics
            .recv()
            .await
            .expect("diagnostic")
            .recovery_outcome,
        Some(BridgeRecoveryOutcome::Exhausted)
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn responses_bridge_shared_send_budget_preserves_last_http_error() {
    let servers = start_servers().await;
    *servers
        .state
        .transient_fault_sends
        .lock()
        .expect("schedule") = vec![1, 3, 5, 7, 8];
    servers.state.empty_completions.store(3, Ordering::Relaxed);

    let body = send_budget_request(&servers).await;

    assert_eq!(servers.state.chat_attempts.load(Ordering::Relaxed), 8);
    assert!(body.contains("response.failed"), "{body}");
    assert!(body.contains("NH-BRIDGE-104"), "{body}");
    assert!(!body.contains("response.completed"), "{body}");
    servers.shutdown().await;
}

#[tokio::test]
async fn responses_bridge_shared_send_budget_allows_success_on_last_send() {
    let servers = start_servers().await;
    *servers
        .state
        .transient_fault_sends
        .lock()
        .expect("schedule") = vec![1, 3, 5, 7];
    servers.state.empty_completions.store(3, Ordering::Relaxed);

    let body = send_budget_request(&servers).await;

    assert_eq!(servers.state.chat_attempts.load(Ordering::Relaxed), 8);
    assert!(body.contains("response.completed"), "{body}");
    assert!(!body.contains("response.failed"), "{body}");
    assert!(body.contains("Working"), "{body}");
    servers.shutdown().await;
}

#[tokio::test]
async fn responses_bridge_shared_send_budget_delegates_last_incomplete_patch() {
    let mut servers = start_servers().await;
    *servers
        .state
        .transient_fault_sends
        .lock()
        .expect("schedule") = vec![1, 3, 5, 7];
    servers
        .state
        .malformed_patch_completions
        .store(4, Ordering::Relaxed);
    let mut diagnostics = servers.bridge.take_diagnostics();

    let body = send_budget_request(&servers).await;

    assert_eq!(servers.state.chat_attempts.load(Ordering::Relaxed), 8);
    assert!(body.contains("response.completed"), "{body}");
    assert!(!body.contains("response.failed"), "{body}");
    assert!(body.contains("custom_tool_call"), "{body}");
    assert!(body.contains("apply_patch"), "{body}");
    assert!(body.contains(r#""input":"{""#), "{body}");
    for _ in 0..3 {
        assert_eq!(
            diagnostics
                .recv()
                .await
                .expect("diagnostic")
                .recovery_outcome,
            Some(BridgeRecoveryOutcome::Retrying)
        );
    }
    assert_eq!(
        diagnostics
            .recv()
            .await
            .expect("diagnostic")
            .recovery_outcome,
        Some(BridgeRecoveryOutcome::Delegated)
    );
    servers.shutdown().await;
}
