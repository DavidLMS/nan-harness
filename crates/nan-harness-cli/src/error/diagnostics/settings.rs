use super::details;
use nan_harness_telemetry::consent::SettingsError;
use nan_harness_telemetry::diagnostic::{
    Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason, DocumentKind,
};

pub(super) fn typed(error: &SettingsError) -> Diagnostic {
    match error {
        SettingsError::MissingConfigDirectory => {
            Diagnostic::general(DiagnosticReason::MissingDirectory)
        }
        SettingsError::CreateDirectory(source) | SettingsError::Write(source) => {
            details::io(DiagnosticOperation::ConfigureTelemetry, source)
        }
        SettingsError::Read(source) => details::io(DiagnosticOperation::ReadConfiguration, source),
        SettingsError::Parse(_) => Diagnostic::new(
            DiagnosticReason::InvalidConfiguration,
            DiagnosticDetails::Schema {
                document: DocumentKind::TelemetrySettings,
                observed_version: None,
            },
        ),
        SettingsError::Serialize(_) => Diagnostic::general(DiagnosticReason::SerializationFailed),
        SettingsError::Random(_) => Diagnostic::general(DiagnosticReason::RandomGenerationFailed),
    }
}
