use super::chunk::{self, ToolCallDelta};
use super::processing::{finish_events, push_text_delta, push_tool_delta};
use super::state::StreamState;
use super::translate;
use crate::stream_common::test_support::response;
use crate::usage::{RequestUsageGuard, new_usage, snapshot};
use futures_util::StreamExt;
use serde_json::from_str;

fn usage_guard() -> RequestUsageGuard {
    RequestUsageGuard::new(&new_usage(), "qwen3.6")
}

#[test]
fn orders_text_and_tool_events() {
    let mut state = StreamState::default();
    let mut events = Vec::new();
    push_text_delta(&mut state, "Reading", &mut events);
    let delta: ToolCallDelta = from_str(
        r#"{"index":0,"id":"call_1","function":{"name":"Read","arguments":"{\"file_path\":"}}"#,
    )
    .expect("tool delta should deserialize");
    push_tool_delta(&mut state, delta, &mut events);
    let delta: ToolCallDelta = from_str(r#"{"index":0,"function":{"arguments":"\"README.md\"}"}}"#)
        .expect("tool delta should deserialize");
    push_tool_delta(&mut state, delta, &mut events);

    assert_eq!(events.len(), 5);
    let finished = finish_events(&state).expect("stream should finish");
    assert_eq!(finished.len(), 4);
}

#[tokio::test]
async fn preserves_interleaved_content_usage_and_completion_order() {
    let usage = new_usage();
    let events = translate(
        response(concat!(
            "data: {\"id\":\"msg_1\",\"choices\":[{\"delta\":{",
            "\"reasoning_content\":\"Think\",\"content\":\"Answer\",",
            "\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{",
            "\"name\":\"Read\",\"arguments\":\"{\\\"file_path\\\":\"}}]}}],",
            "\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":4,",
            "\"completion_tokens_details\":{\"reasoning_tokens\":2}}}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,",
            "\"function\":{\"arguments\":\"\\\"README.md\\\"}\"}}]},",
            "\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":7,",
            "\"completion_tokens\":9,\"completion_tokens_details\":{",
            "\"reasoning_tokens\":3}}}\n\n",
            "data: [DONE]\n\n"
        )),
        "qwen3.6".to_owned(),
        RequestUsageGuard::new(&usage, "qwen3.6"),
    )
    .collect::<Vec<_>>()
    .await
    .into_iter()
    .map(|event| format!("{:?}", event.expect("event should be infallible")))
    .collect::<Vec<_>>();

    assert_eq!(events.len(), 13, "{events:#?}");
    assert!(events[0].contains("event: message_start"), "{events:#?}");
    assert!(events[1].contains("event: content_block_start"));
    assert!(events[1].contains("\\\"type\\\":\\\"thinking\\\""));
    assert!(events[2].contains("\\\"type\\\":\\\"thinking_delta\\\""));
    assert!(events[3].contains("event: content_block_start"));
    assert!(events[3].contains("\\\"type\\\":\\\"text\\\""));
    assert!(events[4].contains("\\\"type\\\":\\\"text_delta\\\""));
    assert!(events[5].contains("\\\"type\\\":\\\"tool_use\\\""));
    assert!(events[6].contains("\\\"partial_json\\\":\\\"{\\\\\\\"file_path\\\\\\\":\\\""));
    assert!(events[7].contains("README.md"));
    for (event, index) in events[8..11].iter().zip(0..3) {
        assert!(event.contains("event: content_block_stop"));
        assert!(event.contains(&format!("\\\"index\\\":{index}")));
    }
    assert!(events[11].contains("event: message_delta"));
    assert!(events[11].contains("\\\"stop_reason\\\":\\\"tool_use\\\""));
    assert!(events[11].contains("\\\"output_tokens\\\":9"));
    assert!(events[12].contains("event: message_stop"));

    let recorded = snapshot(&usage);
    let model = recorded.models.get("qwen3.6").expect("model usage");
    assert_eq!(model.responses_with_usage, 1);
    assert_eq!(model.input_tokens, 7);
    assert_eq!(model.output_tokens, 9);
    assert_eq!(model.reasoning_tokens, 3);
}

#[tokio::test]
async fn reports_typed_upstream_error_before_processing_deltas() {
    let events = translate(
        response("data: {\"error\":{\"message\":\"typed boom\",\"type\":\"api_error\"}}\n\n"),
        "qwen3.6".to_owned(),
        usage_guard(),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(rendered.contains("event: error"), "{rendered}");
    assert!(
        rendered.contains("typed boom [NH-BRIDGE-105]"),
        "{rendered}"
    );
    assert!(!rendered.contains("event: message_start"), "{rendered}");
}

#[tokio::test]
async fn reports_fallback_upstream_error_before_processing_deltas() {
    let events = translate(
        response(
            "data: {\"error\":{\"message\":\"fallback boom\",\"type\":\"api_error\"},\"choices\":\"invalid\"}\n\n",
        ),
        "qwen3.6".to_owned(),
        usage_guard(),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(rendered.contains("event: error"), "{rendered}");
    assert!(
        rendered.contains("fallback boom [NH-BRIDGE-105]"),
        "{rendered}"
    );
    assert!(!rendered.contains("invalid streaming chunk"), "{rendered}");
    assert!(!rendered.contains("event: message_start"), "{rendered}");
}

#[tokio::test]
async fn reports_null_upstream_error_before_processing_deltas() {
    let events = translate(
        response("data: {\"error\":null}\n\n"),
        "qwen3.6".to_owned(),
        usage_guard(),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(rendered.contains("event: error"), "{rendered}");
    assert!(
        rendered.contains("NaN returned a streaming error [NH-BRIDGE-105]"),
        "{rendered}"
    );
    assert!(!rendered.contains("event: message_start"), "{rendered}");
}

#[tokio::test]
async fn preserves_invalid_streaming_json_error() {
    let events = translate(
        response("data: {not valid json}\n\n"),
        "qwen3.6".to_owned(),
        usage_guard(),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(rendered.contains("invalid streaming JSON:"), "{rendered}");
    assert!(rendered.contains("NH-BRIDGE-105"), "{rendered}");
    assert!(!rendered.contains("event: message_start"), "{rendered}");
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
            "data: {\"id\":\"msg_1\",\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
        ),
        "qwen3.6".to_owned(),
        usage_guard(),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(rendered.contains("event: error"), "{rendered}");
    assert!(rendered.contains("stream ended before the [DONE] marker"));
    assert!(!rendered.contains("event: message_stop"), "{rendered}");
}

#[tokio::test]
async fn rejects_truncated_tool_stream_even_with_valid_arguments() {
    let events = translate(
        response(
            "data: {\"id\":\"msg_1\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"Read\",\"arguments\":\"{}\"}}]}}]}\n\n",
        ),
        "qwen3.6".to_owned(),
        usage_guard(),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(rendered.contains("event: error"), "{rendered}");
    assert!(!rendered.contains("event: message_stop"), "{rendered}");
}

#[tokio::test]
async fn completes_stream_after_done_marker() {
    let events = translate(
        response(
            "data: {\"id\":\"msg_1\",\"choices\":[{\"delta\":{\"content\":\"complete\"}}]}\n\ndata: [DONE]\n\n",
        ),
        "qwen3.6".to_owned(),
        usage_guard(),
    )
    .collect::<Vec<_>>()
    .await;
    let rendered = format!("{events:?}");

    assert!(rendered.contains("event: message_stop"), "{rendered}");
    assert!(!rendered.contains("event: error"), "{rendered}");
}
