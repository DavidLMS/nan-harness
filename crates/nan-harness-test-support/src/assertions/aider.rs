use super::errors::ProbeAssertionError;
use super::extraction::{request_has_tool_calls, request_has_tool_traffic};
use crate::terminal::TerminalOutput;
use serde_json::Value;

/// Asserts a no-tool sentinel exchange reached the provider and emitted no tool call/result.
///
/// # Errors
///
/// Returns [`ProbeAssertionError`] when the provider transcript contains no request, a tool
/// call/result, or malformed provider traffic.
pub fn assert_sentinel(
    output: &TerminalOutput,
    requests: &[Value],
    final_marker: &str,
) -> Result<(), ProbeAssertionError> {
    if !output.status.success() {
        return Err(ProbeAssertionError::ProcessFailed);
    }
    if !output.stdout.contains(final_marker) {
        return Err(ProbeAssertionError::MissingMarker(final_marker.to_owned()));
    }
    if requests.is_empty() {
        return Err(ProbeAssertionError::MissingProviderRequest);
    }
    if requests.iter().any(request_has_tool_traffic) {
        return Err(ProbeAssertionError::UnexpectedToolTraffic);
    }
    Ok(())
}

/// Asserts Aider's non-function edit protocol and the required file mutation.
///
/// # Errors
///
/// Returns [`ProbeAssertionError`] when Aider advertises function tools, misses the provider,
/// emits the marker, or fails to mutate the target file.
pub fn assert_aider_edit_protocol(
    output: &TerminalOutput,
    requests: &[Value],
    target: &std::path::Path,
    before: &str,
    after: &str,
) -> Result<(), ProbeAssertionError> {
    if !output.status.success() {
        return Err(ProbeAssertionError::ProcessFailed);
    }
    if !output.stdout.contains(after) {
        return Err(ProbeAssertionError::MissingMarker(after.to_owned()));
    }
    if requests.is_empty() {
        return Err(ProbeAssertionError::MissingProviderRequest);
    }
    if requests
        .iter()
        .any(|request| request.get("tools").is_some() || request_has_tool_calls(request))
    {
        return Err(ProbeAssertionError::UnexpectedFunctionTools);
    }
    let contents = std::fs::read_to_string(target)
        .map_err(|error| ProbeAssertionError::Filesystem(error.to_string()))?;
    if contents == before || !contents.contains(after) {
        return Err(ProbeAssertionError::MissingFilesystemSideEffect(
            target.to_owned(),
        ));
    }
    Ok(())
}
