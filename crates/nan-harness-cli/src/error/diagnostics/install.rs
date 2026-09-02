use super::details;
use crate::commands::install::InstallError;
use nan_harness_telemetry::diagnostic::{
    Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason, IoErrorKind,
    VersionComponent,
};

pub(super) fn typed(error: &InstallError) -> Diagnostic {
    match error {
        InstallError::Prompt(source) => Diagnostic::new(
            DiagnosticReason::UserPromptFailed,
            DiagnosticDetails::Io {
                operation: DiagnosticOperation::RunInstaller,
                error_kind: IoErrorKind::from_std(source.kind()),
            },
        ),
        InstallError::UnsupportedPlatform(_)
        | InstallError::UnsupportedHarness(_)
        | InstallError::CompatibilityManifest(_)
        | InstallError::InvalidRuntimeCommand { .. } => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
        InstallError::RuntimeCommandStart { source, .. }
        | InstallError::PrepareInstaller { source, .. }
        | InstallError::InstallerStart { source, .. }
        | InstallError::CommandStart { source, .. } => {
            details::io(DiagnosticOperation::RunInstaller, source)
        }
        InstallError::RuntimeCommandFailed { exit_code, .. }
        | InstallError::InstallerFailed { exit_code, .. }
        | InstallError::CommandFailed { exit_code, .. } => details::process(
            DiagnosticReason::ProcessExited,
            DiagnosticOperation::RunInstaller,
            *exit_code,
        ),
        InstallError::RuntimeUnsupported {
            detected, minimum, ..
        } => details::version(
            DiagnosticReason::UnsupportedVersion,
            VersionComponent::Runtime,
            details::safe_version(detected),
            Some(minimum.to_string()),
        ),
        InstallError::RuntimeUnparseable { minimum, .. } => details::version(
            DiagnosticReason::UnparseableVersion,
            VersionComponent::Runtime,
            None,
            Some(minimum.to_string()),
        ),
        InstallError::DownloadStart { source, .. } => {
            details::io(DiagnosticOperation::DownloadInstaller, source)
        }
        InstallError::DownloadFailed { exit_code, .. } => details::process(
            DiagnosticReason::ProcessExited,
            DiagnosticOperation::DownloadInstaller,
            *exit_code,
        ),
        InstallError::PostInstallCheckStart { source, .. }
        | InstallError::PostInstallCheckPrepare { source, .. } => {
            details::io(DiagnosticOperation::RunPostInstallCheck, source)
        }
        InstallError::PostInstallCheckFailed { exit_code, .. } => details::process(
            DiagnosticReason::ProcessExited,
            DiagnosticOperation::RunPostInstallCheck,
            *exit_code,
        ),
    }
}
