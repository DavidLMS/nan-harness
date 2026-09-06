use super::{SearchOutcome, TOOL_USE_ID, error_code, json_response, streaming_response};
use crate::error::{ApiError, UpstreamTimeoutPhase};
use axum::response::Response;
use futures_util::StreamExt;
use reqwest::StatusCode;
use serde_json::{Value, json};

const MODEL: &str = "synthetic-anthropic-model";
const QUERY: &str = "synthetic protocol query";

struct SyntheticFailure {
    context: &'static str,
    error: ApiError,
    expected_code: &'static str,
    private_error_message: &'static str,
}

fn synthetic_failures() -> Vec<SyntheticFailure> {
    vec![
        SyntheticFailure {
            context: "upstream HTTP 429",
            error: ApiError::UpstreamStatus {
                status: StatusCode::TOO_MANY_REQUESTS,
                message: "synthetic private upstream rate-limit message".to_owned(),
            },
            expected_code: "too_many_requests",
            private_error_message: "synthetic private upstream rate-limit message",
        },
        SyntheticFailure {
            context: "representative upstream HTTP 4xx",
            error: ApiError::UpstreamStatus {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                message: "synthetic private upstream 4xx message".to_owned(),
            },
            expected_code: "invalid_tool_input",
            private_error_message: "synthetic private upstream 4xx message",
        },
        SyntheticFailure {
            context: "local invalid request",
            error: ApiError::InvalidRequest("synthetic private invalid-request message".to_owned()),
            expected_code: "invalid_tool_input",
            private_error_message: "synthetic private invalid-request message",
        },
        SyntheticFailure {
            context: "upstream HTTP 503",
            error: ApiError::UpstreamStatus {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: "synthetic private upstream 503 message".to_owned(),
            },
            expected_code: "unavailable",
            private_error_message: "synthetic private upstream 503 message",
        },
        SyntheticFailure {
            context: "upstream initial-response timeout",
            error: ApiError::UpstreamTimeout(UpstreamTimeoutPhase::InitialResponse),
            expected_code: "unavailable",
            private_error_message: "",
        },
        SyntheticFailure {
            context: "upstream inactivity timeout",
            error: ApiError::UpstreamTimeout(UpstreamTimeoutPhase::Inactivity),
            expected_code: "unavailable",
            private_error_message: "",
        },
    ]
}

#[test]
fn classifies_search_failure_variants_into_protocol_codes() {
    for failure in synthetic_failures() {
        assert_eq!(
            error_code(&failure.error),
            failure.expected_code,
            "{}",
            failure.context
        );
    }
}

#[tokio::test]
async fn json_error_envelope_hides_private_failure_details() {
    for failure in synthetic_failures() {
        let outcome = SearchOutcome::Error(error_code(&failure.error));
        let response = json_response(QUERY, &outcome, MODEL);

        assert_eq!(response.status(), StatusCode::OK);
        let encoded = response_bytes(response).await;
        let actual: Value =
            serde_json::from_slice(&encoded).expect("JSON response should be valid UTF-8 JSON");

        assert_eq!(
            actual,
            json!({
                "id": "msg_nan_web_search",
                "type": "message",
                "role": "assistant",
                "model": MODEL,
                "content": [
                    {
                        "type": "server_tool_use",
                        "id": TOOL_USE_ID,
                        "name": "web_search",
                        "input": {"query": QUERY},
                    },
                    {
                        "type": "web_search_tool_result",
                        "tool_use_id": TOOL_USE_ID,
                        "content": {
                            "type": "web_search_tool_result_error",
                            "error_code": failure.expected_code,
                        },
                    },
                ],
                "stop_reason": "end_turn",
                "stop_sequence": null,
                "usage": {
                    "input_tokens": 0,
                    "output_tokens": 0,
                    "server_tool_use": {"web_search_requests": 0},
                },
            }),
            "{}",
            failure.context
        );
        assert_private_message_is_absent(&encoded, &failure);
    }
}

#[tokio::test]
async fn streaming_error_events_hides_private_failure_details() {
    for failure in synthetic_failures() {
        let outcome = SearchOutcome::Error(error_code(&failure.error));
        let response = streaming_response(QUERY, &outcome, MODEL);

        assert_eq!(response.status(), StatusCode::OK);
        let encoded = response_bytes(response).await;
        let encoded = String::from_utf8(encoded).expect("SSE response should be valid UTF-8");
        assert_eq!(
            parse_sse_events(&encoded),
            expected_error_events(failure.expected_code),
            "{}",
            failure.context
        );
        assert_private_message_is_absent(encoded.as_bytes(), &failure);
    }
}

fn assert_private_message_is_absent(encoded: &[u8], failure: &SyntheticFailure) {
    if failure.private_error_message.is_empty() {
        return;
    }

    let text = String::from_utf8(encoded.to_vec()).expect("response should be valid UTF-8");
    assert!(
        !text.contains(failure.private_error_message),
        "{} leaked its synthetic private message",
        failure.context
    );
}

async fn response_bytes(response: Response) -> Vec<u8> {
    let mut encoded = Vec::new();
    let mut body = response.into_body().into_data_stream();
    while let Some(chunk) = body.next().await {
        encoded.extend_from_slice(&chunk.expect("finite response body chunk"));
    }
    encoded
}

fn parse_sse_events(encoded: &str) -> Vec<(&str, Value)> {
    encoded
        .split("\n\n")
        .filter(|frame| !frame.is_empty())
        .map(|frame| {
            let mut event = None;
            let mut data = Vec::new();
            for line in frame.lines() {
                if let Some(event_name) = line.strip_prefix("event: ") {
                    event = Some(event_name);
                } else if let Some(event_data) = line.strip_prefix("data: ") {
                    data.push(event_data);
                } else {
                    panic!("unexpected SSE line: {line:?}");
                }
            }

            let event = event.expect("SSE frame should name its event");
            let payload =
                serde_json::from_str(&data.join("\n")).expect("SSE data should be valid JSON");
            (event, payload)
        })
        .collect()
}

fn expected_error_events(failure_code: &'static str) -> Vec<(&'static str, Value)> {
    vec![
        (
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": "msg_nan_web_search",
                    "type": "message",
                    "role": "assistant",
                    "model": MODEL,
                    "content": [],
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": {"input_tokens": 0, "output_tokens": 0},
                },
            }),
        ),
        (
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {
                    "type": "server_tool_use",
                    "id": TOOL_USE_ID,
                    "name": "web_search",
                    "input": {},
                },
            }),
        ),
        (
            "content_block_delta",
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": json!({"query": QUERY}).to_string(),
                },
            }),
        ),
        (
            "content_block_stop",
            json!({"type": "content_block_stop", "index": 0}),
        ),
        (
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": 1,
                "content_block": {
                    "type": "web_search_tool_result",
                    "tool_use_id": TOOL_USE_ID,
                    "content": {
                        "type": "web_search_tool_result_error",
                        "error_code": failure_code,
                    },
                },
            }),
        ),
        (
            "content_block_stop",
            json!({"type": "content_block_stop", "index": 1}),
        ),
        (
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn", "stop_sequence": null},
                "usage": {
                    "input_tokens": 0,
                    "output_tokens": 0,
                    "server_tool_use": {"web_search_requests": 0},
                },
            }),
        ),
        ("message_stop", json!({"type": "message_stop"})),
    ]
}
