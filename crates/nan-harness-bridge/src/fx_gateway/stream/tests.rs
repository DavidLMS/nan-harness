use super::translate;
use crate::stream_common::test_support::response;
use crate::upstream::NanClient;
use crate::usage::{ProviderUsageSnapshot, RequestUsageGuard, new_usage, snapshot};
use futures_util::StreamExt;
use nan_harness_core::SecretValue;
use serde_json::json;
use std::sync::Arc;

fn upstream() -> NanClient {
    NanClient::new(
        "http://127.0.0.1",
        Arc::new(SecretValue::new("test-provider-key").expect("test key should be valid")),
    )
    .expect("test upstream should build")
}

fn usage_guard() -> RequestUsageGuard {
    RequestUsageGuard::new(&new_usage(), "qwen3.6")
}

async fn render_stream(body: &str) -> (String, ProviderUsageSnapshot) {
    let usage = new_usage();
    let events = translate(
        response(body),
        "qwen3.6".to_owned(),
        upstream(),
        None,
        "fallback query".to_owned(),
        RequestUsageGuard::new(&usage, "qwen3.6"),
    )
    .collect::<Vec<_>>()
    .await;
    (format!("{events:?}"), snapshot(&usage))
}

fn assert_failed_stream(label: &str, rendered: &str, usage: &ProviderUsageSnapshot) {
    assert!(rendered.contains("api-error"), "{label}: {rendered}");
    assert!(rendered.contains("NH-BRIDGE-105"), "{label}: {rendered}");
    assert!(!rendered.contains("finishReason"), "{label}: {rendered}");
    assert_eq!(usage.completed_requests(), 0, "{label}: {usage:?}");
}

#[tokio::test]
async fn typed_stream_preserves_reasoning_text_usage_and_event_order() {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"think\",",
        "\"content\":\"answer\"},\"finish_reason\":\"stop\"}],",
        "\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":5,",
        "\"completion_tokens_details\":{\"reasoning_tokens\":2}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (rendered, usage) = render_stream(body).await;
    let mut cursor = 0;
    for marker in [
        "reasoning-start",
        "reasoning-delta",
        "text-start",
        "text-delta",
        "reasoning-end",
        "text-end",
        "finishReason",
    ] {
        let offset = rendered[cursor..]
            .find(marker)
            .unwrap_or_else(|| panic!("missing {marker}: {rendered}"));
        cursor += offset + marker.len();
    }
    assert!(!rendered.contains("api-error"), "{rendered}");
    assert_eq!(usage.completed_requests(), 1);
    assert_eq!(usage.input_tokens(), 3);
    assert_eq!(usage.output_tokens(), 5);
    assert_eq!(usage.reasoning_tokens(), 2);
}

#[tokio::test]
async fn typed_stream_reconstructs_fragmented_tools_exactly_once() {
    let first = json!({
        "choices": [{"delta": {"tool_calls": [{
            "index": 0,
            "id": "call_",
            "function": {"name": "read_", "arguments": r#"{"path":""#}
        }]}}]
    });
    let second = json!({
        "choices": [{"delta": {"tool_calls": [{
            "id": "1",
            "function": {"name": "file", "arguments": "README.md\"}"}
        }]}, "finish_reason": "tool_calls"}]
    });
    let body = format!("data: {first}\n\ndata: {second}\n\ndata: [DONE]\n\n");
    let (rendered, usage) = render_stream(&body).await;

    assert_eq!(rendered.matches("call_1").count(), 1, "{rendered}");
    assert_eq!(rendered.matches("read_file").count(), 1, "{rendered}");
    assert_eq!(rendered.matches("README.md").count(), 1, "{rendered}");
    assert!(rendered.contains("tool-calls"), "{rendered}");
    assert_eq!(usage.completed_requests(), 1);
}

#[tokio::test]
async fn typed_stream_accepts_missing_optional_fields() {
    let (rendered, usage) =
        render_stream("data: {}\n\ndata: {\"choices\":[{}],\"usage\":{}}\n\ndata: [DONE]\n\n")
            .await;

    assert!(rendered.contains("finishReason"), "{rendered}");
    assert!(!rendered.contains("api-error"), "{rendered}");
    assert_eq!(usage.completed_requests(), 1);
    assert_eq!(usage.total_tokens(), 0);
}

#[tokio::test]
async fn typed_stream_rejects_malformed_json() {
    let (rendered, usage) = render_stream("data: {not valid json}\n\n").await;

    assert!(rendered.contains("invalid streaming JSON:"), "{rendered}");
    assert_failed_stream("malformed JSON", &rendered, &usage);
}

#[tokio::test]
async fn typed_stream_rejects_incompatible_chunk_shapes() {
    for (label, chunk) in [
        ("choices", r#"{"choices":"invalid"}"#),
        ("choice", r#"{"choices":[[]]}"#),
        ("delta", r#"{"choices":[{"delta":[]}] }"#),
        ("usage", r#"{"usage":[]}"#),
        (
            "tool calls",
            r#"{"choices":[{"delta":{"tool_calls":"invalid"}}]}"#,
        ),
        (
            "tool call",
            r#"{"choices":[{"delta":{"tool_calls":[[]]}}]}"#,
        ),
        (
            "tool function",
            r#"{"choices":[{"delta":{"tool_calls":[{"function":"invalid"}]}}]}"#,
        ),
        (
            "tool index",
            r#"{"choices":[{"delta":{"tool_calls":[{"index":18446744073709551616}]}}]}"#,
        ),
    ] {
        let body = format!("data: {chunk}\n\ndata: [DONE]\n\n");
        let (rendered, usage) = render_stream(&body).await;
        assert!(
            rendered.contains("invalid streaming chunk:"),
            "{label}: {rendered}"
        );
        assert_failed_stream(label, &rendered, &usage);
    }
}

#[tokio::test]
async fn typed_stream_reports_embedded_errors_with_or_without_messages() {
    for (label, chunk, message) in [
        (
            "message",
            r#"{"error":{"message":"fx boom"},"choices":"invalid"}"#,
            "fx boom",
        ),
        (
            "fallback",
            r#"{"error":{"type":"api_error"}}"#,
            "NaN returned a streaming error",
        ),
    ] {
        let body = format!("data: {chunk}\n\ndata: [DONE]\n\n");
        let (rendered, usage) = render_stream(&body).await;
        assert!(rendered.contains(message), "{label}: {rendered}");
        assert_failed_stream(label, &rendered, &usage);
    }
}

#[tokio::test]
async fn rejects_truncated_text_stream() {
    let events = translate(
        response(
            "data: {\"id\":\"chatcmpl_fx\",\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
        ),
        "qwen3.6".to_owned(),
        upstream(),
        None,
        "fallback query".to_owned(),
        usage_guard(),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(
        rendered.contains("stream ended before the [DONE] marker"),
        "{rendered}"
    );
    assert!(!rendered.contains("finishReason"), "{rendered}");
}

#[tokio::test]
async fn rejects_truncated_tool_stream_without_emitting_tool_call() {
    let events = translate(
        response(
            "data: {\"id\":\"chatcmpl_fx\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_partial\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"README\"}}]}}]}\n\n",
        ),
        "qwen3.6".to_owned(),
        upstream(),
        None,
        "fallback query".to_owned(),
        usage_guard(),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(
        rendered.contains("stream ended before the [DONE] marker"),
        "{rendered}"
    );
    assert!(!rendered.contains("toolCallId"), "{rendered}");
    assert!(!rendered.contains("finishReason"), "{rendered}");
    assert!(!rendered.contains("call_partial"), "{rendered}");
}

#[tokio::test]
async fn completes_stream_after_done_marker() {
    let events = translate(
        response(
            "data: {\"id\":\"chatcmpl_fx\",\"choices\":[{\"delta\":{\"content\":\"complete\"}}]}\n\ndata: [DONE]\n\n",
        ),
        "qwen3.6".to_owned(),
        upstream(),
        None,
        "fallback query".to_owned(),
        usage_guard(),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(rendered.contains("response-metadata"), "{rendered}");
    assert!(rendered.contains("text-start"), "{rendered}");
    assert!(rendered.contains("text-delta"), "{rendered}");
    assert!(rendered.contains("text-end"), "{rendered}");
    assert!(rendered.contains("finishReason"), "{rendered}");
    assert!(!rendered.contains("api-error"), "{rendered}");
}

#[tokio::test]
async fn rejects_empty_tool_name_without_emitting_tool_call() {
    let events = translate(
        response(
            "data: {\"id\":\"chatcmpl_fx\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_empty_name\",\"function\":{\"name\":\"\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n",
        ),
        "qwen3.6".to_owned(),
        upstream(),
        None,
        "fallback query".to_owned(),
        usage_guard(),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(
        rendered.contains("tool call ended without a valid id or name"),
        "{rendered}"
    );
    assert!(!rendered.contains("toolCallId"), "{rendered}");
    assert!(!rendered.contains("finishReason"), "{rendered}");
    assert!(!rendered.contains("call_empty_name"), "{rendered}");
}

#[tokio::test]
async fn rejects_non_object_tool_arguments_after_done_marker() {
    let events = translate(
        response(
            "data: {\"id\":\"chatcmpl_fx\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_invalid_args\",\"function\":{\"name\":\"read_file\",\"arguments\":\"[]\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n",
        ),
        "qwen3.6".to_owned(),
        upstream(),
        None,
        "fallback query".to_owned(),
        usage_guard(),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(
        rendered.contains("tool call ended with invalid JSON object arguments"),
        "{rendered}"
    );
    assert!(!rendered.contains("toolCallId"), "{rendered}");
    assert!(!rendered.contains("finishReason"), "{rendered}");
    assert!(!rendered.contains("call_invalid_args"), "{rendered}");
}
