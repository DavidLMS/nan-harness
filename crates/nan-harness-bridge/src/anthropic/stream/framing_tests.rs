use super::translate;
use crate::sse_framing::MAX_SSE_EVENT_BYTES;
use crate::stream_common::test_support::response;
use crate::usage::{RequestUsageGuard, new_usage, snapshot};
use futures_util::StreamExt;

#[tokio::test]
async fn oversized_unfinished_event_uses_safe_error_and_incomplete_usage() {
    let usage = new_usage();
    let raw_marker = "x".repeat(128);
    let wire = format!("data: {}", "x".repeat(MAX_SSE_EVENT_BYTES));
    let events = translate(
        response(&wire),
        "qwen3.6".to_owned(),
        RequestUsageGuard::new(&usage, "qwen3.6"),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(rendered.contains("event: error"), "{rendered}");
    assert!(rendered.contains("8 MiB framing limit"), "{rendered}");
    assert!(rendered.contains("NH-BRIDGE-105"), "{rendered}");
    assert!(!rendered.contains(&raw_marker), "{rendered}");
    assert!(!rendered.contains("event: message_stop"), "{rendered}");
    assert_eq!(snapshot(&usage).completed_requests(), 0);
}
