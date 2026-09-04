use super::super::CliError;
use crate::app::Cli;
use crate::observability::{HarnessIdentitySource, enrich_telemetry_context};
use nan_harness_core::DetectedHarness;
use nan_harness_telemetry::event::{ErrorReportContext, Failure, UserGuidance};

pub(super) fn build(
    error: &CliError,
    cli: &Cli,
    interactive: bool,
    harness: Option<&DetectedHarness>,
) -> ErrorReportContext {
    let (category, stage, retryable) = error.telemetry_failure();
    let (cause, http_status) = error.telemetry_diagnostics();
    let mut failure = Failure::new(error.code(), category, stage, retryable).with_cause(cause);
    if let Some(status) = http_status {
        failure = failure.with_http_status(status);
    }

    let harness_source = harness.map_or_else(
        || {
            if matches!(error, CliError::CurrentDirectory(_)) {
                HarnessIdentitySource::KindOnly
            } else {
                HarnessIdentitySource::Detect
            }
        },
        HarnessIdentitySource::Known,
    );
    let mut context = enrich_telemetry_context(
        ErrorReportContext::new(failure, interactive).with_diagnostic(error.typed_diagnostic()),
        cli,
        harness_source,
    );
    if matches!(error, CliError::CurrentDirectory(_)) {
        context = context.with_user_guidance(UserGuidance::reopen_terminal(true));
    }
    context
}
