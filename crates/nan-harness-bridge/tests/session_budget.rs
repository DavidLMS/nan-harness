#[path = "session_budget/support.rs"]
mod support;

use nan_harness_bridge::BridgeDiagnosticReason;
use reqwest::StatusCode;
use serde_json::Value;
use std::{sync::atomic::Ordering, time::Duration};
use support::{TRANSPORTS, Transport, events, start};

const CHILD: &str = "NAN_TEST_SESSION_BUDGET";

#[tokio::test]
async fn session_budget_stops_preserve_transport_contracts() {
    if std::env::var_os(CHILD).is_some() {
        let daemon = start_coordinator().await;
        for transport in TRANSPORTS {
            concurrent_admitted_requests(transport).await;
            for limit in [25, 30] {
                exercise(transport, limit).await;
            }
        }
        recovery_budget_stop(false).await;
        recovery_budget_stop(true).await;
        daemon.abort();
        return;
    }
    let directory = tempfile::tempdir().expect("isolated configuration");
    let mut child = tokio::process::Command::new(std::env::current_exe().expect("test executable"));
    child
        .args([
            "--exact",
            "session_budget_stops_preserve_transport_contracts",
            "--nocapture",
        ])
        .env_clear()
        .env(CHILD, "1")
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .env("NAN_HARNESS_INTERNAL_MANAGED_PROCESS", "1")
        .kill_on_drop(true);
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        child.env("LLVM_PROFILE_FILE", profile);
    }
    let status = tokio::time::timeout(Duration::from_mins(1), child.status())
        .await
        .expect("test deadline")
        .expect("child status");
    assert!(status.success());
}

async fn start_coordinator()
-> tokio::task::JoinHandle<Result<(), nan_harness_coordinator::CoordinatorError>> {
    let daemon = tokio::spawn(nan_harness_coordinator::run_daemon());
    let receipt = nan_harness_coordinator::config_directory()
        .expect("configuration")
        .join("coordinator/v1/receipt.json");
    tokio::time::timeout(Duration::from_secs(5), async {
        while !receipt.exists() {
            assert!(!daemon.is_finished());
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("coordinator startup");
    daemon
}

async fn exercise(transport: Transport, limit: u64) {
    let mut system = start(transport, limit).await;
    let mut diagnostics = system.bridge.take_diagnostics();
    let request = transport.request(true);
    let initial = system.send(&request).await;
    assert_eq!(initial.status(), StatusCode::OK, "{transport:?}");
    let initial = initial.text().await.expect("initial completion");
    assert!(
        initial.contains("Synthetic response"),
        "{transport:?}: {initial}"
    );
    let before = system.bridge.usage();
    assert_eq!(before.total_tokens(), 30, "{transport:?}");
    assert_eq!(before.inference_requests(), 1);

    // Concurrent and repeated denials must neither send nor become unknown usage.
    let (one, two) = tokio::join!(system.send(&request), system.send(&request));
    for response in [one, two] {
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.text().await.expect("notice body");
        assert_notice(transport, &body, limit);
    }
    if matches!(transport, Transport::Anthropic | Transport::Chat) {
        let response = system.send(&transport.request(false)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let value: Value = response.json().await.expect("non-streaming notice");
        assert!(value.to_string().contains("session token budget reached"));
        if matches!(transport, Transport::Anthropic) {
            assert_eq!(value["stop_reason"], "end_turn");
            assert_eq!(value["usage"]["input_tokens"], 0);
        } else {
            assert_eq!(value["choices"][0]["finish_reason"], "stop");
            assert_eq!(value["usage"]["total_tokens"], 0);
        }
    }
    for request in transport.constrained() {
        let response = system.send(&request).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{transport:?}");
        assert!(
            response.headers()["content-type"]
                .to_str()
                .expect("content type")
                .starts_with("application/json")
        );
        let body = response.text().await.expect("contract rejection");
        assert!(body.contains("NH-BRIDGE-109"), "{body}");
    }
    let response = system.send(&request).await;
    assert_notice(
        transport,
        &response.text().await.expect("repeated notice"),
        limit,
    );
    assert_eq!(system.provider.sends.load(Ordering::SeqCst), 1);
    assert_eq!(
        system.bridge.usage(),
        before,
        "local notices cannot alter provider accounting"
    );
    let mut count = 0;
    while let Ok(diagnostic) = diagnostics.try_recv() {
        assert_eq!(
            diagnostic.reason,
            BridgeDiagnosticReason::SessionBudgetReached {
                consumed: 30,
                limit
            }
        );
        count += 1;
    }
    assert!(count >= 5, "local diagnostics record the stop");
    system.stop().await;
}

fn assert_notice(transport: Transport, body: &str, limit: u64) {
    let events = events(body);
    let text = events
        .iter()
        .find_map(|event| match transport {
            Transport::Responses => (event["type"] == "response.output_text.delta")
                .then(|| event["delta"].as_str())
                .flatten(),
            Transport::Anthropic => (event["type"] == "content_block_delta")
                .then(|| event["delta"]["text"].as_str())
                .flatten(),
            Transport::Chat => event["choices"][0]["delta"]["content"].as_str(),
            Transport::Fx => (event["type"] == "text-delta")
                .then(|| event["delta"].as_str())
                .flatten(),
        })
        .expect("assistant notice text");
    assert_eq!(
        text,
        format!(
            "nan-harness: session token budget reached.\nUsed 30 of {limit} tokens. No further inference requests will be sent in this session.\nTo continue, start a new nan-harness launch with a higher budget and resume your conversation."
        )
    );
    assert!(!body.contains("NH-BRIDGE-109"));
    assert!(!body.contains("response.failed"));
    assert!(!body.contains("tool_call"));
    assert!(!body.contains("tool_use"));
    let last = events.last().expect("terminal event");
    match transport {
        Transport::Responses => {
            assert_eq!(last["type"], "response.completed");
            assert_eq!(last["response"]["status"], "completed");
            assert_eq!(last["response"]["output"][0]["content"][0]["text"], text);
            let types: Vec<_> = events
                .iter()
                .filter_map(|event| event["type"].as_str())
                .collect();
            assert!(types.windows(6).any(|events| events
                == [
                    "response.output_item.added",
                    "response.content_part.added",
                    "response.output_text.delta",
                    "response.output_text.done",
                    "response.content_part.done",
                    "response.output_item.done"
                ]));
            assert_eq!(last["response"]["usage"]["total_tokens"], 0);
        }
        Transport::Anthropic => {
            assert_eq!(last["type"], "message_stop");
            assert_eq!(events[events.len() - 2]["delta"]["stop_reason"], "end_turn");
        }
        Transport::Chat => {
            assert_eq!(last["choices"][0]["finish_reason"], "stop");
            assert!(body.ends_with("data: [DONE]\n\n"));
        }
        Transport::Fx => {
            assert_eq!(last["type"], "finish");
            assert_eq!(last["finishReason"]["unified"], "stop");
            assert_eq!(last["usage"]["inputTokens"]["total"], 0);
        }
    }
}

async fn recovery_budget_stop(constrained: bool) {
    let mut system = start(Transport::Responses, 30).await;
    let mut diagnostics = system.bridge.take_diagnostics();
    system.provider.empty.store(true, Ordering::SeqCst);
    let request = if constrained {
        Transport::Responses.constrained().remove(0)
    } else {
        Transport::Responses.request(true)
    };
    let response = system.send(&request).await;
    assert_eq!(
        response.status(),
        if constrained {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::OK
        }
    );
    let body = response.text().await.expect("recovery response");
    if constrained {
        assert!(body.contains("NH-BRIDGE-109"));
    } else {
        assert_notice(Transport::Responses, &body, 30);
    }
    assert_eq!(system.provider.sends.load(Ordering::SeqCst), 1);
    assert_eq!(system.bridge.usage().responses_without_usage(), 0);
    assert_eq!(
        system.bridge.usage().inference_requests(),
        1,
        "preserve the already admitted attempt"
    );
    let mut reasons = Vec::new();
    while let Ok(diagnostic) = diagnostics.try_recv() {
        reasons.push(diagnostic.reason);
    }
    assert!(
        reasons.contains(&BridgeDiagnosticReason::InvalidUpstreamResponse),
        "independent failure remains reportable"
    );
    assert!(
        reasons.contains(&BridgeDiagnosticReason::SessionBudgetReached {
            consumed: 30,
            limit: 30
        })
    );
    system.stop().await;
}

async fn concurrent_admitted_requests(transport: Transport) {
    let system = start(transport, 30).await;
    system.provider.hold.store(true, Ordering::SeqCst);
    let request = transport.request(true);
    let consume = || async {
        system
            .send(&request)
            .await
            .text()
            .await
            .expect("admitted response")
    };
    let release = async {
        tokio::time::timeout(Duration::from_secs(5), async {
            while system.provider.sends.load(Ordering::SeqCst) < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("both requests admitted before accounting");
        system.provider.hold.store(false, Ordering::SeqCst);
        system.provider.released.notify_waiters();
    };
    let (first, second, ()) = tokio::join!(consume(), consume(), release);
    for body in [first, second] {
        assert!(body.contains("Synthetic response"), "{body}");
        assert!(!body.contains("session token budget reached"));
    }
    assert_eq!(system.bridge.usage().total_tokens(), 60);
    assert_eq!(system.bridge.usage().inference_requests(), 2);
    let response = system
        .send(&request)
        .await
        .text()
        .await
        .expect("stop after admitted requests");
    assert!(response.contains("Used 60 of 30 tokens."), "{response}");
    assert_eq!(system.provider.sends.load(Ordering::SeqCst), 2);
    assert_eq!(system.bridge.usage().total_tokens(), 60);
    system.stop().await;
}
