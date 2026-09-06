use crate::common::{responses_request, start_servers};
use axum::http::header;
use nan_harness_bridge::{BridgeAttemptBucket, BridgeRecoveryOutcome};
use std::sync::atomic::Ordering;
use std::time::Duration;

#[tokio::test]
async fn responses_bridge_fails_after_five_precommit_truncated_streams() {
    let mut servers = start_servers().await;
    servers
        .state
        .truncated_completions
        .store(5, Ordering::Relaxed);
    let mut diagnostics = servers.bridge.take_diagnostics();

    let body = tokio::time::timeout(Duration::from_secs(30), async {
        reqwest::Client::new()
            .post(format!("{}/v1/responses", servers.bridge.base_url()))
            .bearer_auth("local-session-token")
            .json(&responses_request())
            .send()
            .await
            .expect("request should be accepted before upstream completes")
            .text()
            .await
            .expect("stream should be readable")
    })
    .await
    .expect("full stream should finish within 30 seconds");

    assert!(body.contains("response.failed"), "{body}");
    assert!(body.contains("NH-BRIDGE-105"), "{body}");
    assert!(!body.contains("response.completed"), "{body}");
    assert!(!body.contains("unfinished"), "{body}");
    assert_eq!(
        servers.state.chat_attempts.load(Ordering::Relaxed),
        5,
        "the recovery limit should allow five upstream attempts"
    );
    {
        let requests = servers.state.chat_requests.lock().expect("request lock");
        assert_eq!(requests.len(), 5);
        assert!(
            requests.windows(2).all(|pair| pair[0] == pair[1]),
            "every recovery must repeat the payload exactly"
        );
    }
    {
        let headers = servers.state.chat_headers.lock().expect("header lock");
        assert_eq!(headers.len(), 5);
        assert!(
            headers
                .iter()
                .all(|headers| !headers.contains_key(header::CACHE_CONTROL)
                    && !headers.contains_key("x-request-id")),
            "pre-commit truncation recovery must not enable cache bypass"
        );
    }
    let mut recovery = Vec::new();
    while let Ok(diagnostic) = diagnostics.try_recv() {
        recovery.push(diagnostic);
    }
    assert_eq!(
        recovery.len(),
        5,
        "recovery should emit four retries and one exhaustion"
    );
    assert!(
        recovery[..4]
            .iter()
            .all(|diagnostic| diagnostic.recovery_outcome == Some(BridgeRecoveryOutcome::Retrying)),
        "{recovery:?}"
    );
    assert_eq!(recovery[0].attempt, Some(BridgeAttemptBucket::First));
    assert_eq!(recovery[1].attempt, Some(BridgeAttemptBucket::Second));
    assert_eq!(
        recovery[4].recovery_outcome,
        Some(BridgeRecoveryOutcome::Exhausted)
    );
    assert_eq!(recovery[4].attempt, Some(BridgeAttemptBucket::Later));
    servers.shutdown().await;
}
