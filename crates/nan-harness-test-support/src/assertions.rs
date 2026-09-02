mod aider;
mod claude;
mod errors;
mod extraction;
mod provider;

pub use aider::{assert_aider_edit_protocol, assert_sentinel};
pub use claude::ClaudeTranscript;
pub use errors::{ProbeAssertionError, TranscriptError};
pub use provider::{
    assert_provider_tool_round_trip, assert_tool_results, assert_tool_round_trip,
    assert_tool_round_trip_with_sanitized_ids, expected_tool_call_id,
};

#[cfg(test)]
#[path = "assertions/tests.rs"]
mod tests;
