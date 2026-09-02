use super::{details, persistence};
use crate::commands::uninstall::UninstallError;
use nan_harness_telemetry::diagnostic::{
    Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason, DocumentKind,
};

pub(super) fn typed(error: &UninstallError) -> Diagnostic {
    match error {
        UninstallError::Configuration(_) | UninstallError::Credential(_) => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
        UninstallError::HermesDesktop(error) => error.diagnostic(),
        UninstallError::PenDesktop(error) => error.diagnostic(),
        UninstallError::Persistence(error) => persistence::typed(error),
        UninstallError::ConfirmationRequired | UninstallError::DesktopRecoveryRequired(_) => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
        UninstallError::InstallationNotManaged
        | UninstallError::ExecutableMismatch { .. }
        | UninstallError::UnsafeInstallationPath(_)
        | UninstallError::UnsafeAliasPath(_)
        | UninstallError::UnsafeDataDirectory(_) => {
            Diagnostic::general(DiagnosticReason::ConfigurationConflict)
        }
        UninstallError::CurrentExecutable(source)
        | UninstallError::CanonicalizeExecutable { source, .. }
        | UninstallError::InspectDataDirectory { source, .. }
        | UninstallError::InspectAlias { source, .. }
        | UninstallError::ReadReceipt { source, .. }
        | UninstallError::CreateDataDirectory { source, .. }
        | UninstallError::WriteReceipt { source, .. }
        | UninstallError::Prompt(source) => {
            details::io(DiagnosticOperation::RemoveInstallation, source)
        }
        UninstallError::ParseReceipt(_) => Diagnostic::new(
            DiagnosticReason::InvalidConfiguration,
            DiagnosticDetails::Schema {
                document: DocumentKind::InstallationReceipt,
                observed_version: None,
            },
        ),
        UninstallError::UnsupportedReceiptSchema(version) => Diagnostic::new(
            DiagnosticReason::UnsupportedVersion,
            DiagnosticDetails::Schema {
                document: DocumentKind::InstallationReceipt,
                observed_version: Some(u16::from(*version)),
            },
        ),
        UninstallError::SerializeReceipt(_) => {
            Diagnostic::general(DiagnosticReason::SerializationFailed)
        }
        #[cfg(not(windows))]
        UninstallError::RemoveFile { source, .. }
        | UninstallError::RemoveDataDirectory { source, .. } => {
            details::io(DiagnosticOperation::RemoveInstallation, source)
        }
        #[cfg(windows)]
        UninstallError::CreateHelper(source) | UninstallError::StartHelper(source) => {
            details::io(DiagnosticOperation::RemoveInstallation, source)
        }
    }
}
