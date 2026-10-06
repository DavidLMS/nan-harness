mod lifecycle;
mod polling;
mod protocol;
mod state;

pub use lifecycle::{ScriptedProvider, ScriptedProviderError};

use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ScriptedToolCall {
    pub name: String,
    pub input: Value,
    pub result_expected: bool,
}

#[derive(Debug, Clone)]
pub struct ProviderScenario {
    pub tool_calls: Vec<ScriptedToolCall>,
    pub final_marker: String,
    poll_exec: bool,
}

impl ProviderScenario {
    #[must_use]
    pub fn inventory(final_marker: impl Into<String>) -> Self {
        Self {
            tool_calls: Vec::new(),
            final_marker: final_marker.into(),
            poll_exec: false,
        }
    }

    #[must_use]
    pub fn tool(
        tool_name: impl Into<String>,
        tool_input: Value,
        final_marker: impl Into<String>,
    ) -> Self {
        Self::sequence(
            [ScriptedToolCall {
                name: tool_name.into(),
                input: tool_input,
                result_expected: true,
            }],
            final_marker,
        )
    }

    /// Read through unified exec, polling only the session returned by that call.
    #[must_use]
    pub fn exec_read(input: Value, final_marker: impl Into<String>) -> Self {
        let mut scenario = Self::tool("exec_command", input, final_marker);
        scenario.poll_exec = true;
        scenario
    }

    #[must_use]
    pub fn sequence(
        tool_calls: impl IntoIterator<Item = ScriptedToolCall>,
        final_marker: impl Into<String>,
    ) -> Self {
        Self {
            tool_calls: tool_calls.into_iter().collect(),
            final_marker: final_marker.into(),
            poll_exec: false,
        }
    }
}
