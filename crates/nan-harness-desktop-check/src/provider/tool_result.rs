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
}

impl SelectedTool {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "Read" => Some(Self::Read),
            "read_file" => Some(Self::ReadFile),
            "read_files" => Some(Self::ReadFiles),
            "exec_command" => Some(Self::ExecCommand),
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
                let Some((shape, error, text)) = classify(content) else {
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
                    result.tool_error_detected = true;
                }
            }
        }
        result
    }
}

fn classify(content: &Value) -> Option<(Shape, Option<ErrorCategory>, Vec<&str>)> {
    if let Some(text) = content.as_str() {
        if text.len() > MAX_TEXT_BYTES {
            return None;
        }
        return Some((
            Shape::String,
            text.strip_prefix("Tool error: ").map(error_category),
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
                let found = error_category(text);
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
