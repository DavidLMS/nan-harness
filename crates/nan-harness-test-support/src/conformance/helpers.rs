use super::constants::MAX_DURATION_MILLISECONDS;
use super::report::{ConformanceCheck, ConformanceScenario, ConformanceStatus};
use crate::manifest::Expectation;
use crate::scripted_provider::ScriptedToolCall;
use crate::terminal::TerminalOutput;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

/// Builds a scripted tool call for a deterministic conformance scenario.
#[must_use]
pub fn call(name: &str, input: Value) -> ScriptedToolCall {
    ScriptedToolCall {
        name: name.to_owned(),
        input,
        result_expected: true,
    }
}

/// Finds a tool result in provider requests, accepting punctuation differences in its ID.
///
/// Text blocks in provider-native content arrays are joined with newlines so callers can apply
/// the same assertions to string and structured content responses.
#[must_use]
pub fn tool_result(requests: &[Value], tool_call_id: &str) -> Option<String> {
    requests.iter().find_map(|request| {
        request
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| {
                messages.iter().find_map(|message| {
                    let matches = message.get("role").and_then(Value::as_str) == Some("tool")
                        && message
                            .get("tool_call_id")
                            .and_then(Value::as_str)
                            .is_some_and(|actual| tool_call_ids_match(actual, tool_call_id));
                    matches.then(|| {
                        message
                            .get("content")
                            .map_or_else(|| message.to_string(), message_content)
                    })
                })
            })
    })
}

/// Reports whether a serialized tool result represents an error.
///
/// Both quoted and unquoted textual error results are accepted, alongside the structured error
/// shapes emitted by the supported provider protocols.
#[must_use]
pub fn tool_result_failed(result: &str) -> bool {
    let normalized = result.trim_matches('"').trim_start().to_ascii_lowercase();
    if normalized.starts_with("error") || normalized.starts_with("<system>error:") {
        return true;
    }
    let Ok(value) = serde_json::from_str::<Value>(result) else {
        return false;
    };
    value.get("isError").and_then(Value::as_bool) == Some(true)
        || value
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| matches!(status, "error" | "failed"))
        || value.get("error").is_some_and(|error| !error.is_null())
}

/// Writes a text fixture beneath a conformance workspace.
///
/// # Panics
///
/// Panics if the fixture path has no parent or its directory or contents cannot be written.
pub fn write_fixture(workspace: &Path, relative_path: &str, content: &str) {
    let path = workspace.join(relative_path);
    fs::create_dir_all(path.parent().expect("fixture should have a parent"))
        .expect("fixture directory should exist");
    fs::write(path, content).expect("fixture should be written");
}

/// Asserts that a conformance fixture exists and contains the expected text.
///
/// # Panics
///
/// Panics if the fixture cannot be read or does not contain `expected`.
pub fn assert_file(workspace: &Path, relative_path: &str, expected: &str) {
    let path = workspace.join(relative_path);
    let content = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "expected conformance file '{}' should exist: {error}",
            path.display()
        )
    });
    assert!(content.contains(expected), "file content was {content:?}");
}

/// Extracts tool names from one provider request, accepting `OpenAI` and native tool shapes.
#[must_use]
pub fn tool_names(request: &Value) -> Option<BTreeSet<String>> {
    request
        .get("tools")?
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| {
                    tool.pointer("/function/name")
                        .or_else(|| tool.get("name"))
                        .and_then(Value::as_str)
                })
                .map(ToOwned::to_owned)
                .collect()
        })
        .filter(|tools: &BTreeSet<String>| !tools.is_empty())
}

/// Asserts an exact native tool inventory.
///
/// # Panics
///
/// Panics if `actual` does not exactly match `expected`.
pub fn assert_inventory(actual: &BTreeSet<String>, expected: &[&str]) {
    let expected = expected
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, &expected);
}

/// Asserts that a harness completed successfully without emitting a diagnostic.
///
/// # Panics
///
/// Panics if the harness failed or its standard output contains an `NH-` diagnostic.
pub fn assert_success(output: &TerminalOutput) {
    assert!(output.status.success(), "{}", output.diagnostic());
    assert!(!output.stdout.contains("NH-"), "{}", output.diagnostic());
}

fn tool_call_ids_match(left: &str, right: &str) -> bool {
    left.chars()
        .filter(char::is_ascii_alphanumeric)
        .eq(right.chars().filter(char::is_ascii_alphanumeric))
}

fn message_content(content: &Value) -> String {
    content.as_str().map_or_else(
        || {
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
        },
        ToOwned::to_owned,
    )
}

pub(crate) fn verify_expectation(expectation: &Expectation) -> Result<(), String> {
    match expectation {
        Expectation::None => Ok(()),
        Expectation::FileContains { path, text } => {
            let contents = fs::read_to_string(path).map_err(|error| error.to_string())?;
            contents
                .contains(text)
                .then_some(())
                .ok_or_else(|| "file expectation was not met".to_owned())
        }
        Expectation::FileMissing { path } if !Path::new(path).exists() => Ok(()),
        Expectation::FileMissing { .. } => Err("file expected to be absent exists".to_owned()),
    }
}

pub(crate) fn scenario(
    name: &str,
    status: ConformanceStatus,
    started: Instant,
) -> ConformanceScenario {
    let duration = duration_milliseconds(started.elapsed());
    ConformanceScenario {
        name: name.to_owned(),
        status,
        checks: vec![ConformanceCheck {
            name: "contract".to_owned(),
            status,
            duration_milliseconds: duration,
        }],
        duration_milliseconds: duration,
    }
}

pub(crate) fn failed_scenario(name: &str, started: Instant) -> ConformanceScenario {
    scenario(name, ConformanceStatus::Failed, started)
}

pub(crate) fn duration_milliseconds(duration: Duration) -> u64 {
    duration
        .as_millis()
        .try_into()
        .unwrap_or(MAX_DURATION_MILLISECONDS)
        .min(MAX_DURATION_MILLISECONDS)
}
