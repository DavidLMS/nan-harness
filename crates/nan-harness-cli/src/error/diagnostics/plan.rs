use nan_harness_core::PlanError;
use nan_harness_telemetry::diagnostic::{Diagnostic, DiagnosticReason};

pub(super) fn typed(error: &PlanError) -> Diagnostic {
    match error {
        PlanError::InvalidField { .. }
        | PlanError::AdapterMismatch { .. }
        | PlanError::TransportMismatch { .. } => {
            Diagnostic::general(DiagnosticReason::InvalidLaunchPlan)
        }
        PlanError::MissingSecretReference { .. } => {
            Diagnostic::general(DiagnosticReason::SecretResolutionFailed)
        }
        PlanError::ConflictingEnvironment { .. } | PlanError::UnsafeTemporaryArtifact { .. } => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
    }
}
