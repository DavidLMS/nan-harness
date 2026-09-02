use super::details;
use crate::commands::persistence::PersistenceError;
use nan_harness_telemetry::diagnostic::{
    Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason, DocumentKind,
};

pub(super) fn typed(error: &PersistenceError) -> Diagnostic {
    match error {
        PersistenceError::MissingConfigDirectory | PersistenceError::MissingHomeDirectory => {
            Diagnostic::general(DiagnosticReason::MissingDirectory)
        }
        PersistenceError::CreateDirectory { source, .. }
        | PersistenceError::WriteFile { source, .. }
        | PersistenceError::CreateStateDirectory(source) => {
            details::io(DiagnosticOperation::WriteConfiguration, source)
        }
        PersistenceError::ReadFile { source, .. }
        | PersistenceError::ReadState(source)
        | PersistenceError::ReadPreferences(source) => {
            details::io(DiagnosticOperation::ReadConfiguration, source)
        }
        PersistenceError::RemoveFile { source, .. } => {
            details::io(DiagnosticOperation::RemoveConfiguration, source)
        }
        PersistenceError::ManagedFileChanged(_)
        | PersistenceError::AmbiguousOpenCodeConfig(_)
        | PersistenceError::UnmanagedProviderConflict(_)
        | PersistenceError::ManagedProviderChanged(_)
        | PersistenceError::UnmanagedSectionConflict(_)
        | PersistenceError::ManagedSectionChanged(_) => {
            Diagnostic::general(DiagnosticReason::ConfigurationConflict)
        }
        PersistenceError::BuildClient(_) | PersistenceError::DiscoverModels(_) => {
            Diagnostic::general(DiagnosticReason::NetworkRequestFailed)
        }
        PersistenceError::ModelDiscoveryStatus(status) => Diagnostic::new(
            DiagnosticReason::HttpRequestRejected,
            DiagnosticDetails::Http {
                operation: DiagnosticOperation::DiscoverModels,
                status: *status,
            },
        ),
        PersistenceError::ModelDiscoveryTooLarge | PersistenceError::ParseModels(_) => {
            Diagnostic::general(DiagnosticReason::InvalidResponse)
        }
        PersistenceError::NoModels => Diagnostic::general(DiagnosticReason::ModelCatalogEmpty),
        PersistenceError::Secret(_) => {
            Diagnostic::general(DiagnosticReason::SecretResolutionFailed)
        }
        PersistenceError::SerializeProvider(_)
        | PersistenceError::SerializeState(_)
        | PersistenceError::SerializePreferences(_) => {
            Diagnostic::general(DiagnosticReason::SerializationFailed)
        }
        PersistenceError::UnsupportedStateSchema(version)
        | PersistenceError::UnsupportedPreferencesSchema(version) => Diagnostic::new(
            DiagnosticReason::UnsupportedVersion,
            DiagnosticDetails::Schema {
                document: DocumentKind::IntegrationState,
                observed_version: Some(u16::from(*version)),
            },
        ),
        PersistenceError::RenderConfiguration(_)
        | PersistenceError::InvalidPath(_)
        | PersistenceError::InvalidUtf8 { .. }
        | PersistenceError::InvalidReceiptPath(_)
        | PersistenceError::RootIsNotObject(_)
        | PersistenceError::ProviderIsNotObject(_)
        | PersistenceError::InvalidManagedProvider(_)
        | PersistenceError::InvalidManagedSection(_)
        | PersistenceError::InvalidManagedBlock
        | PersistenceError::ConfigRootIsNotObject { .. }
        | PersistenceError::ConfigFieldIsNotObject { .. }
        | PersistenceError::ParseHarnessConfig { .. }
        | PersistenceError::ParseOpenCodeConfig { .. }
        | PersistenceError::GenerateOpenCodeProvider(_)
        | PersistenceError::ParseState(_)
        | PersistenceError::ParsePreferences(_) => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
    }
}
