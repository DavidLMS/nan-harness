use super::{Classification, io};
use nan_harness_runtime::{BridgeError, ProcessError, RuntimeError, SearchPolicyError};
use nan_harness_telemetry::event::FailureCause;

pub(in crate::error::telemetry) fn classify(error: &RuntimeError) -> Classification {
    match error {
        RuntimeError::InvalidPlan(_) | RuntimeError::Prepared(_) => {
            (FailureCause::InvalidData, None)
        }
        RuntimeError::BindBridge(source)
        | RuntimeError::WaitForProcess(source)
        | RuntimeError::TerminateProcess(source) => (io::classify(source), None),
        RuntimeError::Bridge(error) => classify_bridge(error),
        RuntimeError::BridgeExited | RuntimeError::MissingProcessId => {
            (FailureCause::ProcessExit, None)
        }
        RuntimeError::Process(error) => classify_process(error),
        RuntimeError::Secret(_) => (FailureCause::MissingCredential, None),
        RuntimeError::SearchPolicy(error) => classify_search_policy(error),
        RuntimeError::Random(_) => (FailureCause::Internal, None),
    }
}

fn classify_bridge(error: &BridgeError) -> Classification {
    if let Some(diagnostics) = classify_bridge_http(error) {
        return diagnostics;
    }
    classify_bridge_code(error).unwrap_or((FailureCause::Internal, None))
}

fn classify_bridge_http(error: &BridgeError) -> Option<Classification> {
    if let Some(status) = error.http_status() {
        return Some((FailureCause::HttpStatus, Some(status)));
    }
    if error.is_timeout() {
        return Some((FailureCause::Timeout, None));
    }
    if error.is_invalid_response() {
        return Some((FailureCause::InvalidResponse, None));
    }
    None
}

fn classify_bridge_code(error: &BridgeError) -> Option<Classification> {
    match error.code() {
        "NH-BRIDGE-004" => Some((FailureCause::Network, None)),
        "NH-BRIDGE-005" => Some((FailureCause::InvalidConfiguration, None)),
        _ => None,
    }
}

pub(in crate::error::telemetry) fn classify_process(error: &ProcessError) -> Classification {
    match error {
        ProcessError::Secret(_) => (FailureCause::MissingCredential, None),
        ProcessError::Spawn(source) => match io::classify(source) {
            FailureCause::NotFound => (FailureCause::MissingExecutable, None),
            FailureCause::PermissionDenied => (FailureCause::PermissionDenied, None),
            _ => (FailureCause::ProcessStart, None),
        },
    }
}

pub(in crate::error::telemetry) fn classify_search_policy(
    error: &SearchPolicyError,
) -> Classification {
    match error {
        SearchPolicyError::ReadConfiguration { source, .. } => (io::classify(source), None),
        SearchPolicyError::MissingHomeDirectory
        | SearchPolicyError::UnsupportedHarness(_)
        | SearchPolicyError::RequiresDirectGateway
        | SearchPolicyError::McpNameCollision(_)
        | SearchPolicyError::ConfigurationTooLarge(_)
        | SearchPolicyError::ParseJson { .. }
        | SearchPolicyError::ParseToml { .. }
        | SearchPolicyError::ConvertToml { .. } => (FailureCause::InvalidConfiguration, None),
    }
}
