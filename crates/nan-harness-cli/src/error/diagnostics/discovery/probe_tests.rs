use super::typed;
use nan_harness_runtime::DiscoveryError;
use nan_harness_telemetry::diagnostic::{DiagnosticDetails, DiagnosticOperation, DiagnosticReason};

#[test]
fn probe_failures_have_closed_reasons_and_safe_process_details() {
    for (error, reason, code) in [
        (
            DiscoveryError::VersionProbeTimeout,
            DiagnosticReason::DiscoveryProbeTimeout,
            "NH-DISCOVERY-006",
        ),
        (
            DiscoveryError::VersionProbeOutputLimit,
            DiagnosticReason::DiscoveryProbeOutputLimit,
            "NH-DISCOVERY-007",
        ),
    ] {
        let diagnostic = typed(&error);
        assert_eq!(
            diagnostic,
            nan_harness_telemetry::diagnostic::Diagnostic::new(
                reason,
                DiagnosticDetails::Process {
                    operation: DiagnosticOperation::RunVersionCommand,
                    exit_code: None
                }
            )
        );
        assert_eq!(error.code(), code);
        assert!(error.to_string().contains("--executable"));
    }
}
