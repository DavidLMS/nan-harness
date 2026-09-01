use super::*;

#[test]
fn hermes_error_diagnostics_preserve_family_classification() {
    assert_eq!(
        HermesDesktopError::AlreadyRunning.diagnostic(),
        Diagnostic::general(DiagnosticReason::ConfigurationConflict)
    );
    assert_eq!(
        HermesDesktopError::ModelUnavailable {
            model: "requested".to_owned(),
            available: vec!["available".to_owned()],
        }
        .diagnostic(),
        Diagnostic::general(DiagnosticReason::ModelUnavailable)
    );
    assert_eq!(
        HermesDesktopError::Gateway(ChatGatewayError::Bridge(
            nan_harness_runtime::BridgeError::NoCompatibleModels,
        ))
        .diagnostic(),
        Diagnostic::general(DiagnosticReason::ModelCatalogEmpty)
    );
    assert_eq!(
        HermesDesktopError::GatewayExited.diagnostic(),
        Diagnostic::general(DiagnosticReason::BridgeExited)
    );
    assert_eq!(
        HermesDesktopError::Secret(nan_harness_core::SecretError::EmptyValue).diagnostic(),
        Diagnostic::general(DiagnosticReason::SecretResolutionFailed)
    );

    let permission_denied = std::io::Error::from(ErrorKind::PermissionDenied);
    assert_eq!(
        HermesDesktopError::Launch(permission_denied).diagnostic(),
        Diagnostic::new(
            DiagnosticReason::FilesystemOperationFailed,
            DiagnosticDetails::Io {
                operation: DiagnosticOperation::StartHarness,
                error_kind: IoErrorKind::PermissionDenied,
            },
        )
    );
    assert_eq!(
        HermesDesktopError::ProcessCheckFailed(Some(7)).diagnostic(),
        Diagnostic::general(DiagnosticReason::ProcessWaitFailed)
    );

    assert_eq!(
        HermesDesktopError::DesktopUnavailable.diagnostic(),
        Diagnostic::general(DiagnosticReason::UnsupportedVersion)
    );
    assert_eq!(
        HermesDesktopError::InvalidProfilePath.diagnostic(),
        Diagnostic::general(DiagnosticReason::InvalidConfiguration)
    );
    assert_eq!(
        HermesDesktopError::Serialize(
            serde_json::from_str::<serde_json::Value>("not json").expect_err("invalid JSON"),
        )
        .diagnostic(),
        Diagnostic::general(DiagnosticReason::SerializationFailed)
    );
    assert_eq!(
        HermesDesktopError::MissingStateDirectory.diagnostic(),
        Diagnostic::general(DiagnosticReason::MissingDirectory)
    );
    assert_eq!(
        HermesDesktopError::ReadFile(std::io::Error::from(ErrorKind::NotFound)).diagnostic(),
        Diagnostic::new(
            DiagnosticReason::FilesystemOperationFailed,
            DiagnosticDetails::Io {
                operation: DiagnosticOperation::WriteConfiguration,
                error_kind: IoErrorKind::NotFound,
            },
        )
    );
    assert_eq!(
        HermesDesktopError::Persistence(
            crate::commands::persistence::PersistenceError::MissingConfigDirectory,
        )
        .diagnostic(),
        Diagnostic::general(DiagnosticReason::FilesystemOperationFailed)
    );
    assert_eq!(
        HermesDesktopError::Compatibility(
            nan_harness_runtime::DesktopCompatibilityError::Unavailable,
        )
        .diagnostic(),
        Diagnostic::general(DiagnosticReason::FilesystemOperationFailed)
    );

    #[cfg(any(windows, test))]
    assert_eq!(
        HermesDesktopError::InvalidProcessListing.diagnostic(),
        Diagnostic::general(DiagnosticReason::InvalidResponse)
    );
}
