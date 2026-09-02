use serde_json::Value;
use std::collections::BTreeSet;
use thiserror::Error;

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
