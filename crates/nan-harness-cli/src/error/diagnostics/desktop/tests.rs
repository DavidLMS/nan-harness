use super::{chatgpt, claude};
use crate::commands::chatgpt_desktop::ChatGptDesktopError;
use crate::commands::claude_desktop::ClaudeDesktopError;
use nan_harness_runtime::{BridgeError, DesktopCompatibilityError};
use nan_harness_telemetry::diagnostic::{
    Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason, IoErrorKind,
};
use semver::Version;
use std::io;

const SENSITIVE_PATH: &str = "/private/desktop/provider-token/config.json";
const SENSITIVE_MESSAGE: &str = "raw desktop error with provider-token";

fn assert_diagnostic(diagnostic: &Diagnostic, expected: &Diagnostic) {
    assert_eq!(diagnostic, expected);
    let serialized = serde_json::to_string(&diagnostic).expect("diagnostic should serialize");
    assert!(
        !serialized.contains(SENSITIVE_PATH),
        "path leaked: {serialized}"
    );
    assert!(
        !serialized.contains(SENSITIVE_MESSAGE),
        "raw error leaked: {serialized}"
    );
}

fn io_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("{SENSITIVE_MESSAGE}: {SENSITIVE_PATH}"),
    )
}

fn general(reason: DiagnosticReason) -> Diagnostic {
    Diagnostic::general(reason)
}

fn io_diagnostic(operation: DiagnosticOperation) -> Diagnostic {
    Diagnostic::new(
        DiagnosticReason::FilesystemOperationFailed,
        DiagnosticDetails::Io {
            operation,
            error_kind: IoErrorKind::PermissionDenied,
        },
    )
}

fn process_diagnostic(
    reason: DiagnosticReason,
    operation: DiagnosticOperation,
    exit_code: Option<i32>,
) -> Diagnostic {
    Diagnostic::new(
        reason,
        DiagnosticDetails::Process {
            operation,
            exit_code,
        },
    )
}

#[test]
fn chatgpt_desktop_errors_map_to_safe_typed_diagnostics() {
    let cases = [
        (
            ChatGptDesktopError::UnsupportedPlatform,
            general(DiagnosticReason::UnsupportedVersion),
        ),
        (
            ChatGptDesktopError::OlderUnsupported {
                minimum_app: Version::new(1, 0, 0),
                minimum_codex: Version::new(2, 0, 0),
            },
            general(DiagnosticReason::UnsupportedVersion),
        ),
        (
            ChatGptDesktopError::AppNotFound,
            general(DiagnosticReason::MissingExecutable),
        ),
        (
            ChatGptDesktopError::InvalidInstallation,
            general(DiagnosticReason::InvalidExecutable),
        ),
        (
            ChatGptDesktopError::VersionCommand(io_error()),
            io_diagnostic(DiagnosticOperation::RunVersionCommand),
        ),
        (
            ChatGptDesktopError::VersionCommandFailed,
            process_diagnostic(
                DiagnosticReason::ProcessExited,
                DiagnosticOperation::RunVersionCommand,
                None,
            ),
        ),
        (
            ChatGptDesktopError::UnparseableVersion,
            general(DiagnosticReason::UnparseableVersion),
        ),
        (
            ChatGptDesktopError::AppAlreadyRunning,
            general(DiagnosticReason::ConfigurationConflict),
        ),
        (
            ChatGptDesktopError::AppDidNotTerminate,
            general(DiagnosticReason::ProcessTerminationFailed),
        ),
        (
            ChatGptDesktopError::AppExitedDuringStartup,
            general(DiagnosticReason::ProcessExited),
        ),
        (
            ChatGptDesktopError::InspectProcess(io_error()),
            io_diagnostic(DiagnosticOperation::WaitForHarness),
        ),
        (
            ChatGptDesktopError::ProcessInspectionFailed,
            general(DiagnosticReason::ProcessWaitFailed),
        ),
        (
            ChatGptDesktopError::Bridge(BridgeError::NoCompatibleModels.into()),
            general(DiagnosticReason::BridgeExited),
        ),
        (
            ChatGptDesktopError::BridgeHandshakeTimeout,
            general(DiagnosticReason::AuthenticationRejected),
        ),
        (
            ChatGptDesktopError::StartApp(io_error()),
            io_diagnostic(DiagnosticOperation::StartHarness),
        ),
    ];

    for (error, expected) in cases {
        assert_diagnostic(&chatgpt(&error), &expected);
    }
}

#[test]
fn chatgpt_desktop_profile_state_errors_map_to_safe_typed_diagnostics() {
    let cases = [
        (
            ChatGptDesktopError::BackupHashMismatch,
            general(DiagnosticReason::ConfigurationConflict),
        ),
        (
            ChatGptDesktopError::MissingBackup,
            general(DiagnosticReason::ConfigurationConflict),
        ),
        (
            ChatGptDesktopError::MalformedConfig,
            general(DiagnosticReason::InvalidConfiguration),
        ),
        (
            ChatGptDesktopError::IncompatibleConfigSetting,
            general(DiagnosticReason::InvalidConfiguration),
        ),
        (
            ChatGptDesktopError::State(crate::commands::desktop::DesktopStateError::Io(io_error())),
            general(DiagnosticReason::FilesystemOperationFailed),
        ),
        (
            ChatGptDesktopError::InspectProfile(io_error()),
            io_diagnostic(DiagnosticOperation::ReadConfiguration),
        ),
        (
            ChatGptDesktopError::WriteState(io_error()),
            io_diagnostic(DiagnosticOperation::WriteConfiguration),
        ),
        (
            ChatGptDesktopError::ParseMarker(
                serde_json::from_str::<serde_json::Value>("{").unwrap_err(),
            ),
            general(DiagnosticReason::InvalidConfiguration),
        ),
        (
            ChatGptDesktopError::SerializeState(
                serde_json::from_str::<serde_json::Value>("{").unwrap_err(),
            ),
            general(DiagnosticReason::SerializationFailed),
        ),
    ];

    for (error, expected) in cases {
        assert_diagnostic(&chatgpt(&error), &expected);
    }
}

#[test]
fn claude_desktop_errors_map_to_safe_typed_diagnostics() {
    let cases = [
        (
            ClaudeDesktopError::UnsupportedPlatform,
            general(DiagnosticReason::UnsupportedVersion),
        ),
        (
            ClaudeDesktopError::Compatibility(DesktopCompatibilityError::Unavailable),
            general(DiagnosticReason::UnsupportedVersion),
        ),
        (
            ClaudeDesktopError::AppNotFound {
                platform: "synthetic-platform",
            },
            general(DiagnosticReason::MissingExecutable),
        ),
        (
            ClaudeDesktopError::AlreadyRunning,
            general(DiagnosticReason::ConfigurationConflict),
        ),
        (
            ClaudeDesktopError::DidNotStart,
            general(DiagnosticReason::ProcessStartFailed),
        ),
        (
            ClaudeDesktopError::DidNotTerminate,
            general(DiagnosticReason::ProcessTerminationFailed),
        ),
        (
            ClaudeDesktopError::Bridge(BridgeError::NoCompatibleModels.into()),
            general(DiagnosticReason::BridgeExited),
        ),
        (
            ClaudeDesktopError::MissingHome,
            general(DiagnosticReason::MissingDirectory),
        ),
        (
            ClaudeDesktopError::CreateDirectory(io_error()),
            io_diagnostic(DiagnosticOperation::WriteConfiguration),
        ),
        (
            ClaudeDesktopError::ReadConfig(io_error()),
            io_diagnostic(DiagnosticOperation::ReadConfiguration),
        ),
        (
            ClaudeDesktopError::ProcessCheck(io_error()),
            io_diagnostic(DiagnosticOperation::WaitForHarness),
        ),
        (
            ClaudeDesktopError::ProcessCheckFailed(Some(23)),
            process_diagnostic(
                DiagnosticReason::ProcessWaitFailed,
                DiagnosticOperation::WaitForHarness,
                Some(23),
            ),
        ),
        (
            ClaudeDesktopError::Launch(io_error()),
            io_diagnostic(DiagnosticOperation::StartHarness),
        ),
        (
            ClaudeDesktopError::LaunchFailed(None),
            process_diagnostic(
                DiagnosticReason::ProcessStartFailed,
                DiagnosticOperation::StartHarness,
                None,
            ),
        ),
        (
            ClaudeDesktopError::Terminate(io_error()),
            io_diagnostic(DiagnosticOperation::StopHarness),
        ),
        (
            ClaudeDesktopError::TerminateFailed(Some(24)),
            process_diagnostic(
                DiagnosticReason::ProcessTerminationFailed,
                DiagnosticOperation::StopHarness,
                Some(24),
            ),
        ),
        (
            ClaudeDesktopError::ParseConfig(
                serde_json::from_str::<serde_json::Value>("{").unwrap_err(),
            ),
            general(DiagnosticReason::InvalidConfiguration),
        ),
        (
            ClaudeDesktopError::SerializeConfig(
                serde_json::from_str::<serde_json::Value>("{").unwrap_err(),
            ),
            general(DiagnosticReason::SerializationFailed),
        ),
        (
            ClaudeDesktopError::Restore(io_error()),
            io_diagnostic(DiagnosticOperation::RemoveConfiguration),
        ),
    ];

    for (error, expected) in cases {
        assert_diagnostic(&claude(&error), &expected);
    }
}
