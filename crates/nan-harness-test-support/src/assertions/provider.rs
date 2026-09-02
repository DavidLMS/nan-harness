use super::errors::ProbeAssertionError;
use super::extraction::{
    normalized_tool_result_id, unique_tool_calls, unique_tool_results, value_is_error,
};
use crate::scripted_provider::ScriptedToolCall;
use crate::terminal::TerminalOutput;
use serde_json::Value;
use std::collections::BTreeSet;

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

/// Asserts a strict provider exchange while allowing call and result IDs to lose punctuation.
///
/// # Errors
///
/// Returns [`ProbeAssertionError`] when the process, marker, call sequence, normalized IDs, or
/// result health does not match the script.
pub fn assert_tool_round_trip_with_sanitized_ids(
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
    assert_provider_tool_calls_with_sanitized_ids(requests, expected)?;
    assert_tool_results(requests, expected, &[])
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
    assert_provider_tool_calls(requests, expected)?;

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
    assert_result_health(results)
}

fn assert_provider_tool_calls(
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

    Ok(())
}

fn assert_provider_tool_calls_with_sanitized_ids(
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
        if normalized_tool_result_id(actual_id) != normalized_tool_result_id(&expected_id) {
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
    Ok(())
}

fn assert_result_health(results: Vec<(String, Value)>) -> Result<(), ProbeAssertionError> {
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
