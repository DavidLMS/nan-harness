use super::typed;
use crate::commands::configuration::ConfigurationError;
use crate::commands::credentials::CredentialError;
use crate::commands::hermes_desktop::HermesDesktopError;
use crate::commands::pen_desktop::PenDesktopError;
use crate::commands::persistence::PersistenceError;
use crate::commands::uninstall::UninstallError;
use nan_harness_telemetry::diagnostic::{
    Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason, DocumentKind, IoErrorKind,
};
use std::io;
use std::path::PathBuf;

const FAKE_PATH: &str = "/private/uninstall-fixture/fake-secret/config.json";
const FAKE_TOKEN: &str = "fixture-token-must-not-appear";

fn fake_path() -> PathBuf {
    PathBuf::from(FAKE_PATH)
}

fn sensitive_io_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("{FAKE_TOKEN}: {FAKE_PATH}"),
    )
}

fn sensitive_serialization_error() -> serde_json::Error {
    struct Fixture;

    impl serde::Serialize for Fixture {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(<S::Error as serde::ser::Error>::custom(FAKE_TOKEN))
        }
    }

    serde_json::to_string(&Fixture).expect_err("fixture must fail to serialize")
}

fn parse_receipt_error() -> serde_json::Error {
    serde_json::from_str::<serde_json::Value>("{").expect_err("fixture must be invalid JSON")
}

fn io_diagnostic(operation: DiagnosticOperation, kind: IoErrorKind) -> Diagnostic {
    Diagnostic::new(
        DiagnosticReason::FilesystemOperationFailed,
        DiagnosticDetails::Io {
            operation,
            error_kind: kind,
        },
    )
}

fn receipt_diagnostic(reason: DiagnosticReason, observed_version: Option<u16>) -> Diagnostic {
    Diagnostic::new(
        reason,
        DiagnosticDetails::Schema {
            document: DocumentKind::InstallationReceipt,
            observed_version,
        },
    )
}

fn assert_safe(error: &UninstallError, expected: &Diagnostic) {
    let diagnostic = typed(error);
    assert_eq!(&diagnostic, expected);

    let serialized = serde_json::to_string(&diagnostic).expect("diagnostic should serialize");
    for sensitive in [FAKE_PATH, FAKE_TOKEN] {
        assert!(
            !serialized.contains(sensitive),
            "diagnostic leaked fixture value {sensitive:?}: {serialized}"
        );
    }
}

fn assert_cases(cases: impl IntoIterator<Item = (UninstallError, Diagnostic)>) {
    for (error, expected) in cases {
        assert_safe(&error, &expected);
    }
}

#[test]
fn uninstall_safety_and_path_conflicts_map_to_closed_diagnostics() {
    assert_cases([
        (
            UninstallError::ConfirmationRequired,
            Diagnostic::general(DiagnosticReason::InvalidConfiguration),
        ),
        (
            UninstallError::DesktopRecoveryRequired(FAKE_TOKEN),
            Diagnostic::general(DiagnosticReason::InvalidConfiguration),
        ),
        (
            UninstallError::InstallationNotManaged,
            Diagnostic::general(DiagnosticReason::ConfigurationConflict),
        ),
        (
            UninstallError::ExecutableMismatch {
                expected: fake_path(),
                actual: fake_path(),
            },
            Diagnostic::general(DiagnosticReason::ConfigurationConflict),
        ),
        (
            UninstallError::UnsafeInstallationPath(fake_path()),
            Diagnostic::general(DiagnosticReason::ConfigurationConflict),
        ),
        (
            UninstallError::UnsafeAliasPath(fake_path()),
            Diagnostic::general(DiagnosticReason::ConfigurationConflict),
        ),
        (
            UninstallError::UnsafeDataDirectory(fake_path()),
            Diagnostic::general(DiagnosticReason::ConfigurationConflict),
        ),
    ]);
}

#[test]
fn uninstall_filesystem_errors_preserve_removal_io_details() {
    let expected = io_diagnostic(
        DiagnosticOperation::RemoveInstallation,
        IoErrorKind::PermissionDenied,
    );
    assert_cases([
        (
            UninstallError::CurrentExecutable(sensitive_io_error()),
            expected.clone(),
        ),
        (
            UninstallError::CanonicalizeExecutable {
                path: fake_path(),
                source: sensitive_io_error(),
            },
            expected.clone(),
        ),
        (
            UninstallError::InspectDataDirectory {
                path: fake_path(),
                source: sensitive_io_error(),
            },
            expected.clone(),
        ),
        (
            UninstallError::InspectAlias {
                path: fake_path(),
                source: sensitive_io_error(),
            },
            expected.clone(),
        ),
        (
            UninstallError::ReadReceipt {
                path: fake_path(),
                source: sensitive_io_error(),
            },
            expected.clone(),
        ),
        (
            UninstallError::CreateDataDirectory {
                path: fake_path(),
                source: sensitive_io_error(),
            },
            expected.clone(),
        ),
        (
            UninstallError::WriteReceipt {
                path: fake_path(),
                source: sensitive_io_error(),
            },
            expected.clone(),
        ),
        (UninstallError::Prompt(sensitive_io_error()), expected),
    ]);
}

#[test]
fn uninstall_receipt_errors_map_to_schema_and_serialization_diagnostics() {
    assert_cases([
        (
            UninstallError::ParseReceipt(parse_receipt_error()),
            receipt_diagnostic(DiagnosticReason::InvalidConfiguration, None),
        ),
        (
            UninstallError::UnsupportedReceiptSchema(9),
            receipt_diagnostic(DiagnosticReason::UnsupportedVersion, Some(9)),
        ),
        (
            UninstallError::SerializeReceipt(sensitive_serialization_error()),
            Diagnostic::general(DiagnosticReason::SerializationFailed),
        ),
    ]);
}

#[test]
fn delegated_uninstall_errors_map_to_safe_typed_diagnostics() {
    assert_cases([
        (
            UninstallError::Configuration(ConfigurationError::UnmanagedDocumentConflict(
                fake_path(),
            )),
            Diagnostic::general(DiagnosticReason::InvalidConfiguration),
        ),
        (
            UninstallError::Credential(CredentialError::InvalidBackend(FAKE_TOKEN.to_owned())),
            Diagnostic::general(DiagnosticReason::InvalidConfiguration),
        ),
        (
            UninstallError::Persistence(PersistenceError::ReadFile {
                path: fake_path(),
                source: sensitive_io_error(),
            }),
            io_diagnostic(
                DiagnosticOperation::ReadConfiguration,
                IoErrorKind::PermissionDenied,
            ),
        ),
        (
            UninstallError::HermesDesktop(HermesDesktopError::ModelUnavailable {
                model: FAKE_TOKEN.to_owned(),
                available: vec![FAKE_TOKEN.to_owned()],
            }),
            Diagnostic::general(DiagnosticReason::ModelUnavailable),
        ),
        (
            UninstallError::PenDesktop(PenDesktopError::ReadDocument {
                path: fake_path(),
                source: sensitive_io_error(),
            }),
            Diagnostic::general(DiagnosticReason::FilesystemOperationFailed),
        ),
    ]);
}

#[test]
#[cfg(not(windows))]
fn unix_removal_errors_preserve_removal_io_details() {
    assert_cases([
        (
            UninstallError::RemoveFile {
                path: fake_path(),
                source: sensitive_io_error(),
            },
            io_diagnostic(
                DiagnosticOperation::RemoveInstallation,
                IoErrorKind::PermissionDenied,
            ),
        ),
        (
            UninstallError::RemoveDataDirectory {
                path: fake_path(),
                source: sensitive_io_error(),
            },
            io_diagnostic(
                DiagnosticOperation::RemoveInstallation,
                IoErrorKind::PermissionDenied,
            ),
        ),
    ]);
}

#[test]
#[cfg(windows)]
fn windows_helper_errors_preserve_removal_io_details() {
    assert_cases([
        (
            UninstallError::CreateHelper(sensitive_io_error()),
            io_diagnostic(
                DiagnosticOperation::RemoveInstallation,
                IoErrorKind::PermissionDenied,
            ),
        ),
        (
            UninstallError::StartHelper(sensitive_io_error()),
            io_diagnostic(
                DiagnosticOperation::RemoveInstallation,
                IoErrorKind::PermissionDenied,
            ),
        ),
    ]);
}
