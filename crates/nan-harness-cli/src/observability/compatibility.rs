use super::context::{HarnessIdentitySource, enrich_telemetry_context};
use crate::app::Cli;
use nan_harness_runtime::CompatibilityError;
use nan_harness_telemetry::TelemetryReporter;
use nan_harness_telemetry::diagnostic::{
    Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason, DocumentKind,
};
use nan_harness_telemetry::event::{
    ErrorReportContext, Failure, FailureCategory, FailureCause, FailureStage,
};
use nan_harness_telemetry::glitchtip::GlitchTipExporter;

pub(crate) async fn report_compat_error(
    reporter: &TelemetryReporter<GlitchTipExporter>,
    error: &CompatibilityError,
    cli: &Cli,
    interactive: bool,
) {
    let cause = match error {
        CompatibilityError::FetchManifest(_) | CompatibilityError::ManifestStatus(_) => {
            FailureCause::Network
        }
        CompatibilityError::ParseManifest(_) | CompatibilityError::InvalidEmbeddedManifest(_) => {
            FailureCause::InvalidData
        }
        CompatibilityError::UnsupportedManifestSchema(_) => FailureCause::UnsupportedVersion,
        CompatibilityError::LiveEvidenceAhead { .. }
        | CompatibilityError::VersionBelowMinimum { .. }
        | CompatibilityError::LiveVersionBelowMinimum { .. }
        | CompatibilityError::EmptyReleases
        | CompatibilityError::DuplicateRelease(_)
        | CompatibilityError::DuplicateHarness(_)
        | CompatibilityError::IncompleteEvidencePair { .. }
        | CompatibilityError::MissingEvidence { .. }
        | CompatibilityError::InvalidEvidenceTimestamp { .. }
        | CompatibilityError::InvalidUrl { .. }
        | CompatibilityError::InsecureUrl
        | CompatibilityError::ManifestTooLarge => FailureCause::InvalidData,
        CompatibilityError::MissingConfigDirectory
        | CompatibilityError::ReadState(_)
        | CompatibilityError::ParseState(_)
        | CompatibilityError::UnsupportedStateSchema(_)
        | CompatibilityError::CreateConfigDirectory(_)
        | CompatibilityError::SerializeState(_)
        | CompatibilityError::WriteState(_) => FailureCause::Filesystem,
        CompatibilityError::BuildClient(_) | CompatibilityError::SystemClock(_) => {
            FailureCause::Internal
        }
    };
    let retryable = match error {
        CompatibilityError::FetchManifest(_) => true,
        CompatibilityError::ManifestStatus(status) => matches!(status, 408 | 425 | 429 | 500..=599),
        _ => false,
    };
    let failure = Failure::new(
        error.code(),
        FailureCategory::Configuration,
        FailureStage::Startup,
        retryable,
    )
    .with_cause(cause);
    let context = enrich_telemetry_context(
        ErrorReportContext::new(failure, interactive).with_diagnostic(compat_diagnostic(error)),
        cli,
        HarnessIdentitySource::KindOnly,
    );
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stderr().lock();
    let _ = reporter.report(context, &mut input, &mut output).await;
}

fn compat_diagnostic(error: &CompatibilityError) -> Diagnostic {
    match error {
        CompatibilityError::FetchManifest(_) => {
            Diagnostic::general(DiagnosticReason::NetworkRequestFailed)
        }
        CompatibilityError::ManifestStatus(status) => Diagnostic::new(
            DiagnosticReason::HttpRequestRejected,
            DiagnosticDetails::Http {
                operation: DiagnosticOperation::FetchUpdateManifest,
                status: *status,
            },
        ),
        CompatibilityError::UnsupportedManifestSchema(version) => Diagnostic::new(
            DiagnosticReason::UnsupportedVersion,
            DiagnosticDetails::Schema {
                document: DocumentKind::CompatibilityManifest,
                observed_version: Some(u16::from(*version)),
            },
        ),
        CompatibilityError::MissingConfigDirectory => {
            Diagnostic::general(DiagnosticReason::MissingDirectory)
        }
        CompatibilityError::ReadState(source)
        | CompatibilityError::CreateConfigDirectory(source)
        | CompatibilityError::WriteState(source) => Diagnostic::new(
            DiagnosticReason::FilesystemOperationFailed,
            DiagnosticDetails::Io {
                operation: DiagnosticOperation::ReadConfiguration,
                error_kind: nan_harness_telemetry::diagnostic::IoErrorKind::from_std(source.kind()),
            },
        ),
        CompatibilityError::ParseManifest(_)
        | CompatibilityError::InvalidEmbeddedManifest(_)
        | CompatibilityError::LiveEvidenceAhead { .. }
        | CompatibilityError::VersionBelowMinimum { .. }
        | CompatibilityError::LiveVersionBelowMinimum { .. }
        | CompatibilityError::EmptyReleases
        | CompatibilityError::DuplicateRelease(_)
        | CompatibilityError::DuplicateHarness(_)
        | CompatibilityError::IncompleteEvidencePair { .. }
        | CompatibilityError::MissingEvidence { .. }
        | CompatibilityError::InvalidEvidenceTimestamp { .. }
        | CompatibilityError::InvalidUrl { .. }
        | CompatibilityError::InsecureUrl
        | CompatibilityError::ManifestTooLarge
        | CompatibilityError::ParseState(_)
        | CompatibilityError::UnsupportedStateSchema(_)
        | CompatibilityError::SerializeState(_) => {
            Diagnostic::general(DiagnosticReason::InvalidManifest)
        }
        CompatibilityError::BuildClient(_) | CompatibilityError::SystemClock(_) => {
            Diagnostic::general(DiagnosticReason::InternalInvariant)
        }
    }
}
