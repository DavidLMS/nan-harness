use super::chunk;
use super::completion::finish_events;
use super::state::StreamState;
use super::tools::{custom_input, normalized_arguments, parsed_arguments};
use super::{recovery_retry_delay_with_jitter, stream_failure_outcome, translate};
use crate::error::{ApiError, UpstreamTimeoutPhase};
use crate::responses::request::ToolCatalog;
use crate::stream_common::test_support::response;
use crate::usage::{RequestUsageGuard, new_usage};
use futures_util::StreamExt;
use nan_harness_coordinator::RetryDirective;
use std::time::Duration;

fn usage_guard() -> RequestUsageGuard {
    RequestUsageGuard::new(&new_usage(), "qwen3.6")
}

#[test]
fn response_recovery_uses_growing_delays_and_honors_the_coordinator() {
    assert_eq!(
        recovery_retry_delay_with_jitter(0, RetryDirective::Complete, Duration::from_millis(400),),
        Duration::from_millis(1_400)
    );
    assert_eq!(
        recovery_retry_delay_with_jitter(
            1,
            RetryDirective::RetryAfter(Duration::from_millis(2_800)),
            Duration::from_millis(300),
        ),
        Duration::from_millis(2_800)
    );
    assert_eq!(
        recovery_retry_delay_with_jitter(
            1,
            RetryDirective::RetryAfter(Duration::from_secs(1)),
            Duration::from_secs(5),
        ),
        Duration::from_secs(3)
    );
}

#[test]
fn inactivity_is_reported_to_the_coordinator_as_a_timeout() {
    assert_eq!(
        stream_failure_outcome(&ApiError::UpstreamTimeout(UpstreamTimeoutPhase::Inactivity)),
        nan_harness_coordinator::AttemptOutcome::Timeout
    );
}

#[test]
fn extracts_freeform_input_from_chat_arguments() {
    assert_eq!(
        custom_input(r#"{"input":"*** Begin Patch"}"#),
        "*** Begin Patch"
    );
    assert_eq!(custom_input("raw patch"), "raw patch");
}

#[test]
fn preserves_tool_argument_fallbacks() {
    assert_eq!(
        normalized_arguments(r#"{"path":"src"}"#),
        r#"{"path":"src"}"#
    );
    assert_eq!(normalized_arguments("not json"), r#"{"input":"not json"}"#);
    assert_eq!(
        parsed_arguments("not json"),
        serde_json::json!({"input": "not json"})
    );
}

#[test]
fn rejects_incomplete_tool_calls() {
    let chunk =
        chunk::parse(r#"{"choices":[{"delta":{"tool_calls":[{"index":0}]}}]}"#).expect("chunk");
    let mut state = StreamState::default();
    for choice in chunk.choices {
        for tool_call in choice.delta.tool_calls {
            state.update_tool(tool_call);
        }
    }
    assert!(finish_events(&state, &ToolCatalog::default()).is_err());
}

#[test]
fn completes_reasoning_as_a_responses_reasoning_item() {
    let mut state = StreamState::default();
    state.append_reasoning("Inspect before editing.");
    let events = finish_events(&state, &ToolCatalog::default()).expect("events");
    let rendered = format!("{events:?}");
    assert!(rendered.contains("response.reasoning_summary_text.done"));
    assert!(rendered.contains("Inspect before editing."));
    assert!(rendered.contains("\\\"type\\\":\\\"reasoning\\\""));
}

#[tokio::test]
async fn reports_typed_upstream_error_before_processing_deltas() {
    let events = translate(
        response("data: {\"error\":{\"message\":\"typed boom\",\"type\":\"api_error\"}}\n\n"),
        ToolCatalog::default(),
        usage_guard(),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(rendered.contains("event: response.failed"), "{rendered}");
    assert!(
        rendered.contains("typed boom [NH-BRIDGE-105]"),
        "{rendered}"
    );
    assert!(!rendered.contains("event: response.created"), "{rendered}");
}

#[tokio::test]
async fn reports_fallback_upstream_error_before_processing_deltas() {
    let events = translate(
        response(
            "data: {\"error\":{\"message\":\"fallback boom\",\"type\":\"api_error\"},\"choices\":\"invalid\"}\n\n",
        ),
        ToolCatalog::default(),
        usage_guard(),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(rendered.contains("event: response.failed"), "{rendered}");
    assert!(
        rendered.contains("fallback boom [NH-BRIDGE-105]"),
        "{rendered}"
    );
    assert!(!rendered.contains("invalid streaming chunk"), "{rendered}");
    assert!(!rendered.contains("event: response.created"), "{rendered}");
}

#[tokio::test]
async fn reports_null_upstream_error_before_processing_deltas() {
    let events = translate(
        response("data: {\"error\":null}\n\n"),
        ToolCatalog::default(),
        usage_guard(),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(rendered.contains("event: response.failed"), "{rendered}");
    assert!(
        rendered.contains("NaN returned a streaming error [NH-BRIDGE-105]"),
        "{rendered}"
    );
    assert!(!rendered.contains("event: response.created"), "{rendered}");
}

#[tokio::test]
async fn preserves_invalid_streaming_json_error() {
    let events = translate(
        response("data: {not valid json}\n\n"),
        ToolCatalog::default(),
        usage_guard(),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(rendered.contains("invalid streaming JSON:"), "{rendered}");
    assert!(rendered.contains("NH-BRIDGE-105"), "{rendered}");
    assert!(!rendered.contains("event: response.created"), "{rendered}");
}

#[test]
fn preserves_invalid_streaming_chunk_error() {
    let error = chunk::parse(r#"{"choices":"invalid"}"#).expect_err("chunk should fail");
    assert!(
        error
            .to_string()
            .starts_with("NaN returned an invalid response: invalid streaming chunk:")
    );
    assert_eq!(error.code(), "NH-BRIDGE-105");
}

#[tokio::test]
async fn rejects_truncated_text_stream() {
    let events = translate(
        response(
            "data: {\"id\":\"resp_1\",\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
        ),
        ToolCatalog::default(),
        usage_guard(),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(rendered.contains("event: response.failed"), "{rendered}");
    assert!(rendered.contains("stream ended before the [DONE] marker"));
    assert!(
        !rendered.contains("event: response.completed"),
        "{rendered}"
    );
}

#[tokio::test]
async fn rejects_truncated_tool_stream_even_with_valid_arguments() {
    let events = translate(
        response(
            "data: {\"id\":\"resp_1\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"Read\",\"arguments\":\"{}\"}}]}}]}\n\n",
        ),
        ToolCatalog::default(),
        usage_guard(),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(rendered.contains("event: response.failed"), "{rendered}");
    assert!(
        !rendered.contains("event: response.completed"),
        "{rendered}"
    );
}

#[tokio::test]
async fn completes_stream_after_done_marker() {
    let events = translate(
        response(
            "data: {\"id\":\"resp_1\",\"choices\":[{\"delta\":{\"content\":\"complete\"}}]}\n\ndata: [DONE]\n\n",
        ),
        ToolCatalog::default(),
        usage_guard(),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(rendered.contains("event: response.completed"), "{rendered}");
    assert!(!rendered.contains("event: response.failed"), "{rendered}");
}
