use super::details;
#[cfg(test)]
mod probe_tests;
use nan_harness_runtime::DiscoveryError;
use nan_harness_telemetry::diagnostic::{
    Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason, DocumentKind,
    VersionComponent,
};

pub(super) fn typed(error: &DiscoveryError) -> Diagnostic {
    match error {
        DiscoveryError::VersionProbeTimeout => details::process(
            DiagnosticReason::DiscoveryProbeTimeout,
            DiagnosticOperation::RunVersionCommand,
            None,
        ),
        DiscoveryError::VersionProbeOutputLimit => details::process(
            DiagnosticReason::DiscoveryProbeOutputLimit,
            DiagnosticOperation::RunVersionCommand,
            None,
        ),
        DiscoveryError::InvalidManifest(_) | DiscoveryError::InvalidManifestContract(_) => {
            Diagnostic::new(
                DiagnosticReason::InvalidManifest,
                DiagnosticDetails::Schema {
                    document: DocumentKind::CompatibilityManifest,
                    observed_version: None,
                },
            )
        }
        DiscoveryError::MissingCompatibilityEntry(_) => {
            Diagnostic::general(DiagnosticReason::MissingManifestEntry)
        }
        DiscoveryError::InvalidVersionCommand { .. } => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
        DiscoveryError::ExecutableNotFound(_) => {
            Diagnostic::general(DiagnosticReason::MissingExecutable)
        }
        DiscoveryError::InvalidExecutable(_) => {
            Diagnostic::general(DiagnosticReason::InvalidExecutable)
        }
        DiscoveryError::VersionCommand { source, .. } => {
            details::io(DiagnosticOperation::RunVersionCommand, source)
        }
        DiscoveryError::VersionCommandFailed { exit_code, .. } => details::process(
            DiagnosticReason::ProcessExited,
            DiagnosticOperation::RunVersionCommand,
            *exit_code,
        ),
        DiscoveryError::UnsupportedVersion { detected, .. } => details::version(
            DiagnosticReason::UnsupportedVersion,
            VersionComponent::Harness,
            details::safe_version(detected),
            None,
        ),
        DiscoveryError::UnparseableVersion { .. } => details::version(
            DiagnosticReason::UnparseableVersion,
            VersionComponent::Harness,
            None,
            None,
        ),
    }
}
