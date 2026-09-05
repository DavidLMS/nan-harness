use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::sync::Arc;

use super::state::{
    CONFORMANCE_TOOL_CALL_ID_PREFIX, ProviderState, STRUCTURED_HELPER_TOOL_CALL_ID,
};

pub(super) async fn models(State(state): State<Arc<ProviderState>>) -> Json<Value> {
    state.record_model_request();
    Json(json!({
        "object": "list",
        "data": [
            {"id": "qwen3.6", "object": "model"},
            {"id": "deepseek-v4-flash", "object": "model"},
            {"id": "mimo-v2.5", "object": "model"},
            {"id": "gemma4", "object": "model"}
        ]
    }))
}

pub(super) async fn chat_completions(
    State(state): State<Arc<ProviderState>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    state.record_chat_request(&body);

    let mut progress = state.progress();
    if progress.emitted {
        if let Some(content) = tool_result(&body, &tool_call_id(progress.index)) {
            progress
                .result_identifiers
                .push(result_identifier(&content).unwrap_or_default());
            progress.index += 1;
            progress.emitted = false;
        } else if state
            .scenario()
            .tool_calls
            .get(progress.index)
            .is_some_and(|tool_call| !tool_call.result_expected)
        {
            progress.result_identifiers.push(String::new());
            progress.index += 1;
            progress.emitted = false;
        }
    }
    let response = match state.scenario().tool_calls.get(progress.index) {
        Some(_) if progress.emitted && exposes_tool(&body, "structured_output") => tool_response(
            STRUCTURED_HELPER_TOOL_CALL_ID,
            "structured_output",
            &json!({
                "status": "complete",
                "summary": "Deterministic conformance worker completed.",
                "evidence": ["The scripted provider returned the required report."],
                "nextSteps": [],
                "blocker": ""
            }),
        ),
        Some(_) if progress.emitted => text_response("CONFORMANCE_HELPER_OK"),
        Some(tool_call) if exposes_tool(&body, &tool_call.name) => {
            progress.emitted = true;
            let mut input = tool_call.input.clone();
            expand_fixture_url(&mut input, state.fixture_url());
            expand_result_identifiers(&mut input, &progress.result_identifiers);
            tool_response(&tool_call_id(progress.index), &tool_call.name, &input)
        }
        None => {
            progress.completed = true;
            text_response(&state.scenario().final_marker)
        }
        Some(_) => text_response("CONFORMANCE_HELPER_OK"),
    };
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        response,
    )
}

pub(super) async fn search(
    State(state): State<Arc<ProviderState>>,
    Json(body): Json<Value>,
) -> Json<Value> {
    state.record_search_request(body);
    Json(json!({
        "results": [{
            "title": "nan-harness conformance fixture",
            "url": "https://example.test/nan-harness-conformance",
            "snippet": "A deterministic result returned by the local scripted provider."
        }]
    }))
}

pub(super) async fn fixture() -> &'static str {
    "NAN_HARNESS_WEB_FETCH_FIXTURE"
}

fn expand_fixture_url(value: &mut Value, fixture_url: &str) {
    match value {
        Value::String(text) => *text = text.replace("{{fixture_url}}", fixture_url),
        Value::Array(values) => {
            for value in values {
                expand_fixture_url(value, fixture_url);
            }
        }
        Value::Object(object) => {
            for value in object.values_mut() {
                expand_fixture_url(value, fixture_url);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn expand_result_identifiers(value: &mut Value, identifiers: &[String]) {
    match value {
        Value::String(text) => {
            for (index, identifier) in identifiers.iter().enumerate() {
                *text = text.replace(&format!("{{{{result_id:{index}}}}}"), identifier);
            }
        }
        Value::Array(values) => {
            for value in values {
                expand_result_identifiers(value, identifiers);
            }
        }
        Value::Object(object) => {
            for value in object.values_mut() {
                expand_result_identifiers(value, identifiers);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn exposes_tool(body: &Value, tool_name: &str) -> bool {
    body.get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| {
            tools.iter().any(|tool| {
                tool.pointer("/function/name").and_then(Value::as_str) == Some(tool_name)
            })
        })
}

fn tool_result(body: &Value, tool_call_id: &str) -> Option<String> {
    body.get("messages")
        .and_then(Value::as_array)
        .and_then(|messages| {
            messages.iter().find_map(|message| {
                let matches = message.get("role").and_then(Value::as_str) == Some("tool")
                    && message
                        .get("tool_call_id")
                        .and_then(Value::as_str)
                        .is_some_and(|actual| tool_call_ids_match(actual, tool_call_id));
                matches
                    .then(|| message.get("content").map(message_content))
                    .flatten()
            })
        })
}

fn tool_call_ids_match(left: &str, right: &str) -> bool {
    left.chars()
        .filter(char::is_ascii_alphanumeric)
        .eq(right.chars().filter(char::is_ascii_alphanumeric))
}

fn message_content(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_owned();
    }
    content.as_array().map_or_else(
        || content.to_string(),
        |blocks| {
            blocks
                .iter()
                .map(|block| {
                    block
                        .get("text")
                        .and_then(Value::as_str)
                        .map_or_else(|| block.to_string(), ToOwned::to_owned)
                })
                .collect::<Vec<_>>()
                .join("\n")
        },
    )
}

fn result_identifier(content: &str) -> Option<String> {
    [
        "with ID: ",
        "Scheduled one-shot task ",
        "Scheduled recurring task ",
        "started background job ",
        "started background subagent job ",
        "started subagent ",
        "task_id: ",
        "id: ",
        "Plan file: ",
    ]
    .into_iter()
    .find_map(|prefix| {
        content
            .split_once(prefix)
            .and_then(|(_, suffix)| suffix.split_whitespace().next())
            .map(|value| value.trim_end_matches('.').to_owned())
    })
    .or_else(|| {
        content
            .split_once("Artifact ID: ")
            .and_then(|(_, suffix)| suffix.lines().next())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
    .or_else(|| {
        let value: Value = serde_json::from_str(content).ok()?;
        [
            "/goal/id",
            "/subagentId",
            "/jobId",
            "/taskId",
            "/runId",
            "/outcomeId",
            "/fragmentId",
            "/childSessionKey",
            "/sessionKey",
            "/sessions/0/key",
            "/sessions/0/sessionKey",
            "/agentId",
            "/id",
        ]
        .into_iter()
        .find_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
        .map(ToOwned::to_owned)
    })
}

fn tool_call_id(index: usize) -> String {
    format!("{CONFORMANCE_TOOL_CALL_ID_PREFIX}_{index}")
}

fn tool_response(tool_call_id: &str, tool_name: &str, input: &Value) -> String {
    let chunk = json!({
        "id": "chatcmpl_nan_harness_conformance",
        "model": "qwen3.6",
        "choices": [{
            "index": 0,
            "delta": {
                "tool_calls": [{
                    "index": 0,
                    "id": tool_call_id,
                    "function": {
                        "name": tool_name,
                        "arguments": input.to_string()
                    }
                }]
            }
        }]
    });
    let stop = json!({
        "id": "chatcmpl_nan_harness_conformance",
        "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]
    });
    let usage = json!({
        "id": "chatcmpl_nan_harness_conformance",
        "choices": [],
        "usage": {"prompt_tokens": 20, "completion_tokens": 10}
    });
    sse(&[chunk, stop, usage])
}

fn text_response(text: &str) -> String {
    let chunk = json!({
        "id": "chatcmpl_nan_harness_conformance",
        "model": "qwen3.6",
        "choices": [{"index": 0, "delta": {"content": text}}]
    });
    let stop = json!({
        "id": "chatcmpl_nan_harness_conformance",
        "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
    });
    let usage = json!({
        "id": "chatcmpl_nan_harness_conformance",
        "choices": [],
        "usage": {"prompt_tokens": 10, "completion_tokens": 4}
    });
    sse(&[chunk, stop, usage])
}

fn sse(chunks: &[Value]) -> String {
    let mut body = String::new();
    for chunk in chunks {
        writeln!(&mut body, "data: {chunk}\n").expect("writing to a String cannot fail");
    }
    body.push_str("data: [DONE]\n\n");
    body
}

#[cfg(test)]
mod tests {
    use super::{result_identifier, sse};

    #[test]
    fn sse_preserves_event_boundaries_and_the_done_sentinel() {
        let chunks = [serde_json::json!({"id": 1}), serde_json::json!({"id": 2})];
        assert_eq!(
            sse(&chunks),
            "data: {\"id\":1}\n\ndata: {\"id\":2}\n\ndata: [DONE]\n\n"
        );
        assert_eq!(sse(&[]), "data: [DONE]\n\n");
    }

    #[test]
    fn extracts_identifiers_from_native_tool_messages() {
        for (content, expected) in [
            ("started background job job-42", "job-42"),
            ("started background subagent job child-7", "child-7"),
            ("started subagent agent-3", "agent-3"),
        ] {
            assert_eq!(result_identifier(content).as_deref(), Some(expected));
        }
    }

    #[test]
    fn extracts_identifiers_from_structured_tool_results() {
        for (content, expected) in [
            (r#"{"goal":{"id":"goal-42"}}"#, "goal-42"),
            (r#"{"subagentId":"child-7"}"#, "child-7"),
            (r#"{"jobId":"job-9"}"#, "job-9"),
            (r#"{"agentId":"agent-3"}"#, "agent-3"),
        ] {
            assert_eq!(result_identifier(content).as_deref(), Some(expected));
        }
    }
}
