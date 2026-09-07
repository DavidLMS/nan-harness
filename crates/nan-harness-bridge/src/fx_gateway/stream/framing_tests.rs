use super::translate;
use crate::sse_framing::MAX_SSE_EVENT_BYTES;
use crate::stream_common::test_support::response;
use crate::upstream::NanClient;
use crate::usage::{RequestUsageGuard, new_usage, snapshot};
use futures_util::StreamExt;
use nan_harness_core::SecretValue;
use std::sync::Arc;

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
