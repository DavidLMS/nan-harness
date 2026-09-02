use super::{bridge, details, plan};
use nan_harness_runtime::{ProcessError, RuntimeError, SearchPolicyError};
use nan_harness_telemetry::diagnostic::{
    Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason, IoErrorKind,
};

pub(super) fn typed(error: &RuntimeError) -> Diagnostic {
    match error {
        RuntimeError::InvalidPlan(error) => plan::typed(error),
        RuntimeError::BindBridge(source) => details::io(DiagnosticOperation::BindBridge, source),
        RuntimeError::Bridge(error) => bridge::typed(error),
        RuntimeError::BridgeExited => Diagnostic::general(DiagnosticReason::BridgeExited),
        RuntimeError::Prepared(_) => Diagnostic::general(DiagnosticReason::LaunchPreparationFailed),
        RuntimeError::Process(ProcessError::Secret(_)) | RuntimeError::Secret(_) => {
            Diagnostic::general(DiagnosticReason::SecretResolutionFailed)
        }
        RuntimeError::Process(ProcessError::Spawn(source)) => {
            let reason = if source.kind() == std::io::ErrorKind::NotFound {
                DiagnosticReason::MissingExecutable
            } else {
                DiagnosticReason::ProcessStartFailed
            };
            Diagnostic::new(
                reason,
                DiagnosticDetails::Io {
                    operation: DiagnosticOperation::StartHarness,
                    error_kind: IoErrorKind::from_std(source.kind()),
                },
            )
        }
        RuntimeError::Random(_) => Diagnostic::general(DiagnosticReason::RandomGenerationFailed),
        RuntimeError::WaitForProcess(source) => Diagnostic::new(
            DiagnosticReason::ProcessWaitFailed,
            DiagnosticDetails::Io {
                operation: DiagnosticOperation::WaitForHarness,
                error_kind: IoErrorKind::from_std(source.kind()),
            },
        ),
        RuntimeError::TerminateProcess(source) => Diagnostic::new(
            DiagnosticReason::ProcessTerminationFailed,
            DiagnosticDetails::Io {
                operation: DiagnosticOperation::StopHarness,
                error_kind: IoErrorKind::from_std(source.kind()),
            },
        ),
        RuntimeError::MissingProcessId => {
            Diagnostic::general(DiagnosticReason::ProcessTerminationFailed)
        }
        RuntimeError::SearchPolicy(error) => search_policy(error),
    }
}

fn search_policy(error: &SearchPolicyError) -> Diagnostic {
    match error {
        SearchPolicyError::ReadConfiguration { source, .. } => {
            details::io(DiagnosticOperation::ReadConfiguration, source)
        }
        SearchPolicyError::MissingHomeDirectory
        | SearchPolicyError::UnsupportedHarness(_)
        | SearchPolicyError::RequiresDirectGateway
        | SearchPolicyError::McpNameCollision(_)
        | SearchPolicyError::ConfigurationTooLarge(_)
        | SearchPolicyError::ParseJson { .. }
        | SearchPolicyError::ParseToml { .. }
        | SearchPolicyError::ConvertToml { .. } => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
    }
}
