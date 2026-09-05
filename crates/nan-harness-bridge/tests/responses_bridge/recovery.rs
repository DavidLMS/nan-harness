use crate::common::{responses_request, send_budget_request, start_servers};
use axum::http::{StatusCode, header};
use nan_harness_bridge::{BridgeAttemptBucket, BridgeRecoveryOutcome};
use std::sync::atomic::Ordering;

#[tokio::test]
async fn responses_bridge_retries_transient_upstream_gateway_errors() {
    let servers = start_servers().await;
    servers.state.transient_faults.store(2, Ordering::Relaxed);

    let response = reqwest::Client::new()
        .post(format!("{}/v1/responses", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&responses_request())
        .send()
        .await
        .expect("request should succeed after the bridge retries transient 503s");
    assert_eq!(response.status(), StatusCode::OK);
    let _body = response.text().await.expect("stream should be readable");

    assert_eq!(
        servers.state.chat_attempts.load(Ordering::Relaxed),
        3,
        "the two injected 503s plus one success should be attempted"
    );
    assert_eq!(
        servers.state.transient_faults.load(Ordering::Relaxed),
        0,
        "all injected faults should be consumed"
    );
    assert_eq!(
        servers
            .state
            .chat_requests
            .lock()
            .expect("chat request lock")
            .len(),
        1,
        "only a successful (non-fault) upstream request should be recorded"
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn responses_bridge_changes_the_body_after_a_reasoning_only_completion() {
    let servers = start_servers().await;
    servers
        .state
        .body_keyed_empty_replay
        .store(true, Ordering::Relaxed);

    let response = reqwest::Client::new()
        .post(format!("{}/v1/responses", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&responses_request())
        .send()
        .await
        .expect("request should be accepted before upstream completes");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.expect("stream should be readable");

    assert!(body.contains("response.completed"), "{body}");
    assert!(!body.contains("unfinished"), "{body}");
    {
        let requests = servers.state.chat_requests.lock().expect("request lock");
        assert_eq!(requests.len(), 2);
        assert_ne!(requests[0], requests[1]);
        let first_messages = requests[0]["messages"].as_array().expect("messages");
        let recovery_messages = requests[1]["messages"].as_array().expect("messages");
        assert_eq!(first_messages.len() + 1, recovery_messages.len());
        assert_eq!(
            recovery_messages.first().expect("recovery message")["role"],
            "system"
        );
        assert!(
            recovery_messages.first().expect("recovery message")["content"]
                .as_str()
                .expect("recovery content")
                .contains("nan-harness internal recovery")
        );
        assert_eq!(&recovery_messages[1..], first_messages);
        assert_eq!(
            requests[0]
                .as_object()
                .expect("original body")
                .keys()
                .collect::<Vec<_>>(),
            requests[1]
                .as_object()
                .expect("recovery body")
                .keys()
                .collect::<Vec<_>>()
        );
    }
    {
        let headers = servers.state.chat_headers.lock().expect("header lock");
        assert_eq!(headers.len(), 2);
        assert!(!headers[0].contains_key(header::CACHE_CONTROL));
        assert_eq!(headers[1][header::CACHE_CONTROL], "no-cache");
        assert!(headers[1].contains_key("x-request-id"));
    }
    servers.shutdown().await;
}

#[tokio::test]
async fn responses_bridge_recovers_after_four_fresh_reasoning_only_completions() {
    let servers = start_servers().await;
    servers.state.empty_completions.store(4, Ordering::Relaxed);
    servers.state.empty_id_mode.store(1, Ordering::Relaxed);

    let response = reqwest::Client::new()
        .post(format!("{}/v1/responses", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&responses_request())
        .send()
        .await
        .expect("request should be accepted");
    let body = response.text().await.expect("stream should be readable");
    assert!(body.contains("response.completed"), "{body}");
    {
        let requests = servers.state.chat_requests.lock().expect("request lock");
        assert_eq!(requests.len(), 5);
        assert_ne!(requests[0], requests[1]);
        assert_ne!(requests[1], requests[2]);
        assert_ne!(requests[2], requests[3]);
        assert_ne!(requests[3], requests[4]);
    }
    {
        let headers = servers.state.chat_headers.lock().expect("header lock");
        assert!(
            headers
                .iter()
                .enumerate()
                .all(|(index, headers)| index == 0 || headers.contains_key(header::CACHE_CONTROL))
        );
    }
    servers.shutdown().await;
}

#[tokio::test]
async fn responses_bridge_bypasses_cache_even_when_empty_ids_are_missing() {
    let servers = start_servers().await;
    servers.state.empty_completions.store(2, Ordering::Relaxed);
    servers.state.empty_id_mode.store(2, Ordering::Relaxed);

    let response = reqwest::Client::new()
        .post(format!("{}/v1/responses", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&responses_request())
        .send()
        .await
        .expect("request should be accepted");
    let body = response.text().await.expect("stream should be readable");
    assert!(body.contains("response.completed"), "{body}");
    {
        let requests = servers.state.chat_requests.lock().expect("request lock");
        assert_ne!(requests[0], requests[1]);
        assert_ne!(requests[1], requests[2]);
    }
    {
        let headers = servers.state.chat_headers.lock().expect("header lock");
        assert!(
            headers
                .iter()
                .enumerate()
                .all(|(index, headers)| index == 0 || headers.contains_key(header::CACHE_CONTROL))
        );
    }
    servers.shutdown().await;
}

#[tokio::test]
async fn responses_bridge_recovers_after_two_precommit_truncated_streams() {
    let servers = start_servers().await;
    servers
        .state
        .truncated_completions
        .store(2, Ordering::Relaxed);

    let response = reqwest::Client::new()
        .post(format!("{}/v1/responses", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&responses_request())
        .send()
        .await
        .expect("request should be accepted before upstream completes");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.expect("stream should be readable");

    assert!(body.contains("response.completed"), "{body}");
    assert!(!body.contains("unfinished"), "{body}");
    {
        let requests = servers.state.chat_requests.lock().expect("request lock");
        assert_eq!(requests.len(), 3);
        assert!(
            requests.windows(2).all(|pair| pair[0] == pair[1]),
            "every recovery must repeat the payload exactly"
        );
    }
    servers.shutdown().await;
}

#[tokio::test]
async fn responses_bridge_recovers_incomplete_custom_tool_calls_with_a_nudge() {
    let servers = start_servers().await;
    servers
        .state
        .malformed_patch_completions
        .store(2, Ordering::Relaxed);

    let response = reqwest::Client::new()
        .post(format!("{}/v1/responses", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&responses_request())
        .send()
        .await
        .expect("request should be accepted");
    let body = response.text().await.expect("stream should be readable");

    assert!(body.contains("response.completed"), "{body}");
    assert!(!body.contains("response.failed"), "{body}");
    assert!(!body.contains("must not leak"), "{body}");
    assert!(body.contains("Working"), "{body}");
    {
        let requests = servers.state.chat_requests.lock().expect("request lock");
        assert_eq!(requests.len(), 3);
        assert_ne!(requests[0], requests[1]);
        assert_ne!(requests[1], requests[2]);
        assert_eq!(
            requests[1]["messages"]
                .as_array()
                .expect("messages")
                .first()
                .expect("nudge")["role"],
            "system"
        );
    }
    servers.shutdown().await;
}

#[tokio::test]
async fn responses_bridge_delegates_an_incomplete_patch_after_recovery_is_exhausted() {
    let mut servers = start_servers().await;
    servers
        .state
        .malformed_patch_completions
        .store(8, Ordering::Relaxed);
    let mut diagnostics = servers.bridge.take_diagnostics();

    let response = reqwest::Client::new()
        .post(format!("{}/v1/responses", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&responses_request())
        .send()
        .await
        .expect("request should be accepted");
    let body = response.text().await.expect("stream should be readable");

    assert!(body.contains("response.completed"), "{body}");
    assert!(!body.contains("response.failed"), "{body}");
    assert!(body.contains("custom_tool_call"), "{body}");
    assert!(body.contains("apply_patch"), "{body}");
    assert!(body.contains(r#""input":"{""#), "{body}");
    assert_eq!(servers.state.chat_attempts.load(Ordering::Relaxed), 8);
    let mut recovery = Vec::new();
    for _ in 0..8 {
        recovery.push(diagnostics.recv().await.expect("recovery diagnostic"));
    }
    assert!(recovery[..7].iter().all(|diagnostic| {
        diagnostic.recovery_outcome == Some(BridgeRecoveryOutcome::Retrying)
    }));
    assert_eq!(
        recovery[7].recovery_outcome,
        Some(BridgeRecoveryOutcome::Delegated)
    );
    assert_eq!(recovery[7].attempt, Some(BridgeAttemptBucket::Later));
    servers.shutdown().await;
}

#[tokio::test]
async fn responses_bridge_recovers_truncated_text_without_delivering_the_discarded_answer() {
    let servers = start_servers().await;
    servers
        .state
        .truncated_text_completions
        .store(1, Ordering::Relaxed);

    let body = send_budget_request(&servers).await;

    assert_eq!(servers.state.chat_attempts.load(Ordering::Relaxed), 2);
    assert!(body.contains("response.completed"), "{body}");
    assert!(!body.contains("response.failed"), "{body}");
    assert!(body.contains("Working"), "{body}");
    assert!(!body.contains("Discard this partial answer"), "{body}");
    {
        let requests = servers.state.chat_requests.lock().expect("requests");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], requests[1]);
    }
    servers.shutdown().await;
}

#[tokio::test]
async fn responses_bridge_fails_after_eight_reasoning_only_completions() {
    let mut servers = start_servers().await;
    servers.state.empty_completions.store(8, Ordering::Relaxed);
    let mut diagnostics = servers.bridge.take_diagnostics();

    let response = reqwest::Client::new()
        .post(format!("{}/v1/responses", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&responses_request())
        .send()
        .await
        .expect("request should be accepted");
    let body = response.text().await.expect("stream should be readable");
    assert!(body.contains("response.failed"), "{body}");
    assert!(body.contains("NH-BRIDGE-105"), "{body}");
    assert!(!body.contains("unfinished"), "{body}");
    assert_eq!(servers.state.chat_attempts.load(Ordering::Relaxed), 8);
    let mut recovery = Vec::new();
    for _ in 0..8 {
        recovery.push(diagnostics.recv().await.expect("recovery diagnostic"));
    }
    let first = &recovery[0];
    let second = &recovery[1];
    let last = &recovery[7];
    assert_eq!(
        first.recovery_outcome,
        Some(BridgeRecoveryOutcome::Retrying)
    );
    assert_eq!(first.attempt, Some(BridgeAttemptBucket::First));
    assert_eq!(
        second.recovery_outcome,
        Some(BridgeRecoveryOutcome::Retrying)
    );
    assert_eq!(second.attempt, Some(BridgeAttemptBucket::Second));
    assert_eq!(second.cache_replay_detected, Some(true));
    assert_eq!(second.cache_bypass_attempted, Some(true));
    assert_eq!(
        last.recovery_outcome,
        Some(BridgeRecoveryOutcome::Exhausted)
    );
    assert_eq!(last.attempt, Some(BridgeAttemptBucket::Later));
    assert_eq!(last.cache_replay_detected, Some(true));
    assert_eq!(last.cache_bypass_attempted, Some(true));
    {
        let requests = servers.state.chat_requests.lock().expect("request lock");
        assert!(requests.windows(2).all(|pair| pair[0] != pair[1]));
    }
    {
        let headers = servers.state.chat_headers.lock().expect("header lock");
        assert!(!headers[0].contains_key(header::CACHE_CONTROL));
        assert!(
            headers[1..]
                .iter()
                .all(|headers| headers.contains_key(header::CACHE_CONTROL))
        );
    }
    servers.shutdown().await;
}
