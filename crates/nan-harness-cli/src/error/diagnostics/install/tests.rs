use super::typed;
use crate::commands::install::InstallError;
use nan_harness_core::HarnessKind;
use nan_harness_telemetry::diagnostic::{
    DiagnosticDetails, DiagnosticOperation, DiagnosticReason, IoErrorKind, VersionComponent,
};
use semver::Version;
use std::io;

const FAKE_PATH: &str = "/private/install-fixture/fake-installer.sh";
const FAKE_MESSAGE: &str = "fixture-only sensitive install message";
const FAKE_TOKEN: &str = "fixture-token-must-not-appear";
const FAKE_RAW_COMMAND: &str = "node --version fixture-token-must-not-appear";
const FAKE_INSTALLER_URL: &str = "https://install-fixture.invalid/fake-installer.sh";
const FAKE_INTERPRETER: &str = "fake-interpreter";
const FAKE_PROGRAM: &str = "fake-installer-program";

fn sensitive_io_error(kind: io::ErrorKind) -> io::Error {
    io::Error::new(kind, format!("{FAKE_MESSAGE}: {FAKE_PATH}"))
}

fn io_diagnostic(
    reason: DiagnosticReason,
    operation: DiagnosticOperation,
    kind: IoErrorKind,
) -> (DiagnosticReason, DiagnosticDetails) {
    (
        reason,
        DiagnosticDetails::Io {
            operation,
            error_kind: kind,
        },
    )
}

fn process_diagnostic(
    operation: DiagnosticOperation,
    exit_code: Option<i32>,
) -> (DiagnosticReason, DiagnosticDetails) {
    (
        DiagnosticReason::ProcessExited,
        DiagnosticDetails::Process {
            operation,
            exit_code,
        },
    )
}

fn assert_safe(
    error: &InstallError,
    expected_reason: DiagnosticReason,
    expected_details: &DiagnosticDetails,
) {
    let diagnostic = typed(error);
    assert_eq!(diagnostic.reason(), expected_reason);
    assert_eq!(diagnostic.details(), expected_details);

    let serialized = serde_json::to_string(&diagnostic).expect("diagnostic should serialize");
    for sensitive in [
        FAKE_PATH,
        FAKE_MESSAGE,
        FAKE_TOKEN,
        FAKE_RAW_COMMAND,
        FAKE_INSTALLER_URL,
        FAKE_INTERPRETER,
        FAKE_PROGRAM,
    ] {
        assert!(
            !serialized.contains(sensitive),
            "diagnostic leaked fixture value {sensitive:?}: {serialized}"
        );
    }
}

#[test]
fn install_configuration_errors_map_to_invalid_configuration() {
    let cases = [
        (
            InstallError::UnsupportedPlatform(HarnessKind::Pi),
            (
                DiagnosticReason::InvalidConfiguration,
                DiagnosticDetails::General,
            ),
        ),
        (
            InstallError::UnsupportedHarness(HarnessKind::Aider),
            (
                DiagnosticReason::InvalidConfiguration,
                DiagnosticDetails::General,
            ),
        ),
        (
            InstallError::CompatibilityManifest(FAKE_MESSAGE.to_owned()),
            (
                DiagnosticReason::InvalidConfiguration,
                DiagnosticDetails::General,
            ),
        ),
        (
            InstallError::InvalidRuntimeCommand {
                harness: HarnessKind::DeepSeekHarness,
                command: FAKE_RAW_COMMAND.to_owned(),
            },
            (
                DiagnosticReason::InvalidConfiguration,
                DiagnosticDetails::General,
            ),
        ),
    ];

    for (error, (reason, details)) in cases {
        assert_safe(&error, reason, &details);
    }
}

#[test]
fn installer_start_errors_map_to_typed_io_diagnostics() {
    let cases = [
        (
            InstallError::Prompt(sensitive_io_error(io::ErrorKind::InvalidInput)),
            (
                DiagnosticReason::UserPromptFailed,
                DiagnosticDetails::Io {
                    operation: DiagnosticOperation::RunInstaller,
                    error_kind: IoErrorKind::InvalidInput,
                },
            ),
        ),
        (
            InstallError::RuntimeCommandStart {
                harness: HarnessKind::DeepSeekHarness,
                command: FAKE_RAW_COMMAND.to_owned(),
                minimum: Version::new(22, 19, 0),
                hint: FAKE_PATH.to_owned(),
                source: sensitive_io_error(io::ErrorKind::InvalidInput),
            },
            io_diagnostic(
                DiagnosticReason::FilesystemOperationFailed,
                DiagnosticOperation::RunInstaller,
                IoErrorKind::InvalidInput,
            ),
        ),
        (
            InstallError::DownloadStart {
                harness: HarnessKind::KimiCode,
                url: FAKE_INSTALLER_URL,
                source: sensitive_io_error(io::ErrorKind::TimedOut),
            },
            io_diagnostic(
                DiagnosticReason::FilesystemOperationFailed,
                DiagnosticOperation::DownloadInstaller,
                IoErrorKind::TimedOut,
            ),
        ),
        (
            InstallError::PrepareInstaller {
                harness: HarnessKind::KimiCode,
                source: sensitive_io_error(io::ErrorKind::PermissionDenied),
            },
            io_diagnostic(
                DiagnosticReason::FilesystemOperationFailed,
                DiagnosticOperation::RunInstaller,
                IoErrorKind::PermissionDenied,
            ),
        ),
        (
            InstallError::InstallerStart {
                harness: HarnessKind::KimiCode,
                interpreter: FAKE_INTERPRETER,
                source: sensitive_io_error(io::ErrorKind::NotFound),
            },
            io_diagnostic(
                DiagnosticReason::FilesystemOperationFailed,
                DiagnosticOperation::RunInstaller,
                IoErrorKind::NotFound,
            ),
        ),
        (
            InstallError::CommandStart {
                harness: HarnessKind::Cline,
                program: FAKE_PROGRAM,
                source: sensitive_io_error(io::ErrorKind::ConnectionRefused),
            },
            io_diagnostic(
                DiagnosticReason::FilesystemOperationFailed,
                DiagnosticOperation::RunInstaller,
                IoErrorKind::ConnectionRefused,
            ),
        ),
        (
            InstallError::PostInstallCheckStart {
                harness: HarnessKind::Cline,
                command: FAKE_RAW_COMMAND.to_owned(),
                source: sensitive_io_error(io::ErrorKind::PermissionDenied),
            },
            io_diagnostic(
                DiagnosticReason::FilesystemOperationFailed,
                DiagnosticOperation::RunPostInstallCheck,
                IoErrorKind::PermissionDenied,
            ),
        ),
        (
            InstallError::PostInstallCheckPrepare {
                harness: HarnessKind::Cline,
                source: sensitive_io_error(io::ErrorKind::InvalidInput),
            },
            io_diagnostic(
                DiagnosticReason::FilesystemOperationFailed,
                DiagnosticOperation::RunPostInstallCheck,
                IoErrorKind::InvalidInput,
            ),
        ),
    ];

    for (error, (reason, details)) in cases {
        assert_safe(&error, reason, &details);
    }
}

#[test]
fn installer_exit_failures_map_to_typed_process_diagnostics() {
    let cases = [
        (
            InstallError::RuntimeCommandFailed {
                harness: HarnessKind::DeepSeekHarness,
                command: FAKE_RAW_COMMAND.to_owned(),
                minimum: Version::new(22, 19, 0),
                exit_code: Some(12),
                hint: FAKE_PATH.to_owned(),
            },
            process_diagnostic(DiagnosticOperation::RunInstaller, Some(12)),
        ),
        (
            InstallError::DownloadFailed {
                harness: HarnessKind::KimiCode,
                exit_code: Some(5),
            },
            process_diagnostic(DiagnosticOperation::DownloadInstaller, Some(5)),
        ),
        (
            InstallError::InstallerFailed {
                harness: HarnessKind::KimiCode,
                interpreter: FAKE_INTERPRETER,
                exit_code: Some(13),
            },
            process_diagnostic(DiagnosticOperation::RunInstaller, Some(13)),
        ),
        (
            InstallError::CommandFailed {
                harness: HarnessKind::Cline,
                program: FAKE_PROGRAM,
                exit_code: Some(14),
            },
            process_diagnostic(DiagnosticOperation::RunInstaller, Some(14)),
        ),
        (
            InstallError::PostInstallCheckFailed {
                harness: HarnessKind::Cline,
                command: FAKE_RAW_COMMAND.to_owned(),
                exit_code: Some(19),
                details: format!("{FAKE_MESSAGE}: {FAKE_PATH}: {FAKE_TOKEN}"),
            },
            process_diagnostic(DiagnosticOperation::RunPostInstallCheck, Some(19)),
        ),
    ];

    for (error, (reason, details)) in cases {
        assert_safe(&error, reason, &details);
    }
}

#[test]
fn runtime_versions_normalize_safe_input_and_omit_unparseable_input() {
    let minimum = Version::new(22, 19, 0);
    let safe_error = InstallError::RuntimeUnsupported {
        harness: HarnessKind::DeepSeekHarness,
        detected: format!("Node.js v20.19.4 {FAKE_PATH} {FAKE_TOKEN}"),
        minimum: minimum.clone(),
        hint: FAKE_MESSAGE.to_owned(),
    };
    assert_safe(
        &safe_error,
        DiagnosticReason::UnsupportedVersion,
        &DiagnosticDetails::Version {
            component: VersionComponent::Runtime,
            detected: Some("20.19.4".to_owned()),
            expected: Some("22.19.0".to_owned()),
        },
    );

    let unsafe_error = InstallError::RuntimeUnparseable {
        harness: HarnessKind::DeepSeekHarness,
        detected: format!("{FAKE_MESSAGE} {FAKE_TOKEN} {FAKE_PATH}"),
        minimum,
        hint: FAKE_PATH.to_owned(),
    };
    assert_safe(
        &unsafe_error,
        DiagnosticReason::UnparseableVersion,
        &DiagnosticDetails::Version {
            component: VersionComponent::Runtime,
            detected: None,
            expected: Some("22.19.0".to_owned()),
        },
    );
}
