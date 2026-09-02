use super::errors::TranscriptError;
use super::extraction::{
    find_all_tool_results, find_all_tool_uses, value_contains_bool, value_contains_pair,
    value_contains_string,
};
use serde_json::Value;
use std::collections::BTreeSet;

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
