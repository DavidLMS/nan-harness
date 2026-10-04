//! Payload-free observations of the single scripted file-tool result.

use serde::Serialize;
use serde_json::Value;

const EXPECTED_CALL: &str = "call_nan_harness_conformance_0";
const MAX_MESSAGES: usize = 4096;
const MAX_RESULTS: usize = 32;
const MAX_TEXT_BYTES: usize = 65_536;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SelectedTool {
    Read,
    ReadFile,
    ReadFiles,
    ExecCommand,
    FixtureRead,
}

impl SelectedTool {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "Read" => Some(Self::Read),
            "read_file" => Some(Self::ReadFile),
            "read_files" => Some(Self::ReadFiles),
            "exec_command" => Some(Self::ExecCommand),
            "mcp__nanh-read-fixture__read_file" => Some(Self::FixtureRead),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Shape {
    Absent,
    String,
    TextArray,
    Mixed,
    Unsupported,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ErrorCategory {
    None,
    FileNotFound,
    FileTooLarge,
    ReadBudget,
    Directory,
    Unknown,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ErrorEnvelope {
    SingleXmlReadWrapper,
    SingleXmlOther,
    PlainReadWrapper,
    PlainOther,
    MultipleOrIncompleteXml,
    MixedFragments,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ExecResult {
    LaunchFailed,
    ExitedZero,
    ExitedNonzero,
    Running,
    Unknown,
    Ambiguous,
}

// Advisory lexical hints only: tool output is untrusted and grants no authority.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
enum FailureHint {
    PermissionDenied,
    MissingFile,
    InvalidPath,
    Sandbox,
    MissingCommand,
    Unsupported,
    Unknown,
    Ambiguous,
}

fn failure_hint(texts: &[&str]) -> FailureHint {
    let mut found = FailureHint::Unknown;
    for text in texts {
        let normalized = text.to_ascii_lowercase();
        for (hint, signals) in [
            (
                FailureHint::PermissionDenied,
                &["permission denied", "access is denied", "eacces:", "eperm:"][..],
            ),
            (
                FailureHint::MissingFile,
                &[
                    "no such file or directory",
                    "file does not exist",
                    "cannot find the file",
                    "cannot find the path",
                    "enoent:",
                ][..],
            ),
            (
                FailureHint::InvalidPath,
                &[
                    "invalid path",
                    "path is not absolute",
                    "invalid filename",
                    "filename, directory name, or volume label syntax is incorrect",
                ][..],
            ),
            (
                FailureHint::Sandbox,
                &["sandbox", "bwrap:", "landlock", "seccomp"][..],
            ),
            (
                FailureHint::MissingCommand,
                &[
                    "command not found",
                    "is not recognized as an internal or external command",
                ][..],
            ),
            (
                FailureHint::Unsupported,
                &["not supported", "unsupported operation", "not implemented"][..],
            ),
        ] {
            if signals.iter().any(|signal| normalized.contains(signal)) {
                if found != FailureHint::Unknown && found != hint {
                    return FailureHint::Ambiguous;
                }
                found = hint;
            }
        }
    }
    found
}

// Parse only the bounded source envelope, never output text or a substring.
fn exec_result(text: &[&str]) -> ExecResult {
    let [text] = text else {
        return ExecResult::Unknown;
    };
    if text.len() > MAX_TEXT_BYTES {
        return ExecResult::Unknown;
    }
    let text = text.strip_prefix("Tool error: ").unwrap_or(text);
    if text.starts_with("Failed to create unified exec process: ") {
        return ExecResult::LaunchFailed;
    }
    let mut lines = text.lines().peekable();
    if !exec_prefix(&mut lines) {
        return ExecResult::Unknown;
    }
    if !lines
        .next()
        .and_then(|line| line.strip_prefix("Wall time: "))
        .and_then(|value| value.strip_suffix(" seconds"))
        .is_some_and(wall_time_decimal)
    {
        return ExecResult::Unknown;
    }
    let state = match lines.next() {
        Some(line) if line.starts_with("Process exited with code ") => {
            match line
                .strip_prefix("Process exited with code ")
                .and_then(|value| value.parse::<i32>().ok())
            {
                Some(0) => ExecResult::ExitedZero,
                Some(_) => ExecResult::ExitedNonzero,
                None => return ExecResult::Unknown,
            }
        }
        Some(line) if line.starts_with("Process running with session ID ") => {
            if !line
                .strip_prefix("Process running with session ID ")
                .is_some_and(unsigned_decimal)
            {
                return ExecResult::Unknown;
            }
            ExecResult::Running
        }
        _ => return ExecResult::Unknown,
    };
    if lines
        .peek()
        .is_some_and(|line| line.starts_with("Original token count: "))
        && !lines
            .next()
            .and_then(|line| line.strip_prefix("Original token count: "))
            .is_some_and(unsigned_decimal)
    {
        return ExecResult::Unknown;
    }
    if lines.next() != Some("Output:") {
        return ExecResult::Unknown;
    }
    state
}

fn exec_prefix(lines: &mut std::iter::Peekable<std::str::Lines<'_>>) -> bool {
    if lines
        .peek()
        .is_some_and(|line| line.starts_with("Warning: truncated output (original token count: "))
    {
        let Some(line) = lines.next() else {
            return false;
        };
        if !line
            .strip_prefix("Warning: truncated output (original token count: ")
            .and_then(|value| value.strip_suffix(')'))
            .is_some_and(unsigned_decimal)
        {
            return false;
        }
    }
    if lines
        .peek()
        .is_some_and(|line| line.starts_with("Total output lines: "))
        && (!lines
            .next()
            .and_then(|line| line.strip_prefix("Total output lines: "))
            .is_some_and(unsigned_decimal)
            || lines.next() != Some(""))
    {
        return false;
    }
    if lines
        .peek()
        .is_some_and(|line| line.starts_with("Chunk ID: "))
        && !lines
            .next()
            .and_then(|line| line.strip_prefix("Chunk ID: "))
            .is_some_and(|value| {
                !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_whitespace)
            })
    {
        return false;
    }
    true
}

fn wall_time_decimal(value: &str) -> bool {
    let value = value.strip_prefix('-').unwrap_or(value);
    let mut parts = value.split('.');
    parts.next().is_some_and(unsigned_decimal)
        && parts.next().is_none_or(unsigned_decimal)
        && parts.next().is_none()
}

fn unsigned_decimal(value: &str) -> bool {
    !value.is_empty() && value.len() <= 20 && value.bytes().all(|byte| byte.is_ascii_digit())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolResultObservation {
    selected_tool: SelectedTool,
    result_present: bool,
    result_count: usize,
    status: &'static str,
    shape: Shape,
    tool_error_detected: bool,
    error_category: ErrorCategory,
    error_envelope: Option<ErrorEnvelope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exec_result: Option<ExecResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_hint: Option<FailureHint>,
}

impl ToolResultObservation {
    pub(crate) fn collect(requests: &[Value], selected_tool: SelectedTool) -> Self {
        let mut result = Self {
            selected_tool,
            result_present: false,
            result_count: 0,
            status: "complete",
            shape: Shape::Absent,
            tool_error_detected: false,
            error_category: ErrorCategory::None,
            error_envelope: None,
            exec_result: None,
            failure_hint: None,
        };
        let mut seen = Vec::new();
        let mut inspected = 0;
        for request in requests {
            let Some(messages) = request.get("messages").and_then(Value::as_array) else {
                continue;
            };
            for message in messages {
                inspected += 1;
                if inspected > MAX_MESSAGES {
                    result.status = "limit";
                    return result;
                }
                if message.get("role").and_then(Value::as_str) != Some("tool")
                    || message.get("tool_call_id").and_then(Value::as_str) != Some(EXPECTED_CALL)
                {
                    continue;
                }
                let content = message.get("content").unwrap_or(&Value::Null);
                let Some((shape, error, text)) = classify(content, selected_tool) else {
                    result.status = "limit";
                    result.shape = if result.result_present {
                        Shape::Mixed
                    } else {
                        Shape::Absent
                    };
                    return result;
                };
                if matches!(shape, Shape::String | Shape::TextArray)
                    && seen.contains(&(shape, text.clone()))
                {
                    continue;
                }
                if seen.len() == MAX_RESULTS {
                    result.status = "limit";
                    return result;
                }
                let envelope = error.map(|_| observed_error_envelope(shape, &text, selected_tool));
                if matches!(selected_tool, SelectedTool::ExecCommand) {
                    result.exec_result = Some(if result.result_present {
                        ExecResult::Ambiguous
                    } else {
                        exec_result(&text)
                    });
                }
                if result.result_present && result.failure_hint.is_some() {
                    result.failure_hint = Some(FailureHint::Ambiguous);
                } else if error.is_some()
                    || matches!(
                        result.exec_result,
                        Some(ExecResult::LaunchFailed | ExecResult::ExitedNonzero)
                    )
                {
                    result.failure_hint = Some(if result.result_present {
                        FailureHint::Ambiguous
                    } else {
                        failure_hint(&text)
                    });
                }
                seen.push((shape, text));
                result.result_present = true;
                result.result_count += 1;
                result.shape = if result.result_count == 1 {
                    shape
                } else {
                    Shape::Mixed
                };
                if let Some(category) = error {
                    result.error_category =
                        if result.tool_error_detected && result.error_category != category {
                            ErrorCategory::Unknown
                        } else {
                            category
                        };
                    result.error_envelope = match (result.error_envelope, envelope) {
                        (Some(prior), Some(current)) if prior != current => {
                            Some(ErrorEnvelope::MixedFragments)
                        }
                        (_, current) => current,
                    };
                    result.tool_error_detected = true;
                }
            }
        }
        result
    }
}

fn classify(
    content: &Value,
    selected_tool: SelectedTool,
) -> Option<(Shape, Option<ErrorCategory>, Vec<&str>)> {
    if let Some(text) = content.as_str() {
        if text.len() > MAX_TEXT_BYTES {
            return None;
        }
        return Some((
            Shape::String,
            text.strip_prefix("Tool error: ")
                .map(|text| selected_error_category(text, selected_tool)),
            vec![text],
        ));
    }
    let Some(parts) = content.as_array() else {
        return Some((Shape::Unsupported, None, Vec::new()));
    };
    if parts.len() > MAX_RESULTS {
        return None;
    }
    let mut bytes = 0_usize;
    let mut all_text = true;
    let mut texts = Vec::new();
    for part in parts {
        match (
            part.get("type").and_then(Value::as_str),
            part.get("text").and_then(Value::as_str),
        ) {
            (Some("text"), Some(text)) => {
                bytes = bytes.checked_add(text.len())?;
                texts.push(text);
            }
            _ => all_text = false,
        }
        if bytes > MAX_TEXT_BYTES {
            return None;
        }
    }
    let error = parts.first().is_some_and(|part| {
        part.get("type").and_then(Value::as_str) == Some("text")
            && part.get("text").and_then(Value::as_str) == Some("Tool error")
    });
    let category = error.then(|| {
        let mut category = ErrorCategory::Unknown;
        for part in parts.iter().skip(1) {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                let found = selected_error_category(text, selected_tool);
                if found != ErrorCategory::Unknown {
                    if category != ErrorCategory::Unknown && category != found {
                        return ErrorCategory::Unknown;
                    }
                    category = found;
                }
            }
        }
        category
    });
    Some((
        if all_text {
            Shape::TextArray
        } else {
            Shape::Mixed
        },
        category,
        texts,
    ))
}

fn observed_error_envelope(shape: Shape, texts: &[&str], selected: SelectedTool) -> ErrorEnvelope {
    let text = match (shape, texts) {
        (Shape::String, [text]) => text.strip_prefix("Tool error: ").unwrap_or(text),
        (Shape::TextArray, ["Tool error", text]) => text,
        _ => return ErrorEnvelope::MixedFragments,
    }
    .trim();
    let read_wrapper = |body: &str| {
        matches!(selected, SelectedTool::Read)
            && body
                .strip_prefix("Error calling tool (Read): ")
                .is_some_and(|inner| !inner.starts_with("Error calling tool (Read): "))
    };
    if text.contains("<tool_use_error>") || text.contains("</tool_use_error>") {
        let Some(body) = text
            .strip_prefix("<tool_use_error>")
            .and_then(|body| body.strip_suffix("</tool_use_error>"))
        else {
            return ErrorEnvelope::MultipleOrIncompleteXml;
        };
        if body.contains("<tool_use_error>") || body.contains("</tool_use_error>") {
            return ErrorEnvelope::MultipleOrIncompleteXml;
        }
        return if read_wrapper(body.trim()) {
            ErrorEnvelope::SingleXmlReadWrapper
        } else {
            ErrorEnvelope::SingleXmlOther
        };
    }
    if read_wrapper(text) {
        ErrorEnvelope::PlainReadWrapper
    } else {
        ErrorEnvelope::PlainOther
    }
}

fn selected_error_category(text: &str, selected_tool: SelectedTool) -> ErrorCategory {
    if matches!(selected_tool, SelectedTool::Read)
        && let Some(body) = text
            .trim()
            .strip_prefix("<tool_use_error>")
            .and_then(|body| body.strip_suffix("</tool_use_error>"))
        && !body.contains("<tool_use_error>")
        && !body.contains("</tool_use_error>")
        && let Some(inner) = body.trim().strip_prefix("Error calling tool (Read): ")
    {
        // The pinned SDK wraps thrown Read errors once inside this envelope.
        return error_category(inner);
    }
    error_category(text)
}

fn error_category(text: &str) -> ErrorCategory {
    // Claude's transcript renderer recognizes this tool-error envelope. Only
    // one complete envelope may expose the same fixed error prefixes below.
    let text = if let Some(body) = text
        .trim()
        .strip_prefix("<tool_use_error>")
        .and_then(|body| body.strip_suffix("</tool_use_error>"))
    {
        if body.contains("<tool_use_error>") || body.contains("</tool_use_error>") {
            return ErrorCategory::Unknown;
        }
        body.trim()
    } else {
        text
    };
    if text.starts_with("File does not exist.") {
        ErrorCategory::FileNotFound
    } else if text.starts_with("FileTooLargeError:") {
        ErrorCategory::FileTooLarge
    } else if text.starts_with("MaxFileReadTokenExceededError:") {
        ErrorCategory::ReadBudget
    } else if text.starts_with("EISDIR:") {
        ErrorCategory::Directory
    } else {
        ErrorCategory::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(role: &str, content: Value) -> Value {
        let mut value = json!({"messages":[{"role":role,"tool_call_id":EXPECTED_CALL}]});
        value["messages"][0]["content"] = content;
        value
    }

    #[test]
    fn failure_hints_are_closed_advisory_and_ambiguous_when_conflicting() {
        assert_eq!(
            failure_hint(&["cat: /PRIVATE: Permission denied"]),
            FailureHint::PermissionDenied
        );
        assert_eq!(failure_hint(&["ENOENT: PRIVATE"]), FailureHint::MissingFile);
        assert_eq!(
            failure_hint(&["bwrap: PRIVATE: Permission denied"]),
            FailureHint::Ambiguous
        );
        assert_eq!(
            failure_hint(&["unclassified PRIVATE"]),
            FailureHint::Unknown
        );
        assert_eq!(
            failure_hint(&["invalid path PRIVATE"]),
            FailureHint::InvalidPath
        );
        let requests = vec![serde_json::json!({"messages":[{
            "role":"tool", "tool_call_id":EXPECTED_CALL,
            "content":"Tool error: EACCES: PRIVATE"
        }]})];
        let observed = ToolResultObservation::collect(&requests, SelectedTool::Read);
        assert_eq!(observed.failure_hint, Some(FailureHint::PermissionDenied));
        assert_eq!(observed.error_category, ErrorCategory::Unknown);
        let encoded = serde_json::to_string(&observed).unwrap();
        assert!(!encoded.contains("PRIVATE"));
        assert!(!encoded.contains("EACCES"));
    }

    #[test]
    fn owned_fixture_tool_name_is_exact_and_not_a_general_mcp_alias() {
        assert!(matches!(
            SelectedTool::from_name("mcp__nanh-read-fixture__read_file"),
            Some(SelectedTool::FixtureRead)
        ));
        for name in [
            "nanh-read-fixture__read_file",
            "mcp__other__read_file",
            "MCP__nanh-read-fixture__read_file",
        ] {
            assert!(SelectedTool::from_name(name).is_none());
        }
    }

    #[test]
    fn complete_tool_error_envelope_exposes_only_fixed_categories() {
        assert_eq!(
            error_category(
                "<tool_use_error>\nFile does not exist. private path\n</tool_use_error>"
            ),
            ErrorCategory::FileNotFound
        );
        for body in [
            "<tool_use_error>File does not exist. private",
            "<tool_use_error>File does not exist.</tool_use_error> extra",
            "<tool_use_error>File does not exist.</tool_use_error><tool_use_error>private</tool_use_error>",
            "<other>File does not exist.</other>",
        ] {
            assert_eq!(error_category(body), ErrorCategory::Unknown);
        }
        let value = ToolResultObservation::collect(
            &[request(
                "tool",
                json!(
                    "Tool error: <tool_use_error>File does not exist. private path</tool_use_error>"
                ),
            )],
            SelectedTool::Read,
        );
        let encoded = serde_json::to_string(&value).unwrap();
        assert!(encoded.contains("file-not-found"));
        assert!(!encoded.contains("private path"));
        assert!(!encoded.contains("tool_use_error"));
    }

    #[test]
    fn selected_read_wrapper_preserves_fixed_categories_without_exposing_content() {
        for (prefix, expected) in [
            ("File does not exist.", ErrorCategory::FileNotFound),
            ("FileTooLargeError:", ErrorCategory::FileTooLarge),
            ("MaxFileReadTokenExceededError:", ErrorCategory::ReadBudget),
            ("EISDIR:", ErrorCategory::Directory),
        ] {
            let body = format!(
                "<tool_use_error>Error calling tool (Read): {prefix} private</tool_use_error>"
            );
            for content in [
                json!(format!("Tool error: {body}")),
                json!([{"type":"text","text":"Tool error"}, {"type":"text","text":body}]),
            ] {
                let facts =
                    ToolResultObservation::collect(&[request("tool", content)], SelectedTool::Read);
                assert_eq!(facts.error_category, expected);
                let encoded = serde_json::to_string(&facts).unwrap();
                assert!(!encoded.contains("private"));
                assert!(!encoded.contains("Error calling tool"));
            }
        }
    }

    #[test]
    fn wrapper_requires_selected_read_complete_single_envelope_and_exact_spelling() {
        let valid = "<tool_use_error>Error calling tool (Read): File does not exist. private</tool_use_error>";
        for selected in [
            SelectedTool::ReadFile,
            SelectedTool::ReadFiles,
            SelectedTool::ExecCommand,
        ] {
            let facts = ToolResultObservation::collect(
                &[request("tool", json!(format!("Tool error: {valid}")))],
                selected,
            );
            assert_eq!(facts.error_category, ErrorCategory::Unknown);
        }
        for body in [
            "Error calling tool (Read): File does not exist.",
            "<tool_use_error>Error calling tool (Read): File does not exist.",
            "<tool_use_error>Error calling tool (read): File does not exist.</tool_use_error>",
            "<tool_use_error>Error calling tool (read_file): File does not exist.</tool_use_error>",
            "<tool_use_error>Error calling tool (Read): Error calling tool (Read): File does not exist.</tool_use_error>",
            "<tool_use_error>Error calling tool (Read): <tool_use_error>File does not exist.</tool_use_error></tool_use_error>",
            "<tool_use_error>Error calling tool (Read): unknown private</tool_use_error>",
        ] {
            assert_eq!(
                selected_error_category(body, SelectedTool::Read),
                ErrorCategory::Unknown
            );
        }
        for role in ["assistant", "user"] {
            let facts = ToolResultObservation::collect(
                &[request(role, json!(format!("Tool error: {valid}")))],
                SelectedTool::Read,
            );
            assert!(!facts.result_present);
        }
        let facts =
            ToolResultObservation::collect(&[request("tool", json!(valid))], SelectedTool::Read);
        assert!(!facts.tool_error_detected);
    }

    #[test]
    fn error_envelope_formats_are_closed_without_changing_categories() {
        for (body, expected) in [
            (
                "<tool_use_error>Error calling tool (Read): private unknown</tool_use_error>",
                ErrorEnvelope::SingleXmlReadWrapper,
            ),
            (
                "<tool_use_error>private unknown</tool_use_error>",
                ErrorEnvelope::SingleXmlOther,
            ),
            (
                "Error calling tool (Read): private unknown",
                ErrorEnvelope::PlainReadWrapper,
            ),
            ("private unknown", ErrorEnvelope::PlainOther),
            (
                "<tool_use_error>private unknown",
                ErrorEnvelope::MultipleOrIncompleteXml,
            ),
            (
                "<tool_use_error><tool_use_error>private</tool_use_error></tool_use_error>",
                ErrorEnvelope::MultipleOrIncompleteXml,
            ),
        ] {
            let facts = ToolResultObservation::collect(
                &[request("tool", json!(format!("Tool error: {body}")))],
                SelectedTool::Read,
            );
            assert_eq!(facts.error_envelope, Some(expected));
            assert_eq!(facts.error_category, ErrorCategory::Unknown);
            assert!(!serde_json::to_string(&facts).unwrap().contains("private"));
        }
        let array = request(
            "tool",
            json!([
                {"type":"text","text":"Tool error"},
                {"type":"text","text":"<tool_use_error>private"},
                {"type":"text","text":"</tool_use_error>"}
            ]),
        );
        let facts = ToolResultObservation::collect(&[array], SelectedTool::Read);
        assert_eq!(facts.error_envelope, Some(ErrorEnvelope::MixedFragments));
        let facts = ToolResultObservation::collect(
            &[request("tool", json!("plain result"))],
            SelectedTool::Read,
        );
        assert_eq!(facts.error_envelope, None);
        let facts = ToolResultObservation::collect(
            &[request("assistant", json!("Tool error: private"))],
            SelectedTool::Read,
        );
        assert_eq!(facts.error_envelope, None);
        let facts = ToolResultObservation::collect(
            &[request(
                "tool",
                json!(
                    "Tool error: <tool_use_error>Error calling tool (Read): private</tool_use_error>"
                ),
            )],
            SelectedTool::ReadFile,
        );
        assert_eq!(facts.error_envelope, Some(ErrorEnvelope::SingleXmlOther));
    }

    #[test]
    fn only_exact_tool_result_and_bridge_error_prefix_are_evidence() {
        let spoof = request(
            "assistant",
            json!("Tool error: File does not exist. private"),
        );
        let plain = request("tool", json!("File does not exist. private"));
        let facts = ToolResultObservation::collect(&[spoof, plain], SelectedTool::Read);
        assert_eq!(facts.result_count, 1);
        assert!(!facts.tool_error_detected);
        let error = request("tool", json!("Tool error: File does not exist. private"));
        let facts = ToolResultObservation::collect(&[error.clone(), error], SelectedTool::Read);
        assert_eq!(facts.result_count, 1);
        assert_eq!(facts.error_category, ErrorCategory::FileNotFound);
        assert!(!serde_json::to_string(&facts).unwrap().contains("private"));
    }

    #[test]
    fn arrays_conflicts_and_limits_keep_observation_incomplete() {
        let error = request(
            "tool",
            json!([
                {"type":"text","text":"Tool error"},
                {"type":"text","text":"EISDIR: private"},
                {"type":"image_url","image_url":{"url":"private"}}
            ]),
        );
        let facts = ToolResultObservation::collect(&[error], SelectedTool::ReadFile);
        assert_eq!(facts.shape, Shape::Mixed);
        assert_eq!(facts.error_category, ErrorCategory::Directory);
        let facts = ToolResultObservation::collect(
            &[
                request("tool", json!("first")),
                request("tool", json!("second")),
            ],
            SelectedTool::ReadFiles,
        );
        assert_eq!(facts.result_count, 2);
        assert_eq!(facts.shape, Shape::Mixed);
        let facts = ToolResultObservation::collect(
            &[request(
                "tool",
                json!(vec![json!({"type":"text","text":"x"}); 33]),
            )],
            SelectedTool::ExecCommand,
        );
        assert_eq!(facts.status, "limit");
        assert_eq!(facts.shape, Shape::Absent);
    }

    #[test]
    fn conflicting_result_limit_and_selected_names_are_closed() {
        let requests: Vec<_> = (0..33)
            .map(|index| request("tool", json!(format!("result-{index}"))))
            .collect();
        let facts = ToolResultObservation::collect(&requests, SelectedTool::Read);
        assert_eq!(facts.result_count, 32);
        assert_eq!(facts.shape, Shape::Mixed);
        assert_eq!(facts.status, "limit");
        for (name, expected) in [
            ("Read", "read"),
            ("read_file", "read-file"),
            ("read_files", "read-files"),
            ("exec_command", "exec-command"),
        ] {
            let selected = SelectedTool::from_name(name).unwrap();
            assert_eq!(serde_json::to_value(selected).unwrap(), expected);
        }
        assert!(SelectedTool::from_name("private-tool").is_none());
        let facts = ToolResultObservation::collect(
            &[
                request("tool", json!("Tool error: File does not exist. private")),
                request("tool", json!("Tool error: EISDIR: private")),
            ],
            SelectedTool::Read,
        );
        assert_eq!(facts.error_category, ErrorCategory::Unknown);
    }

    #[test]
    fn exec_envelopes_are_closed_and_never_inspect_output_for_status() {
        for (text, expected) in [
            (
                "Wall time: 0.1 seconds\nProcess exited with code 0\nOutput:\nprivate",
                ExecResult::ExitedZero,
            ),
            (
                "Chunk ID: private\nWall time: 1 seconds\nProcess exited with code -1\nOriginal token count: 3\nOutput:\nprivate",
                ExecResult::ExitedNonzero,
            ),
            (
                "Wall time: 0 seconds\nProcess running with session ID 123\nOutput:\nprivate",
                ExecResult::Running,
            ),
            (
                "Failed to create unified exec process: private",
                ExecResult::LaunchFailed,
            ),
            (
                "Tool error: Failed to create unified exec process: private",
                ExecResult::LaunchFailed,
            ),
            (
                "private\nProcess exited with code 0\nOutput:",
                ExecResult::Unknown,
            ),
            (
                "Wall time: NaN seconds\nProcess exited with code 0\nOutput:",
                ExecResult::Unknown,
            ),
            (
                "Wall time: 1 seconds\nProcess exited with code 0",
                ExecResult::Unknown,
            ),
            (
                "Wall time: 1 seconds\nProcess exited with code 0\nProcess running with session ID 1\nOutput:",
                ExecResult::Unknown,
            ),
        ] {
            assert_eq!(exec_result(&[text]), expected);
        }
        assert_eq!(
            exec_result(&["marker", "Process exited with code 0"]),
            ExecResult::Unknown
        );
        assert_eq!(
            exec_result(&[&"x".repeat(MAX_TEXT_BYTES + 1)]),
            ExecResult::Unknown
        );
    }

    #[test]
    fn exec_observation_deduplicates_identical_results_but_rejects_distinct_results() {
        let output = "Wall time: 1 seconds\nProcess exited with code 0\nOutput:\nPRIVATE";
        let one = request("tool", json!(output));
        let facts =
            ToolResultObservation::collect(&[one.clone(), one.clone()], SelectedTool::ExecCommand);
        assert_eq!(facts.exec_result, Some(ExecResult::ExitedZero));
        assert_eq!(facts.result_count, 1);
        assert!(!serde_json::to_string(&facts).unwrap().contains("PRIVATE"));
        let different = request(
            "tool",
            json!("Failed to create unified exec process: PRIVATE"),
        );
        let facts = ToolResultObservation::collect(&[one, different], SelectedTool::ExecCommand);
        assert_eq!(facts.exec_result, Some(ExecResult::Ambiguous));
        assert_eq!(
            ToolResultObservation::collect(&[], SelectedTool::Read).exec_result,
            None
        );
    }

    #[test]
    fn similar_ids_and_message_overflow_never_become_tool_evidence() {
        let mut wrong = request("tool", json!("Tool error: FileTooLargeError: private"));
        wrong["messages"][0]["tool_call_id"] = json!("call-nan-harness-conformance-0");
        let facts = ToolResultObservation::collect(&[wrong], SelectedTool::Read);
        assert!(!facts.result_present);
        let messages = vec![json!({"role":"user"}); MAX_MESSAGES + 1];
        let facts =
            ToolResultObservation::collect(&[json!({"messages":messages})], SelectedTool::Read);
        assert_eq!(facts.status, "limit");
        assert!(!facts.result_present);
    }
}
