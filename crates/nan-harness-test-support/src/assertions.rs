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
        if !self.contains_pair("name", tool_name) || !self.contains_pair("type", "tool_use") {
            return Err(TranscriptError::MissingToolUse(tool_name.to_owned()));
        }
        if !self.contains_pair("type", "tool_result") {
            return Err(TranscriptError::MissingToolResult(tool_name.to_owned()));
        }
        if self.contains_bool("is_error", true) {
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
        if !self.contains_pair("name", tool_name) || !self.contains_pair("type", "tool_use") {
            return Err(TranscriptError::MissingToolUse(tool_name.to_owned()));
        }
        if !self.contains_pair("type", "tool_result") {
            return Err(TranscriptError::MissingToolResult(tool_name.to_owned()));
        }
        if !self.contains_bool("is_error", true) {
            return Err(TranscriptError::MissingExpectedToolError(
                tool_name.to_owned(),
            ));
        }
        if !self.source.contains(expected_error) {
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

    fn contains_bool(&self, key: &str, expected: bool) -> bool {
        self.events
            .iter()
            .any(|event| value_contains_bool(event, key, expected))
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
}
