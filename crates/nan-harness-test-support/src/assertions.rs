use crate::scripted_provider::ScriptedToolCall;
use crate::terminal::TerminalOutput;
use serde_json::Value;
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Debug)]
pub struct ClaudeTranscript {
    events: Vec<Value>,
    source: String,
}

impl ClaudeTranscript {
    /// Parses Claude Code's newline-delimited `stream-json` output.
    ///
    /// # Errors
    ///
    /// Returns [`TranscriptError`] when a non-empty line is not valid JSON.
    pub fn parse(source: impl Into<String>) -> Result<Self, TranscriptError> {
        let source = source.into();
        let events = source
            .lines()
            .filter(|line| !line.trim().is_empty())
            .enumerate()
            .map(|(index, line)| {
                serde_json::from_str(line).map_err(|source| TranscriptError::InvalidEvent {
                    line: index + 1,
                    source,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { events, source })
    }

    #[must_use]
    pub fn tools(&self) -> BTreeSet<String> {
        self.events
            .iter()
            .find(|event| {
                event.get("type").and_then(Value::as_str) == Some("system")
                    && event.get("subtype").and_then(Value::as_str) == Some("init")
            })
            .and_then(|event| event.get("tools"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect()
    }

    /// Validates the complete Claude Code tool lifecycle for one scenario.
    ///
    /// # Errors
    ///
    /// Returns [`TranscriptError`] when the tool call, successful result, final marker, or clean
    /// result event is absent.
    pub fn require_complete_tool_round_trip(
        &self,
        tool_name: &str,
        final_marker: &str,
    ) -> Result<(), TranscriptError> {
        let (tool_use, tool_result, success, is_error) = self
            .lifecycle_indices(tool_name)
            .ok_or_else(|| TranscriptError::MissingToolUse(tool_name.to_owned()))?;
        if tool_result <= tool_use || success <= tool_result {
            return Err(TranscriptError::InvalidLifecycle(tool_name.to_owned()));
        }
        if is_error {
            return Err(TranscriptError::ToolError(tool_name.to_owned()));
        }
        if self.events[tool_result]
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|value| value == "tool_result")
            && value_contains_bool(&self.events[tool_result], "is_error", true)
        {
            return Err(TranscriptError::ToolError(tool_name.to_owned()));
        }
        if !self.source.contains(final_marker) {
            return Err(TranscriptError::MissingMarker(final_marker.to_owned()));
        }
        if !self.contains_pair("subtype", "success") {
            return Err(TranscriptError::MissingSuccess);
        }
        Ok(())
    }

    /// Validates a tool lifecycle that must stop at a documented external prerequisite.
    ///
    /// # Errors
    ///
    /// Returns an error when the tool call, error result, expected diagnostic, final marker, or
    /// clean Claude Code result is absent.
    pub fn require_expected_tool_error(
        &self,
        tool_name: &str,
        expected_error: &str,
        final_marker: &str,
    ) -> Result<(), TranscriptError> {
        let (tool_use, tool_result, success, is_error) = self
            .lifecycle_indices(tool_name)
            .ok_or_else(|| TranscriptError::MissingToolUse(tool_name.to_owned()))?;
        if tool_result <= tool_use || success <= tool_result {
            return Err(TranscriptError::InvalidLifecycle(tool_name.to_owned()));
        }
        if !is_error {
            return Err(TranscriptError::MissingExpectedToolError(
                tool_name.to_owned(),
            ));
        }
        if !value_contains_string(&self.events[tool_result], expected_error) {
            return Err(TranscriptError::MissingExpectedDiagnostic(
                expected_error.to_owned(),
            ));
        }
        if !self.source.contains(final_marker) {
            return Err(TranscriptError::MissingMarker(final_marker.to_owned()));
        }
        if !self.contains_pair("subtype", "success") {
            return Err(TranscriptError::MissingSuccess);
        }
        Ok(())
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    fn contains_pair(&self, key: &str, expected: &str) -> bool {
        self.events
            .iter()
            .any(|event| value_contains_pair(event, key, expected))
    }

    fn lifecycle_indices(&self, tool_name: &str) -> Option<(usize, usize, usize, bool)> {
        let tool_uses = self
            .events
            .iter()
            .enumerate()
            .flat_map(|(index, event)| {
                find_all_tool_uses(event)
                    .into_iter()
                    .map(move |(id, name)| (index, id, name))
            })
            .filter(|(_, _, name)| name == tool_name)
            .collect::<Vec<_>>();
        if tool_uses.len() != 1 {
            return None;
        }
        let (tool_use, tool_id, _) = &tool_uses[0];
        let tool_results = self
            .events
            .iter()
            .enumerate()
            .flat_map(|(index, event)| {
                (index > *tool_use)
                    .then(|| {
                        find_all_tool_results(event)
                            .into_iter()
                            .filter(|(actual_id, _)| actual_id == tool_id)
                            .map(move |(actual_id, is_error)| (index, actual_id, is_error))
                    })
                    .into_iter()
                    .flatten()
            })
            .collect::<Vec<_>>();
        if tool_results.len() != 1 {
            return None;
        }
        let (tool_result, _, is_error) = tool_results[0];
        let success = self.events.iter().enumerate().find_map(|(index, event)| {
            (index > tool_result && value_contains_pair(event, "subtype", "success"))
                .then_some(index)
        })?;
        Some((*tool_use, tool_result, success, is_error))
    }
}

/// Asserts the exact provider-side tool exchange emitted by a scripted scenario.
///
/// This is intentionally shared by the published canary and ignored native conformance tests so
/// a probe cannot accidentally become weaker in one of the two execution paths.
///
/// # Errors
///
/// Returns [`ProbeAssertionError`] when the request sequence, call IDs, names, arguments, or
/// returned tool results do not match the script exactly.
pub fn assert_tool_round_trip(
    output: &TerminalOutput,
    requests: &[Value],
    expected: &[ScriptedToolCall],
    final_marker: &str,
) -> Result<(), ProbeAssertionError> {
    if !output.status.success() {
        return Err(ProbeAssertionError::ProcessFailed);
    }
    if !output.stdout.contains(final_marker) {
        return Err(ProbeAssertionError::MissingMarker(final_marker.to_owned()));
    }
    assert_provider_tool_round_trip(requests, expected)
}

/// Asserts a no-tool sentinel exchange reached the provider and emitted no tool call/result.
///
/// # Errors
///
/// Returns [`ProbeAssertionError`] when the provider transcript contains no request, a tool
/// call/result, or malformed provider traffic.
pub fn assert_sentinel(
    output: &TerminalOutput,
    requests: &[Value],
    final_marker: &str,
) -> Result<(), ProbeAssertionError> {
    if !output.status.success() {
        return Err(ProbeAssertionError::ProcessFailed);
    }
    if !output.stdout.contains(final_marker) {
        return Err(ProbeAssertionError::MissingMarker(final_marker.to_owned()));
    }
    if requests.is_empty() {
        return Err(ProbeAssertionError::MissingProviderRequest);
    }
    if requests.iter().any(request_has_tool_traffic) {
        return Err(ProbeAssertionError::UnexpectedToolTraffic);
    }
    Ok(())
}

/// Asserts Aider's non-function edit protocol and the required file mutation.
///
/// # Errors
///
/// Returns [`ProbeAssertionError`] when Aider advertises function tools, misses the provider,
/// emits the marker, or fails to mutate the target file.
pub fn assert_aider_edit_protocol(
    output: &TerminalOutput,
    requests: &[Value],
    target: &std::path::Path,
    before: &str,
    after: &str,
) -> Result<(), ProbeAssertionError> {
    if !output.status.success() {
        return Err(ProbeAssertionError::ProcessFailed);
    }
    if !output.stdout.contains(after) {
        return Err(ProbeAssertionError::MissingMarker(after.to_owned()));
    }
    if requests.is_empty() {
        return Err(ProbeAssertionError::MissingProviderRequest);
    }
    if requests
        .iter()
        .any(|request| request.get("tools").is_some() || request_has_tool_calls(request))
    {
        return Err(ProbeAssertionError::UnexpectedFunctionTools);
    }
    let contents = std::fs::read_to_string(target)
        .map_err(|error| ProbeAssertionError::Filesystem(error.to_string()))?;
    if contents == before || !contents.contains(after) {
        return Err(ProbeAssertionError::MissingFilesystemSideEffect(
            target.to_owned(),
        ));
    }
    Ok(())
}

/// Asserts only the provider-side portion of a scripted round trip.
///
/// # Errors
///
/// Returns [`ProbeAssertionError`] when the script does not match exactly.
pub fn assert_provider_tool_round_trip(
    requests: &[Value],
    expected: &[ScriptedToolCall],
) -> Result<(), ProbeAssertionError> {
    if requests.is_empty() {
        return Err(ProbeAssertionError::MissingProviderRequest);
    }
    let calls = unique_tool_calls(requests);
    if calls.len() != expected.len() {
        return Err(ProbeAssertionError::UnexpectedToolCallCount {
            expected: expected.len(),
            actual: calls.len(),
        });
    }
    for (index, (actual_id, actual_name, actual_input)) in calls.iter().enumerate() {
        let expected_call = &expected[index];
        let expected_id = expected_tool_call_id(index);
        if actual_id != &expected_id {
            return Err(ProbeAssertionError::UnexpectedToolCallId {
                expected: expected_id,
                actual: actual_id.clone(),
            });
        }
        if actual_name != &expected_call.name {
            return Err(ProbeAssertionError::UnexpectedToolName {
                expected: expected_call.name.clone(),
                actual: actual_name.clone(),
            });
        }
        if actual_input != &expected_call.input {
            return Err(ProbeAssertionError::UnexpectedToolInput {
                expected: expected_call.input.clone(),
                actual: actual_input.clone(),
            });
        }
    }

    let results = unique_tool_results(requests);
    let expected_result_ids = expected
        .iter()
        .enumerate()
        .filter(|(_, call)| call.result_expected)
        .map(|(index, _)| expected_tool_call_id(index))
        .collect::<BTreeSet<_>>();
    let actual_result_ids = results
        .iter()
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>();
    if actual_result_ids != expected_result_ids || results.len() != expected_result_ids.len() {
        return Err(ProbeAssertionError::UnexpectedToolResults {
            expected: expected_result_ids,
            actual: actual_result_ids,
        });
    }
    for (_, content) in results {
        if content.is_null() || content.as_str().is_some_and(str::is_empty) {
            return Err(ProbeAssertionError::EmptyToolResult);
        }
        if value_is_error(&content) {
            return Err(ProbeAssertionError::ToolResultError);
        }
    }
    Ok(())
}

/// Asserts result IDs and result health for an existing scripted call list. This lighter helper
/// is useful for the long ignored native scenarios, whose inputs intentionally contain dynamic
/// result-ID placeholders while still sharing the same result/cleanup truth as the canary.
///
/// # Errors
///
/// Returns [`ProbeAssertionError`] when a scripted result is missing, unexpected, duplicated, or
/// failed outside the explicitly allowed tool names.
pub fn assert_tool_results(
    requests: &[Value],
    expected: &[ScriptedToolCall],
    allowed_errors: &[&str],
) -> Result<(), ProbeAssertionError> {
    if requests.is_empty() {
        return Err(ProbeAssertionError::MissingProviderRequest);
    }
    let results = unique_tool_results(requests);
    let expected_ids = expected
        .iter()
        .enumerate()
        .filter(|(_, call)| call.result_expected)
        .map(|(index, _)| normalized_tool_result_id(&expected_tool_call_id(index)))
        .collect::<BTreeSet<_>>();
    let actual_ids = results
        .iter()
        .map(|(id, _)| normalized_tool_result_id(id))
        .collect::<BTreeSet<_>>();
    if expected_ids != actual_ids || expected_ids.len() != results.len() {
        return Err(ProbeAssertionError::UnexpectedToolResults {
            expected: expected_ids,
            actual: actual_ids,
        });
    }
    for (index, call) in expected.iter().enumerate() {
        if !call.result_expected {
            continue;
        }
        let result = results
            .iter()
            .find(|(id, _)| {
                normalized_tool_result_id(id)
                    == normalized_tool_result_id(&expected_tool_call_id(index))
            })
            .map(|(_, content)| content)
            .ok_or(ProbeAssertionError::EmptyToolResult)?;
        if result.is_null() || result.as_str().is_some_and(str::is_empty) {
            return Err(ProbeAssertionError::EmptyToolResult);
        }
        if value_is_error(result) && !allowed_errors.contains(&call.name.as_str()) {
            return Err(ProbeAssertionError::ToolResultError);
        }
    }
    Ok(())
}

#[must_use]
pub fn expected_tool_call_id(index: usize) -> String {
    format!("call_nan_harness_conformance_{index}")
}

fn normalized_tool_result_id(identifier: &str) -> String {
    identifier
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

fn extract_tool_calls(request: &Value) -> Vec<(String, String, Value)> {
    request
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("assistant"))
        .flat_map(|message| {
            message
                .get("tool_calls")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|call| {
                    let id = call.get("id")?.as_str()?.to_owned();
                    let function = call.get("function")?;
                    let name = function.get("name")?.as_str()?.to_owned();
                    let arguments = function.get("arguments")?;
                    let input = arguments.as_str().map_or_else(
                        || Some(arguments.clone()),
                        |arguments| serde_json::from_str(arguments).ok(),
                    )?;
                    Some((id, name, input))
                })
        })
        .collect()
}

fn unique_tool_calls(requests: &[Value]) -> Vec<(String, String, Value)> {
    let mut calls = Vec::new();
    for call in requests.iter().flat_map(extract_tool_calls) {
        if calls.iter().any(|existing| existing == &call) {
            continue;
        }
        calls.push(call);
    }
    calls
}

fn extract_tool_results(request: &Value) -> Vec<(String, Value)> {
    request
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("tool"))
        .filter_map(|message| {
            Some((
                message.get("tool_call_id")?.as_str()?.to_owned(),
                message.get("content")?.clone(),
            ))
        })
        .collect()
}

fn unique_tool_results(requests: &[Value]) -> Vec<(String, Value)> {
    let mut results = Vec::new();
    for result in requests.iter().flat_map(extract_tool_results) {
        if results.iter().any(|existing| existing == &result) {
            continue;
        }
        results.push(result);
    }
    results
}

fn request_has_tool_traffic(request: &Value) -> bool {
    request_has_tool_calls(request)
        || request
            .get("messages")
            .and_then(Value::as_array)
            .is_some_and(|messages| {
                messages
                    .iter()
                    .any(|message| message.get("role").and_then(Value::as_str) == Some("tool"))
            })
}

fn request_has_tool_calls(request: &Value) -> bool {
    request
        .get("messages")
        .and_then(Value::as_array)
        .is_some_and(|messages| {
            messages.iter().any(|message| {
                message
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .is_some_and(|calls| !calls.is_empty())
            })
        })
}

fn value_is_error(value: &Value) -> bool {
    let text = value
        .as_str()
        .map(str::trim_start)
        .unwrap_or_default()
        .to_ascii_lowercase();
    text.starts_with("error")
        || text.starts_with("<system>error:")
        || value.get("isError").and_then(Value::as_bool) == Some(true)
        || value
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| matches!(status, "error" | "failed"))
        || value.get("error").is_some_and(|error| !error.is_null())
}

#[derive(Debug, Error)]
pub enum ProbeAssertionError {
    #[error("harness process did not exit successfully")]
    ProcessFailed,
    #[error("provider did not receive a chat request")]
    MissingProviderRequest,
    #[error("provider emitted unexpected tool traffic")]
    UnexpectedToolTraffic,
    #[error("provider emitted {actual} scripted calls; expected {expected}")]
    UnexpectedToolCallCount { expected: usize, actual: usize },
    #[error("provider emitted tool call id '{actual}', expected '{expected}'")]
    UnexpectedToolCallId { expected: String, actual: String },
    #[error("provider emitted tool '{actual}', expected '{expected}'")]
    UnexpectedToolName { expected: String, actual: String },
    #[error("provider emitted unexpected tool input")]
    UnexpectedToolInput { expected: Value, actual: Value },
    #[error("provider returned tool result identifiers {actual:?}; expected {expected:?}")]
    UnexpectedToolResults {
        expected: BTreeSet<String>,
        actual: BTreeSet<String>,
    },
    #[error("provider returned an empty tool result")]
    EmptyToolResult,
    #[error("provider returned a tool error")]
    ToolResultError,
    #[error("harness did not emit marker '{0}'")]
    MissingMarker(String),
    #[error("harness did not produce the required filesystem side effect at '{0}'")]
    MissingFilesystemSideEffect(std::path::PathBuf),
    #[error("harness emitted unexpected function tools")]
    UnexpectedFunctionTools,
    #[error("filesystem assertion failed: {0}")]
    Filesystem(String),
}

#[cfg(test)]
mod tests {
    use super::{
        ClaudeTranscript, ProbeAssertionError, assert_provider_tool_round_trip, assert_sentinel,
        assert_tool_results, assert_tool_round_trip,
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
}

fn value_contains_pair(value: &Value, key: &str, expected: &str) -> bool {
    match value {
        Value::Object(object) => {
            object.get(key).and_then(Value::as_str) == Some(expected)
                || object
                    .values()
                    .any(|value| value_contains_pair(value, key, expected))
        }
        Value::Array(values) => values
            .iter()
            .any(|value| value_contains_pair(value, key, expected)),
        _ => false,
    }
}

fn find_all_tool_uses(value: &Value) -> Vec<(String, String)> {
    let mut matches = Vec::new();
    collect_all_tool_uses(value, &mut matches);
    matches
}

fn collect_all_tool_uses(value: &Value, matches: &mut Vec<(String, String)>) {
    match value {
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("tool_use")
                && let (Some(id), Some(name)) = (
                    object.get("id").and_then(Value::as_str),
                    object.get("name").and_then(Value::as_str),
                )
            {
                matches.push((id.to_owned(), name.to_owned()));
            }
            for value in object.values() {
                collect_all_tool_uses(value, matches);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_all_tool_uses(value, matches);
            }
        }
        _ => {}
    }
}

fn find_all_tool_results(value: &Value) -> Vec<(String, bool)> {
    let mut matches = Vec::new();
    collect_all_tool_results(value, &mut matches);
    matches
}

fn collect_all_tool_results(value: &Value, matches: &mut Vec<(String, bool)>) {
    match value {
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("tool_result")
                && let Some(id) = ["tool_use_id", "tool_call_id"]
                    .into_iter()
                    .find_map(|key| object.get(key).and_then(Value::as_str))
            {
                matches.push((
                    id.to_owned(),
                    object
                        .get("is_error")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                ));
            }
            for value in object.values() {
                collect_all_tool_results(value, matches);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_all_tool_results(value, matches);
            }
        }
        _ => {}
    }
}

fn value_contains_string(value: &Value, expected: &str) -> bool {
    match value {
        Value::String(text) => text.contains(expected),
        Value::Array(values) => values
            .iter()
            .any(|value| value_contains_string(value, expected)),
        Value::Object(object) => object
            .values()
            .any(|value| value_contains_string(value, expected)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn value_contains_bool(value: &Value, key: &str, expected: bool) -> bool {
    match value {
        Value::Object(object) => {
            object.get(key).and_then(Value::as_bool) == Some(expected)
                || object
                    .values()
                    .any(|value| value_contains_bool(value, key, expected))
        }
        Value::Array(values) => values
            .iter()
            .any(|value| value_contains_bool(value, key, expected)),
        _ => false,
    }
}

#[derive(Debug, Error)]
pub enum TranscriptError {
    #[error("Claude stream event on line {line} is not valid JSON: {source}")]
    InvalidEvent {
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("Claude did not emit a tool_use event for '{0}'")]
    MissingToolUse(String),
    #[error("Claude did not emit a tool_result event for '{0}'")]
    MissingToolResult(String),
    #[error("Claude reported an error while executing '{0}'")]
    ToolError(String),
    #[error("Claude did not report the expected external prerequisite error for '{0}'")]
    MissingExpectedToolError(String),
    #[error("Claude did not emit the expected diagnostic '{0}'")]
    MissingExpectedDiagnostic(String),
    #[error("Claude did not emit the final marker '{0}'")]
    MissingMarker(String),
    #[error("Claude did not finish with a successful result event")]
    MissingSuccess,
    #[error("Claude emitted an out-of-order tool lifecycle for '{0}'")]
    InvalidLifecycle(String),
}
