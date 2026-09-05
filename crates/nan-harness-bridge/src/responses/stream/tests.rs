use super::chunk;
use super::completion::finish_events;
use super::state::StreamState;
use super::tools::{custom_input, normalized_arguments, parsed_arguments};
use super::{
    RecoveryNudge, TranslationRequest, recovery_body, recovery_retry_delay_with_jitter,
    repeated_response_id, stream_failure_outcome, translate,
    translate_request_with_progress_interval,
};
use crate::error::{ApiError, UpstreamTimeoutPhase};
use crate::responses::request::ToolCatalog;
use crate::stream_common::test_support::response;
use crate::upstream::NanClient;
use crate::usage::{RequestUsageGuard, new_usage};
use axum::Router;
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::post;
use futures_util::StreamExt;
use nan_harness_coordinator::{RequestPriority, RetryDirective};
use nan_harness_core::SecretValue;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

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
fn cache_replay_requires_two_present_equal_provider_ids() {
    assert!(repeated_response_id(Some("chatcmpl-a"), Some("chatcmpl-a")));
    assert!(!repeated_response_id(
        Some("chatcmpl-a"),
        Some("chatcmpl-b")
    ));
    assert!(!repeated_response_id(None, Some("chatcmpl-a")));
    assert!(!repeated_response_id(Some("chatcmpl-a"), None));
}

#[test]
fn recovery_prepends_an_internal_system_instruction() {
    let body = serde_json::json!({
        "model": "glm5.3-flash",
        "messages": [
            {"role": "system", "content": "Original instructions"},
            {"role": "user", "content": "Inspect the scheduler"}
        ],
        "stream": true
    });

    let recovered = recovery_body(&body, RecoveryNudge::Output);

    assert_eq!(recovered["model"], body["model"]);
    assert_eq!(recovered["stream"], body["stream"]);
    assert_eq!(recovered["messages"][0]["role"], "system");
    assert!(
        recovered["messages"][0]["content"]
            .as_str()
            .expect("recovery content")
            .starts_with("nan-harness internal recovery ")
    );
    assert_eq!(recovered["messages"][1], body["messages"][0]);
    assert_eq!(recovered["messages"][2], body["messages"][1]);
    let tool_recovery = recovery_body(&body, RecoveryNudge::Tool);
    assert_ne!(tool_recovery["messages"][0], recovered["messages"][0]);
    assert_eq!(tool_recovery["messages"][1], body["messages"][0]);
    assert_eq!(tool_recovery["messages"][2], body["messages"][1]);
    let next_recovery = recovery_body(&body, RecoveryNudge::Tool);
    assert_ne!(tool_recovery["messages"][0], next_recovery["messages"][0]);
    assert_eq!(body["messages"][0]["content"], "Original instructions");
    assert_eq!(body["messages"].as_array().expect("messages").len(), 2);
}

#[tokio::test]
async fn emits_protocol_progress_while_waiting_for_upstream_headers() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("upstream should bind");
    let address = listener.local_addr().expect("upstream address");
    let app = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            tokio::time::sleep(Duration::from_millis(75)).await;
            (
                [(header::CONTENT_TYPE, "text/event-stream")],
                "data: {\"id\":\"chatcmpl_test\",\"choices\":[{\"delta\":{\"content\":\"done\"}}]}\n\ndata: [DONE]\n\n",
            )
                .into_response()
        }),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("upstream should serve");
    });
    let upstream = NanClient::new(
        &format!("http://{address}/v1"),
        Arc::new(SecretValue::new("provider-key").expect("valid key")),
        "progress-test",
    )
    .expect("client should build");
    let capture = upstream.begin_capture(b"{}");
    let (diagnostics, _) = tokio::sync::mpsc::unbounded_channel();
    let events = translate_request_with_progress_interval(
        TranslationRequest {
            upstream,
            body: serde_json::json!({"model": "qwen3.6"}),
            tools: ToolCatalog::default(),
            usage_guard: usage_guard(),
            diagnostics,
            priority: RequestPriority::Foreground,
            capture,
        },
        Duration::from_millis(10),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");
    assert!(
        rendered.matches("response.in_progress").count() >= 3,
        "{rendered}"
    );
    assert!(rendered.contains("response.completed"), "{rendered}");
    server.abort();
}

#[test]
fn extracts_freeform_input_from_chat_arguments() {
    assert_eq!(
        custom_input(
            "apply_patch",
            r#"{"input":"*** Begin Patch\n*** End Patch"}"#
        )
        .expect("wrapped patch"),
        "*** Begin Patch\n*** End Patch"
    );
    assert_eq!(
        custom_input("custom", "raw input").expect("raw custom input"),
        "raw input"
    );
    assert_eq!(
        custom_input(
            "apply_patch",
            r#"{"input":"*** Begin Patch\n*** End Patch\n"#
        )
        .expect("patch with a missing JSON string and object suffix"),
        "*** Begin Patch\n*** End Patch\n"
    );
    assert_eq!(
        custom_input(
            "apply_patch",
            r#"{"input":"*** Begin Patch\n*** End Patch""#
        )
        .expect("patch with a missing JSON object suffix"),
        "*** Begin Patch\n*** End Patch"
    );
    assert!(custom_input("apply_patch", "{").is_err());
    assert!(custom_input("apply_patch", r#"{"input":"*** Begin Patch\ntruncated"#).is_err());
    assert!(custom_input("apply_patch", r#"{"input":"{}"}"#).is_err());
    assert!(custom_input("custom", "{}").is_err());
}

#[test]
fn preserves_complete_large_patch_input() {
    let patch = format!(
        "*** Begin Patch\n*** Add File: example.txt\n{}*** End Patch",
        "+complete synthetic line\n".repeat(200)
    );
    assert!(patch.len() > 3_000);
    let arguments = serde_json::json!({"input": patch}).to_string();
    assert_eq!(
        custom_input("apply_patch", &arguments).expect("complete patch"),
        patch
    );
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
    assert!(finish_events(&state, &ToolCatalog::default(), false).is_err());
}

#[test]
fn completes_reasoning_as_a_responses_reasoning_item() {
    let mut state = StreamState::default();
    state.append_reasoning("Inspect before editing.");
    let events = finish_events(&state, &ToolCatalog::default(), false).expect("events");
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

#[tokio::test]
async fn recovery_buffer_accepts_exactly_eight_mib_and_rejects_one_more_byte() {
    for overflow in [false, true] {
        let content = "x".repeat(super::MAX_RECOVERY_BUFFER_BYTES + usize::from(overflow));
        let chunk = serde_json::json!({"choices": [{"delta": {"content": content}}]});
        let wire = format!("data: {chunk}\n\ndata: [DONE]\n\n");
        let catalog = ToolCatalog::default();
        let mut usage = usage_guard();
        let items = super::translate_items(response(&wire), &catalog, &mut usage, true, false);
        futures_util::pin_mut!(items);
        let mut emitted = 0;
        let mut completed = false;
        let mut rejected = false;
        while let Some(item) = items.next().await {
            match item {
                super::TranslationItem::Event(_) => emitted += 1,
                super::TranslationItem::Complete => completed = true,
                super::TranslationItem::Recoverable { error, .. } => {
                    assert!(matches!(error, ApiError::InvalidUpstream(_)));
                    assert!(error.to_string().contains("8 MiB recovery limit"));
                    rejected = true;
                }
                _ => panic!("unexpected translation outcome"),
            }
        }
        assert_eq!(completed, !overflow);
        assert_eq!(rejected, overflow);
        assert_eq!(emitted == 0, overflow);
    }
}

#[tokio::test]
async fn committed_text_is_followed_by_failure_without_becoming_recoverable() {
    let catalog = ToolCatalog::default();
    let mut usage = usage_guard();
    let items = super::translate_items(
        response("data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n"),
        &catalog,
        &mut usage,
        false,
        false,
    );
    futures_util::pin_mut!(items);
    let mut delivered_text = false;
    let mut failed = false;
    while let Some(item) = items.next().await {
        match item {
            super::TranslationItem::Event(event) => {
                let rendered = format!("{event:?}");
                delivered_text |= rendered.contains("response.output_text.delta");
            }
            super::TranslationItem::Failed(_) => {
                assert!(
                    delivered_text,
                    "text must have been delivered before failure"
                );
                failed = true;
            }
            _ => panic!("a committed response must fail without recovery or completion"),
        }
    }
    assert!(failed);
}
