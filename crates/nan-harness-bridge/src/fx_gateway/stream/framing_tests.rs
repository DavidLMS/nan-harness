use super::translate;
use crate::sse_framing::MAX_SSE_EVENT_BYTES;
use crate::stream_common::test_support::response;
use crate::upstream::NanClient;
use crate::usage::{RequestUsageGuard, new_usage, snapshot};
use futures_util::StreamExt;
use nan_harness_core::SecretValue;
use std::sync::Arc;

#[tokio::test]
async fn framing_validation_precedes_coordinator_success() {
    use crate::stream_common::coordination_test_support as coordination;
    use nan_harness_coordinator::AttemptOutcome;
    let test_name = concat!(
        module_path!(),
        "::framing_validation_precedes_coordinator_success"
    )
    .trim_start_matches("nan_harness_bridge::");
    if !coordination::in_isolated_child(test_name).await {
        return;
    }
    for oversized in [true, false] {
        let wire = if oversized {
            format!(
                "data: {}\n\ndata: [DONE]\n\n",
                "x".repeat(MAX_SSE_EVENT_BYTES)
            )
        } else {
            "data: [DONE]\r\r".to_owned()
        };
        let (response, observed) = coordination::response(wire).await;
        let usage = new_usage();
        let upstream = NanClient::new(
            "http://127.0.0.1",
            Arc::new(SecretValue::new("test-key").expect("key")),
            "framing-test",
        )
        .expect("upstream");
        let events = translate(
            response,
            "qwen3.6".to_owned(),
            upstream,
            None,
            String::new(),
            RequestUsageGuard::new(&usage, "qwen3.6"),
        )
        .collect::<Vec<_>>()
        .await;
        let expected = if oversized {
            AttemptOutcome::InvalidResponse
        } else {
            AttemptOutcome::Success
        };
        assert_eq!(
            observed.await.expect("coordinator observation"),
            expected,
            "{events:?}"
        );
        assert_eq!(snapshot(&usage).completed_requests(), u64::from(!oversized));
    }
}

#[tokio::test]
async fn oversized_unfinished_event_uses_safe_error_and_incomplete_usage() {
    let usage = new_usage();
    let raw_marker = "x".repeat(128);
    let wire = format!("data: {}", "x".repeat(MAX_SSE_EVENT_BYTES));
    let upstream = NanClient::new(
        "http://127.0.0.1",
        Arc::new(SecretValue::new("test-provider-key").expect("valid test key")),
        "fx_framing_test",
    )
    .expect("test upstream should build");
    let events = translate(
        response(&wire),
        "qwen3.6".to_owned(),
        upstream,
        None,
        "fallback query".to_owned(),
        RequestUsageGuard::new(&usage, "qwen3.6"),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(rendered.contains("api-error"), "{rendered}");
    assert!(rendered.contains("8 MiB framing limit"), "{rendered}");
    assert!(rendered.contains("NH-BRIDGE-105"), "{rendered}");
    assert!(!rendered.contains(&raw_marker), "{rendered}");
    assert!(!rendered.contains("finishReason"), "{rendered}");
    assert_eq!(snapshot(&usage).completed_requests(), 0);
}
