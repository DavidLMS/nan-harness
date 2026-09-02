use super::{
    ClaudeTranscript, ProbeAssertionError, assert_provider_tool_round_trip, assert_sentinel,
    assert_tool_results, assert_tool_round_trip, assert_tool_round_trip_with_sanitized_ids,
};
use crate::scripted_provider::ScriptedToolCall;
use crate::terminal::TerminalOutput;
use serde_json::json;
use std::process::Command;

#[test]
fn strict_provider_probe_requires_exact_call_and_result() {
    let expected = ScriptedToolCall {
        name: "write_file".to_owned(),
        input: json!({"path": "tool-output.txt", "content": "ok"}),
        result_expected: true,
    };
    let requests = vec![
        json!({
            "messages": [{
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_nan_harness_conformance_0",
                    "function": {
                        "name": "write_file",
                        "arguments": "{\"path\":\"tool-output.txt\",\"content\":\"ok\"}"
                    }
                }]
            }]
        }),
        json!({
            "messages": [{
                "role": "tool",
                "tool_call_id": "call_nan_harness_conformance_0",
                "content": "written"
            }]
        }),
    ];
    assert_provider_tool_round_trip(&requests, std::slice::from_ref(&expected))
        .expect("the exact scripted exchange should pass");
    let mut wrong = requests;
    wrong[0]["messages"][0]["tool_calls"][0]["function"]["name"] = json!("unexpected");
    assert!(matches!(
        assert_provider_tool_round_trip(&wrong, std::slice::from_ref(&expected)),
        Err(ProbeAssertionError::UnexpectedToolName { .. })
    ));
}

#[test]
fn strict_provider_probe_collapses_replayed_conversation_history() {
    let expected = ScriptedToolCall {
        name: "write_file".to_owned(),
        input: json!({"path": "tool-output.txt", "content": "ok"}),
        result_expected: true,
    };
    let call = json!({
        "role": "assistant",
        "tool_calls": [{
            "id": "call_nan_harness_conformance_0",
            "function": {
                "name": "write_file",
                "arguments": "{\"path\":\"tool-output.txt\",\"content\":\"ok\"}"
            }
        }]
    });
    let result = json!({
        "role": "tool",
        "tool_call_id": "call_nan_harness_conformance_0",
        "content": "written"
    });
    let requests = vec![
        json!({"messages": [call.clone(), result.clone()]}),
        json!({"messages": [{"role": "user", "content": "continue"}, call, result]}),
    ];
    assert_provider_tool_round_trip(&requests, std::slice::from_ref(&expected))
        .expect("replayed history should represent one logical exchange");
}

#[test]
fn native_result_probe_accepts_sanitized_result_identifiers() {
    let expected = ScriptedToolCall {
        name: "read".to_owned(),
        input: json!({"path": "fixture.txt"}),
        result_expected: true,
    };
    let requests = vec![json!({
        "messages": [{
            "role": "tool",
            "tool_call_id": "callnanharnessconformance0",
            "content": "fixture"
        }]
    })];
    assert_tool_results(&requests, &[expected], &[])
        .expect("native harnesses may remove punctuation from tool result identifiers");
}

#[test]
fn strict_call_probe_accepts_sanitized_call_and_result_identifiers() {
    let expected = ScriptedToolCall {
        name: "write".to_owned(),
        input: json!({"path": "fixture.txt", "content": "ok"}),
        result_expected: true,
    };
    let requests = vec![
        json!({
            "messages": [{
                "role": "assistant",
                "tool_calls": [{
                    "id": "callnanharnessconformance0",
                    "function": {
                        "name": "write",
                        "arguments": "{\"path\":\"fixture.txt\",\"content\":\"ok\"}"
                    }
                }]
            }]
        }),
        json!({
            "messages": [{
                "role": "tool",
                "tool_call_id": "callnanharnessconformance0",
                "content": "written"
            }]
        }),
    ];
    let output = TerminalOutput {
        status: Command::new("true").status().expect("true should run"),
        stdout: "marker".to_owned(),
        stderr: String::new(),
    };
    assert_tool_round_trip_with_sanitized_ids(&output, &requests, &[expected], "marker")
        .expect("the exact exchange with sanitized identifiers should pass");
}

#[test]
fn sentinel_rejects_tool_traffic() {
    let output = TerminalOutput {
        status: Command::new("true").status().expect("true should run"),
        stdout: "NAN_HARNESS_CONFORMANCE_SENTINEL_OK".to_owned(),
        stderr: String::new(),
    };
    let requests = vec![json!({
        "messages": [{
            "role": "assistant",
            "tool_calls": [{"id": "unexpected", "function": {"name": "read", "arguments": "{}"}}]
        }]
    })];
    assert!(matches!(
        assert_sentinel(&output, &requests, "NAN_HARNESS_CONFORMANCE_SENTINEL_OK"),
        Err(ProbeAssertionError::UnexpectedToolTraffic)
    ));
}

#[test]
fn round_trip_requires_process_success_and_final_marker() {
    let expected = ScriptedToolCall {
        name: "read".to_owned(),
        input: json!({"path": "read-target.txt"}),
        result_expected: true,
    };
    let output = TerminalOutput {
        status: Command::new("true").status().expect("true should run"),
        stdout: String::new(),
        stderr: String::new(),
    };
    assert!(matches!(
        assert_tool_round_trip(&output, &[], &[expected], "marker"),
        Err(ProbeAssertionError::MissingMarker(_))
    ));
}

#[test]
fn claude_transcript_requires_the_matching_nested_tool_lifecycle() {
    let source = [
        json!({
            "type": "assistant",
            "message": {"content": [{
                "type": "tool_use",
                "id": "toolu_design_sync",
                "name": "DesignSync",
                "input": {"method": "list_projects"}
            }]}
        }),
        json!({
            "type": "user",
            "message": {"content": [{
                "type": "tool_result",
                "tool_use_id": "toolu_design_sync",
                "is_error": true,
                "content": "DesignSync needs design-system authorization"
            }]}
        }),
        json!({
            "type": "result",
            "subtype": "success",
            "result": "DESIGN_SYNC_CONFORMANCE_OK"
        }),
    ]
    .into_iter()
    .map(|event| event.to_string())
    .collect::<Vec<_>>()
    .join("\n");
    let transcript = ClaudeTranscript::parse(source).expect("events should parse");
    transcript
        .require_expected_tool_error(
            "DesignSync",
            "DesignSync needs design-system authorization",
            "DESIGN_SYNC_CONFORMANCE_OK",
        )
        .expect("the exact nested lifecycle should pass");
}

#[test]
fn claude_transcript_allows_prerequisite_tool_lifecycles() {
    let source = [
        json!({
            "type": "assistant",
            "message": {"content": [{
                "type": "tool_use",
                "id": "toolu_read",
                "name": "Read",
                "input": {"file_path": "fixture.txt"}
            }]}
        }),
        json!({
            "type": "user",
            "message": {"content": [{
                "type": "tool_result",
                "tool_use_id": "toolu_read",
                "content": "before"
            }]}
        }),
        json!({
            "type": "assistant",
            "message": {"content": [{
                "type": "tool_use",
                "id": "toolu_edit",
                "name": "Edit",
                "input": {"file_path": "fixture.txt"}
            }]}
        }),
        json!({
            "type": "user",
            "message": {"content": [{
                "type": "tool_result",
                "tool_use_id": "toolu_edit",
                "content": "updated"
            }]}
        }),
        json!({
            "type": "result",
            "subtype": "success",
            "result": "EDIT_CONFORMANCE_OK"
        }),
    ]
    .into_iter()
    .map(|event| event.to_string())
    .collect::<Vec<_>>()
    .join("\n");
    let transcript = ClaudeTranscript::parse(source).expect("events should parse");
    transcript
        .require_complete_tool_round_trip("Edit", "EDIT_CONFORMANCE_OK")
        .expect("the target lifecycle should ignore completed prerequisite tools");
}
