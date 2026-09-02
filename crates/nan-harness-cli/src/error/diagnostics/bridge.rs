use super::details;
use nan_harness_runtime::BridgeError;
use nan_harness_telemetry::diagnostic::{
    Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason,
};

pub(super) fn typed(error: &BridgeError) -> Diagnostic {
    match error {
        BridgeError::ListenerAddress(source) | BridgeError::Serve(source) => {
            details::io(DiagnosticOperation::RunBridge, source)
        }
        BridgeError::NonLoopbackAddress(_) | BridgeError::BuildClient(_) => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
        BridgeError::ModelDiscoveryTransport(_) => {
            Diagnostic::general(DiagnosticReason::NetworkRequestFailed)
        }
        BridgeError::ModelDiscoveryStatus { status, .. } => Diagnostic::new(
            DiagnosticReason::HttpRequestRejected,
            DiagnosticDetails::Http {
                operation: DiagnosticOperation::DiscoverModels,
                status: status.as_u16(),
            },
        ),
        BridgeError::ModelDiscoveryTooLarge | BridgeError::InvalidModelDiscoveryResponse(_) => {
            Diagnostic::general(DiagnosticReason::InvalidResponse)
        }
        BridgeError::NoCompatibleModels => Diagnostic::general(DiagnosticReason::ModelCatalogEmpty),
        BridgeError::SelectedModelUnavailable { .. } => {
            Diagnostic::general(DiagnosticReason::ModelUnavailable)
        }
        BridgeError::TaskJoin(_) => Diagnostic::general(DiagnosticReason::BridgeExited),
    }
}
