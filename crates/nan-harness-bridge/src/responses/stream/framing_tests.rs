use super::{TranslationItem, translate_items};
use crate::error::ApiError;
use crate::responses::request::ToolCatalog;
use crate::sse_framing::MAX_SSE_EVENT_BYTES;
use crate::stream_common::test_support::response;
use crate::usage::{RequestUsageGuard, new_usage, snapshot};
use futures_util::StreamExt;

#[tokio::test]
async fn precommit_overflow_is_recoverable_without_completing_usage() {
    let usage = new_usage();
    let catalog = ToolCatalog::default();
    let mut usage_guard = RequestUsageGuard::new(&usage, "qwen3.6");
    let wire = format!("data: {}", "x".repeat(MAX_SSE_EVENT_BYTES));
    let items = translate_items(response(&wire), &catalog, &mut usage_guard, true, false);
    futures_util::pin_mut!(items);
    let mut recoverable = 0;

    while let Some(item) = items.next().await {
        match item {
            TranslationItem::Recoverable(failure) => {
                assert!(matches!(failure.error, ApiError::InvalidUpstream(_)));
                assert!(failure.error.to_string().contains("8 MiB framing limit"));
                recoverable += 1;
            }
            _ => panic!("precommit framing overflow must only request recovery"),
        }
    }

    assert_eq!(recoverable, 1);
    assert_eq!(snapshot(&usage).completed_requests(), 0);
}

#[tokio::test]
async fn postcommit_overflow_fails_without_requesting_recovery() {
    let usage = new_usage();
    let catalog = ToolCatalog::default();
    let mut usage_guard = RequestUsageGuard::new(&usage, "qwen3.6");
    let first = serde_json::json!({"choices": [{"delta": {"content": "committed"}}]});
    let wire = format!("data: {first}\n\ndata: {}", "x".repeat(MAX_SSE_EVENT_BYTES));
    let items = translate_items(response(&wire), &catalog, &mut usage_guard, false, false);
    futures_util::pin_mut!(items);
    let mut delivered_text = false;
    let mut failed = false;

    while let Some(item) = items.next().await {
        match item {
            TranslationItem::Event(event) => {
                delivered_text |= format!("{event:?}").contains("response.output_text.delta");
            }
            TranslationItem::Failed(error) => {
                assert!(delivered_text, "content must be committed before failure");
                assert!(error.to_string().contains("8 MiB framing limit"));
                failed = true;
            }
            TranslationItem::Recoverable(_) => {
                panic!("a committed response must never request recovery");
            }
            _ => panic!("postcommit overflow must fail without completion"),
        }
    }

    assert!(failed);
    assert_eq!(snapshot(&usage).completed_requests(), 0);
}
