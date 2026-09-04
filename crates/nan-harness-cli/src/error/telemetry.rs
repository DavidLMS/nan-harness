mod context;
mod diagnostics;
mod failure;
mod reporting;

use super::CliError;
use crate::app::Cli;
use nan_harness_core::DetectedHarness;
use nan_harness_telemetry::diagnostic::Diagnostic;
use nan_harness_telemetry::event::{
    ErrorReportContext, FailureCategory, FailureCause, FailureStage,
};

impl CliError {
    pub(crate) fn telemetry_context(
        &self,
        cli: &Cli,
        interactive: bool,
        harness: Option<&DetectedHarness>,
    ) -> ErrorReportContext {
        context::build(self, cli, interactive, harness)
    }

    pub(crate) fn should_report_telemetry(&self, cli: &Cli) -> bool {
        reporting::should_report(self, cli)
    }

    const fn telemetry_failure(&self) -> (FailureCategory, FailureStage, bool) {
        failure::classify(self)
    }

    fn telemetry_diagnostics(&self) -> (FailureCause, Option<u16>) {
        diagnostics::classify(self)
    }

    fn typed_diagnostic(&self) -> Diagnostic {
        super::diagnostics::typed_diagnostic(self)
    }
}

#[cfg(test)]
mod tests {
    use super::diagnostics::runtime::{classify, classify_process, classify_search_policy};
    use crate::error::CliError;
    use nan_harness_runtime::{BridgeError, ProcessError, RuntimeError, SearchPolicyError};
    use nan_harness_telemetry::event::{FailureCategory, FailureCause, FailureStage};

    #[test]
    fn runtime_diagnostics_preserve_process_and_search_policy_classification() {
        assert_eq!(
            classify(&RuntimeError::Process(ProcessError::Spawn(
                std::io::Error::from(std::io::ErrorKind::NotFound),
            ))),
            (FailureCause::MissingExecutable, None),
        );
        assert_eq!(
            classify_process(&ProcessError::Spawn(std::io::Error::from(
                std::io::ErrorKind::PermissionDenied,
            ))),
            (FailureCause::PermissionDenied, None),
        );
        assert_eq!(
            classify_search_policy(&SearchPolicyError::RequiresDirectGateway),
            (FailureCause::InvalidConfiguration, None),
        );
    }

    #[test]
    fn runtime_diagnostics_preserve_bridge_status_and_codes() {
        assert_eq!(
            classify(&RuntimeError::Bridge(BridgeError::ModelDiscoveryStatus {
                status: reqwest::StatusCode::SERVICE_UNAVAILABLE,
                message: "redacted provider response".to_owned(),
            },)),
            (FailureCause::HttpStatus, Some(503)),
        );
        assert_eq!(
            classify(&RuntimeError::Bridge(BridgeError::NoCompatibleModels)),
            (FailureCause::InvalidConfiguration, None),
        );
    }

    #[tokio::test]
    async fn preflight_task_failures_are_internal_and_sanitized() {
        let task = tokio::spawn(std::future::pending::<()>());
        task.abort();
        let source = task.await.expect_err("aborted task should fail to join");
        let error = CliError::PreflightTaskFailed(source);

        assert_eq!(
            error.telemetry_failure(),
            (FailureCategory::Internal, FailureStage::Startup, false)
        );
        assert_eq!(
            error.telemetry_diagnostics(),
            (FailureCause::Internal, None)
        );
    }
}
