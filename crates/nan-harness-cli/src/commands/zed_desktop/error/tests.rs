use super::ZedDesktopError;
use crate::commands::desktop::DesktopStateError;
use nan_harness_runtime::{BridgeError, ChatGatewayError, DesktopCompatibilityError};
use nan_harness_telemetry::diagnostic::{DiagnosticDetails, DiagnosticReason};
use serde_json::json;
use std::io;

struct Expected {
    code: &'static str,
    reason: DiagnosticReason,
}

fn assert_contract(error: &ZedDesktopError, expected: &Expected) {
    assert_eq!(error.code(), expected.code);
    let diagnostic = error.diagnostic();
    assert_eq!(diagnostic.reason(), expected.reason);
    assert_eq!(diagnostic.details(), &DiagnosticDetails::General);
}

fn invalid_json() -> serde_json::Error {
    serde_json::from_str::<serde_json::Value>("not-json").expect_err("fixture must be invalid")
}

fn invalid_utf8() -> std::str::Utf8Error {
    let bytes = vec![u8::MAX];
    std::str::from_utf8(&bytes).expect_err("fixture must be invalid UTF-8")
}

#[test]
fn executable_and_compatibility_errors_have_stable_typed_categories() {
    let cases = [
        (
            ZedDesktopError::UnsupportedPlatform,
            Expected {
                code: "NH-ZED-001",
                reason: DiagnosticReason::UnsupportedVersion,
            },
        ),
        (
            ZedDesktopError::Compatibility(DesktopCompatibilityError::MissingPlatform),
            Expected {
                code: "NH-ZED-001",
                reason: DiagnosticReason::UnsupportedVersion,
            },
        ),
        (
            ZedDesktopError::OlderUnsupported,
            Expected {
                code: "NH-ZED-001",
                reason: DiagnosticReason::UnsupportedVersion,
            },
        ),
        (
            ZedDesktopError::NewerUntested,
            Expected {
                code: "NH-ZED-001",
                reason: DiagnosticReason::UnsupportedVersion,
            },
        ),
        (
            ZedDesktopError::AppNotFound,
            Expected {
                code: "NH-ZED-001",
                reason: DiagnosticReason::MissingExecutable,
            },
        ),
        (
            ZedDesktopError::InvalidInstallation,
            Expected {
                code: "NH-ZED-001",
                reason: DiagnosticReason::InvalidExecutable,
            },
        ),
        (
            ZedDesktopError::VersionCommand(io::Error::from(io::ErrorKind::NotFound)),
            Expected {
                code: "NH-ZED-001",
                reason: DiagnosticReason::UnparseableVersion,
            },
        ),
        (
            ZedDesktopError::VersionCommandFailed(Some(17)),
            Expected {
                code: "NH-ZED-001",
                reason: DiagnosticReason::UnparseableVersion,
            },
        ),
    ];

    for (error, expected) in cases {
        assert_contract(&error, &expected);
    }
}

#[test]
fn conflict_process_and_gateway_errors_keep_their_distinct_categories() {
    let cases = [
        (
            ZedDesktopError::AlreadyRunning,
            Expected {
                code: "NH-ZED-002",
                reason: DiagnosticReason::ConfigurationConflict,
            },
        ),
        (
            ZedDesktopError::PendingRecovery,
            Expected {
                code: "NH-ZED-002",
                reason: DiagnosticReason::ConfigurationConflict,
            },
        ),
        (
            ZedDesktopError::OrphanBackup,
            Expected {
                code: "NH-ZED-002",
                reason: DiagnosticReason::ConfigurationConflict,
            },
        ),
        (
            ZedDesktopError::SettingsChangedBeforeWrite,
            Expected {
                code: "NH-ZED-002",
                reason: DiagnosticReason::ConfigurationConflict,
            },
        ),
        (
            ZedDesktopError::UnmanagedProviderConflict,
            Expected {
                code: "NH-ZED-002",
                reason: DiagnosticReason::ConfigurationConflict,
            },
        ),
        (
            ZedDesktopError::ManagedConfigurationChanged,
            Expected {
                code: "NH-ZED-002",
                reason: DiagnosticReason::ConfigurationConflict,
            },
        ),
        (
            ZedDesktopError::DidNotStart,
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::ProcessStartFailed,
            },
        ),
        (
            ZedDesktopError::Launch(io::Error::from(io::ErrorKind::PermissionDenied)),
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::ProcessStartFailed,
            },
        ),
        (
            ZedDesktopError::DidNotTerminate,
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::ProcessTerminationFailed,
            },
        ),
        (
            ZedDesktopError::Terminate(io::Error::from(io::ErrorKind::TimedOut)),
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::ProcessTerminationFailed,
            },
        ),
        (
            ZedDesktopError::TerminateFailed(None),
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::ProcessTerminationFailed,
            },
        ),
        (
            ZedDesktopError::Gateway(ChatGatewayError::Bridge(BridgeError::NoCompatibleModels)),
            Expected {
                code: "NH-BRIDGE-005",
                reason: DiagnosticReason::BridgeExited,
            },
        ),
        (
            ZedDesktopError::GatewayExited,
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::BridgeExited,
            },
        ),
    ];

    for (error, expected) in cases {
        assert_contract(&error, &expected);
    }
}

#[test]
fn wait_serialization_model_and_configuration_errors_are_typed() {
    let cases = [
        (
            ZedDesktopError::ProcessCheck(io::Error::from(io::ErrorKind::Other)),
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::ProcessWaitFailed,
            },
        ),
        (
            ZedDesktopError::ProcessCheckFailed(Some(9)),
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::ProcessWaitFailed,
            },
        ),
        (
            ZedDesktopError::Wait(io::Error::from(io::ErrorKind::BrokenPipe)),
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::ProcessWaitFailed,
            },
        ),
    ];

    for (error, expected) in cases {
        assert_contract(&error, &expected);
    }
}

#[test]
fn serialization_model_and_configuration_errors_are_typed() {
    let cases = [
        (
            ZedDesktopError::Serialize(invalid_json()),
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::SerializationFailed,
            },
        ),
        (
            ZedDesktopError::ModelUnavailable {
                model: "requested-model".to_owned(),
                available: vec!["available-model".to_owned()],
            },
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::ModelUnavailable,
            },
        ),
        (
            ZedDesktopError::EmptyModelCatalog,
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::ModelCatalogEmpty,
            },
        ),
        (
            ZedDesktopError::ReservedArgument,
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::InvalidConfiguration,
            },
        ),
        (
            ZedDesktopError::SettingsUtf8(invalid_utf8()),
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::InvalidConfiguration,
            },
        ),
        (
            ZedDesktopError::ParseSettings("settings-secret".to_owned()),
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::InvalidConfiguration,
            },
        ),
        (
            ZedDesktopError::SettingsRootNotObject,
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::InvalidConfiguration,
            },
        ),
        (
            ZedDesktopError::SettingsFieldNotObject("agent.default_model"),
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::InvalidConfiguration,
            },
        ),
        (
            ZedDesktopError::InvalidDefaultModel,
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::InvalidConfiguration,
            },
        ),
        (
            ZedDesktopError::GenerateSettings("settings-secret".to_owned()),
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::InvalidConfiguration,
            },
        ),
        (
            ZedDesktopError::ParseReceipt(invalid_json()),
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::InvalidConfiguration,
            },
        ),
        (
            ZedDesktopError::InvalidReceipt,
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::InvalidConfiguration,
            },
        ),
        (
            ZedDesktopError::InvalidWorkspace,
            Expected {
                code: "NH-ZED-003",
                reason: DiagnosticReason::InvalidConfiguration,
            },
        ),
    ];

    for (error, expected) in cases {
        assert_contract(&error, &expected);
    }
}

#[test]
fn filesystem_and_state_errors_are_general_and_privacy_safe() {
    let private_path = "/private/user/zed/provider-secret/settings.json";
    let cases = [
        (
            ZedDesktopError::MissingHomeDirectory,
            DiagnosticReason::FilesystemOperationFailed,
        ),
        (
            ZedDesktopError::MissingStateDirectory,
            DiagnosticReason::FilesystemOperationFailed,
        ),
        (
            ZedDesktopError::MissingPlatformDirectory,
            DiagnosticReason::FilesystemOperationFailed,
        ),
        (
            ZedDesktopError::InvalidPath,
            DiagnosticReason::FilesystemOperationFailed,
        ),
        (
            ZedDesktopError::ReadSettings(io::Error::new(
                io::ErrorKind::PermissionDenied,
                private_path,
            )),
            DiagnosticReason::FilesystemOperationFailed,
        ),
        (
            ZedDesktopError::ReadBackup(io::Error::new(io::ErrorKind::NotFound, private_path)),
            DiagnosticReason::FilesystemOperationFailed,
        ),
        (
            ZedDesktopError::BackupHashMismatch,
            DiagnosticReason::FilesystemOperationFailed,
        ),
        (
            ZedDesktopError::RemoveBackup(io::Error::new(
                io::ErrorKind::PermissionDenied,
                private_path,
            )),
            DiagnosticReason::FilesystemOperationFailed,
        ),
        (
            ZedDesktopError::BindGateway(io::Error::new(io::ErrorKind::AddrInUse, private_path)),
            DiagnosticReason::FilesystemOperationFailed,
        ),
        (
            ZedDesktopError::State(DesktopStateError::Io(io::Error::new(
                io::ErrorKind::PermissionDenied,
                private_path,
            ))),
            DiagnosticReason::FilesystemOperationFailed,
        ),
    ];

    for (error, reason) in cases {
        let expected = Expected {
            code: "NH-ZED-003",
            reason,
        };
        assert_contract(&error, &expected);
    }

    let diagnostic = ZedDesktopError::ReadSettings(io::Error::new(
        io::ErrorKind::PermissionDenied,
        private_path,
    ))
    .diagnostic();
    assert_eq!(
        serde_json::to_value(&diagnostic).expect("diagnostic should serialize"),
        json!({
            "reason": "filesystem-operation-failed",
            "details": { "kind": "general" }
        })
    );
    let serialized = serde_json::to_string(&diagnostic).expect("diagnostic should serialize");
    assert!(!serialized.contains(private_path));
    assert!(!serialized.contains("provider-secret"));
}
