use super::constants::MAX_DURATION_MILLISECONDS;
use super::report::{ConformanceCheck, ConformanceScenario, ConformanceStatus};
use crate::manifest::Expectation;
use crate::scripted_provider::ScriptedToolCall;
use crate::terminal::TerminalOutput;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const PROGRESS_ENV: &str = "NAN_HARNESS_CONFORMANCE_PROGRESS";

#[derive(serde::Serialize)]
struct ProgressEvent<'a> {
    schema_version: u8,
    scenario: &'a str,
    stage: &'a str,
    status: &'a str,
    elapsed_milliseconds: u64,
}

static PROGRESS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Appends one safe conformance progress event when explicitly enabled.
pub(crate) fn progress_event(scenario: &str, stage: &str, status: &str, started: Instant) {
    let Some(path) = std::env::var_os(PROGRESS_ENV).map(std::path::PathBuf::from) else {
        return;
    };
    write_progress_event(&path, scenario, stage, status, started);
}

fn write_progress_event(path: &Path, scenario: &str, stage: &str, status: &str, started: Instant) {
    let event = ProgressEvent {
        schema_version: 1,
        scenario,
        stage,
        status,
        elapsed_milliseconds: duration_milliseconds(started.elapsed()),
    };
    let Ok(encoded) = serde_json::to_vec(&event) else {
        return;
    };
    let lock = PROGRESS_LOCK.get_or_init(|| Mutex::new(()));
    let Ok(_guard) = lock.lock() else {
        return;
    };
    let Ok(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_owned)
        .ok_or(())
    else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let mut line = encoded;
    line.push(b'\n');
    let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let _ = file.write_all(&line).and_then(|()| file.sync_data());
}

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

#[cfg(test)]
mod progress_tests {
    use super::write_progress_event;
    use serde_json::Value;
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn progress_is_durable_before_pending_await_and_after_cancellation() {
        let workspace = tempfile::tempdir().expect("workspace should exist");
        let path = workspace.path().join("progress.jsonl");
        let started = Instant::now();
        write_progress_event(&path, "inventory", "process", "started", started);
        let pending = tokio::time::timeout(Duration::from_millis(10), async {
            std::future::pending::<()>().await;
        })
        .await;
        assert!(pending.is_err());
        write_progress_event(&path, "inventory", "process", "failed", started);
        let lines = std::fs::read_to_string(&path).expect("progress should be durable");
        let events = lines
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["status"], "started");
        assert_eq!(events[1]["status"], "failed");
        for event in &events {
            assert_eq!(event.as_object().expect("event object").len(), 5);
            assert_eq!(event["schemaVersion"], Value::Null);
            assert_eq!(event["schema_version"], 1);
            assert!(event["elapsed_milliseconds"].as_u64().is_some());
        }
        let encoded = lines.to_ascii_lowercase();
        assert!(!encoded.contains("path"));
        assert!(!encoded.contains("output"));
        assert!(!encoded.contains("secret"));
    }

    #[test]
    fn progress_writer_ignores_missing_or_unwritable_paths() {
        let workspace = tempfile::tempdir().expect("workspace should exist");
        let missing_parent = workspace.path().join("missing/progress.jsonl");
        write_progress_event(
            &missing_parent,
            "sentinel",
            "scenario",
            "started",
            Instant::now(),
        );
        assert!(missing_parent.is_file());
        let unwritable = workspace.path().join("directory");
        std::fs::create_dir(&unwritable).expect("directory should exist");
        write_progress_event(
            &unwritable,
            "sentinel",
            "scenario",
            "started",
            Instant::now(),
        );
    }
}
