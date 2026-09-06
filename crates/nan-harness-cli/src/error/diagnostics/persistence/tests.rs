use super::typed;
use crate::commands::persistence::PersistenceError;
use nan_harness_core::SecretError;
use nan_harness_telemetry::diagnostic::{
    Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason, DocumentKind, IoErrorKind,
};
use std::io;
use std::path::PathBuf;

const FAKE_PATH: &str = "/private/diagnostic-fixture/persistence-secret.toml";
const FAKE_TOKEN: &str = "fixture_persistence_token";
const FAKE_SOURCE: &str = "fixture persistence source text";

macro_rules! assert_cases {
    ($(($error:expr, $expected:expr, $injected_values:expr $(,)?)),+ $(,)?) => {
        $(
            let error = $error;
            let expected = $expected;
            assert_diagnostic(&error, &expected, $injected_values);
        )+
    };
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

fn assert_diagnostic(error: &PersistenceError, expected: &Diagnostic, injected_values: &[&str]) {
    let rendered = error.to_string();
    for value in injected_values {
        assert!(
            rendered.contains(value),
            "fixture value {value:?} was not present in source error: {rendered}"
        );
    }

    let diagnostic = typed(error);
    assert_eq!(&diagnostic, expected);

    let serialized = serde_json::to_string(&diagnostic).expect("diagnostic should serialize");
    for value in injected_values {
        assert!(
            !serialized.contains(value),
            "diagnostic leaked fixture value {value:?}: {serialized}"
        );
    }
}

fn fake_path() -> PathBuf {
    PathBuf::from(FAKE_PATH)
}

fn fake_io(kind: io::ErrorKind) -> io::Error {
    io::Error::new(kind, FAKE_SOURCE)
}

macro_rules! directory_io_conflict_cases {
    () => {
        let invalid_utf8 =
            String::from_utf8(vec![0xff]).expect_err("fixture must be invalid UTF-8");
        let invalid_json =
            serde_json::from_str::<serde_json::Value>("{").expect_err("invalid JSON");
        let invalid_json_with_source = serde_json::from_str::<serde_json::Value>(FAKE_SOURCE)
            .expect_err("fixture must be invalid JSON");

        assert_cases! {
            (
                PersistenceError::MissingConfigDirectory,
                Diagnostic::general(DiagnosticReason::MissingDirectory),
                &[][..],
            ),
            (
                PersistenceError::MissingHomeDirectory,
                Diagnostic::general(DiagnosticReason::MissingDirectory),
                &[][..],
            ),
            (
                PersistenceError::RenderConfiguration(FAKE_SOURCE.to_owned()),
                Diagnostic::general(DiagnosticReason::InvalidConfiguration),
                &[FAKE_SOURCE][..],
            ),
            (
                PersistenceError::CreateDirectory {
                    path: fake_path(),
                    source: fake_io(io::ErrorKind::NotFound),
                },
                io_diagnostic(
                    DiagnosticOperation::WriteConfiguration,
                    IoErrorKind::NotFound,
                ),
                &[FAKE_PATH, FAKE_SOURCE][..],
            ),
            (
                PersistenceError::ReadFile {
                    path: fake_path(),
                    source: fake_io(io::ErrorKind::PermissionDenied),
                },
                io_diagnostic(
                    DiagnosticOperation::ReadConfiguration,
                    IoErrorKind::PermissionDenied,
                ),
                &[FAKE_PATH, FAKE_SOURCE][..],
            ),
            (
                PersistenceError::WriteFile {
                    path: fake_path(),
                    source: fake_io(io::ErrorKind::ReadOnlyFilesystem),
                },
                io_diagnostic(
                    DiagnosticOperation::WriteConfiguration,
                    IoErrorKind::Other,
                ),
                &[FAKE_PATH, FAKE_SOURCE][..],
            ),
            (
                PersistenceError::RemoveFile {
                    path: fake_path(),
                    source: fake_io(io::ErrorKind::IsADirectory),
                },
                io_diagnostic(
                    DiagnosticOperation::RemoveConfiguration,
                    IoErrorKind::Other,
                ),
                &[FAKE_PATH, FAKE_SOURCE][..],
            ),
            (
                PersistenceError::InvalidPath(fake_path()),
                Diagnostic::general(DiagnosticReason::InvalidConfiguration),
                &[FAKE_PATH][..],
            ),
            (
                PersistenceError::InvalidUtf8 {
                    path: fake_path(),
                    source: invalid_utf8,
                },
                Diagnostic::general(DiagnosticReason::InvalidConfiguration),
                &[FAKE_PATH][..],
            ),
            (
                PersistenceError::InvalidReceiptPath(FAKE_TOKEN.to_owned()),
                Diagnostic::general(DiagnosticReason::InvalidConfiguration),
                &[FAKE_TOKEN][..],
            ),
            (
                PersistenceError::ManagedFileChanged(fake_path()),
                Diagnostic::general(DiagnosticReason::ConfigurationConflict),
                &[FAKE_PATH][..],
            ),
            (
                PersistenceError::AmbiguousOpenCodeConfig(fake_path()),
                Diagnostic::general(DiagnosticReason::ConfigurationConflict),
                &[FAKE_PATH][..],
            ),
            (
                PersistenceError::RootIsNotObject(fake_path()),
                Diagnostic::general(DiagnosticReason::InvalidConfiguration),
                &[FAKE_PATH][..],
            ),
            (
                PersistenceError::ProviderIsNotObject(fake_path()),
                Diagnostic::general(DiagnosticReason::InvalidConfiguration),
                &[FAKE_PATH][..],
            ),
            (
                PersistenceError::InvalidManagedProvider(fake_path()),
                Diagnostic::general(DiagnosticReason::InvalidConfiguration),
                &[FAKE_PATH][..],
            ),
            (
                PersistenceError::UnmanagedProviderConflict(fake_path()),
                Diagnostic::general(DiagnosticReason::ConfigurationConflict),
                &[FAKE_PATH][..],
            ),
            (
                PersistenceError::ManagedProviderChanged(fake_path()),
                Diagnostic::general(DiagnosticReason::ConfigurationConflict),
                &[FAKE_PATH][..],
            ),
            (
                PersistenceError::InvalidManagedSection(fake_path()),
                Diagnostic::general(DiagnosticReason::InvalidConfiguration),
                &[FAKE_PATH][..],
            ),
            (
                PersistenceError::UnmanagedSectionConflict(fake_path()),
                Diagnostic::general(DiagnosticReason::ConfigurationConflict),
                &[FAKE_PATH][..],
            ),
            (
                PersistenceError::ManagedSectionChanged(fake_path()),
                Diagnostic::general(DiagnosticReason::ConfigurationConflict),
                &[FAKE_PATH][..],
            ),
            (
                PersistenceError::InvalidManagedBlock,
                Diagnostic::general(DiagnosticReason::InvalidConfiguration),
                &[][..],
            ),
            (
                PersistenceError::ConfigRootIsNotObject {
                    harness: "fixture",
                    path: fake_path(),
                },
                Diagnostic::general(DiagnosticReason::InvalidConfiguration),
                &[FAKE_PATH][..],
            ),
            (
                PersistenceError::ConfigFieldIsNotObject {
                    harness: "fixture",
                    field: "provider",
                    path: fake_path(),
                },
                Diagnostic::general(DiagnosticReason::InvalidConfiguration),
                &[FAKE_PATH][..],
            ),
            (
                PersistenceError::ParseHarnessConfig {
                    harness: "fixture",
                    path: fake_path(),
                    message: FAKE_SOURCE.to_owned(),
                },
                Diagnostic::general(DiagnosticReason::InvalidConfiguration),
                &[FAKE_PATH, FAKE_SOURCE][..],
            ),
            (
                PersistenceError::ParseOpenCodeConfig {
                    path: fake_path(),
                    message: FAKE_SOURCE.to_owned(),
                },
                Diagnostic::general(DiagnosticReason::InvalidConfiguration),
                &[FAKE_PATH, FAKE_SOURCE][..],
            ),
            (
                PersistenceError::GenerateOpenCodeProvider(FAKE_SOURCE.to_owned()),
                Diagnostic::general(DiagnosticReason::InvalidConfiguration),
                &[FAKE_SOURCE][..],
            ),
            (
                PersistenceError::SerializeProvider(invalid_json_with_source),
                Diagnostic::general(DiagnosticReason::SerializationFailed),
                &[][..],
            ),
        (
            PersistenceError::SerializeProvider(invalid_json),
            Diagnostic::general(DiagnosticReason::SerializationFailed),
            &[],
        )
        }
    };
}

#[test]
fn persistence_diagnostics_cover_directory_io_conflicts_and_configuration() {
    directory_io_conflict_cases!();
}

macro_rules! model_secret_state_cases {
    () => {
        let invalid_json =
            serde_json::from_str::<serde_json::Value>("{").expect_err("invalid JSON");
        let invalid_proxy = reqwest::Proxy::all("http://[").expect_err("proxy URL must be invalid");
        let invalid_proxy_for_discovery =
            reqwest::Proxy::all("http://[").expect_err("proxy URL must be invalid");

        assert_cases! {
            (
                PersistenceError::BuildClient(invalid_proxy),
                Diagnostic::general(DiagnosticReason::NetworkRequestFailed),
                &[][..],
            ),
            (
                PersistenceError::DiscoverModels(invalid_proxy_for_discovery),
                Diagnostic::general(DiagnosticReason::NetworkRequestFailed),
                &[][..],
            ),
            (
                PersistenceError::ModelDiscoveryStatus(503),
                Diagnostic::new(
                    DiagnosticReason::HttpRequestRejected,
                    DiagnosticDetails::Http {
                        operation: DiagnosticOperation::DiscoverModels,
                        status: 503,
                    },
                ),
                &[][..],
            ),
            (
                PersistenceError::ModelDiscoveryTooLarge,
                Diagnostic::general(DiagnosticReason::InvalidResponse),
                &[][..],
            ),
            (
                PersistenceError::ParseModels(invalid_json),
                Diagnostic::general(DiagnosticReason::InvalidResponse),
                &[][..],
            ),
            (
                PersistenceError::NoModels,
                Diagnostic::general(DiagnosticReason::ModelCatalogEmpty),
                &[][..],
            ),
            (
                PersistenceError::Secret(SecretError::MissingReference(FAKE_TOKEN.to_owned())),
                Diagnostic::general(DiagnosticReason::SecretResolutionFailed),
                &[FAKE_TOKEN][..],
            ),
            (
                PersistenceError::CreateStateDirectory(fake_io(io::ErrorKind::NotFound)),
                io_diagnostic(
                    DiagnosticOperation::WriteConfiguration,
                    IoErrorKind::NotFound,
                ),
                &[FAKE_SOURCE][..],
            ),
            (
                PersistenceError::ReadState(fake_io(io::ErrorKind::PermissionDenied)),
                io_diagnostic(
                    DiagnosticOperation::ReadConfiguration,
                    IoErrorKind::PermissionDenied,
                ),
                &[FAKE_SOURCE][..],
            ),
            (
                PersistenceError::ParseState(
                    serde_json::from_str::<serde_json::Value>("{").expect_err("invalid JSON"),
                ),
                Diagnostic::general(DiagnosticReason::InvalidConfiguration),
                &[][..],
            ),
            (
                PersistenceError::UnsupportedStateSchema(9),
                Diagnostic::new(
                    DiagnosticReason::UnsupportedVersion,
                    DiagnosticDetails::Schema {
                        document: DocumentKind::IntegrationState,
                        observed_version: Some(9),
                    },
                ),
                &[][..],
            ),
            (
                PersistenceError::SerializeState(
                    serde_json::from_str::<serde_json::Value>("{").expect_err("invalid JSON"),
                ),
                Diagnostic::general(DiagnosticReason::SerializationFailed),
                &[][..],
            ),
            (
                PersistenceError::ReadPreferences(fake_io(io::ErrorKind::NotFound)),
                io_diagnostic(
                    DiagnosticOperation::ReadConfiguration,
                    IoErrorKind::NotFound,
                ),
                &[FAKE_SOURCE][..],
            ),
            (
                PersistenceError::ParsePreferences(
                    serde_json::from_str::<serde_json::Value>("{").expect_err("invalid JSON"),
                ),
                Diagnostic::general(DiagnosticReason::InvalidConfiguration),
                &[][..],
            ),
            (
                PersistenceError::UnsupportedPreferencesSchema(8),
                Diagnostic::new(
                    DiagnosticReason::UnsupportedVersion,
                    DiagnosticDetails::Schema {
                        document: DocumentKind::IntegrationState,
                        observed_version: Some(8),
                    },
                ),
                &[][..],
            ),
            (
                PersistenceError::SerializePreferences(
                    serde_json::from_str::<serde_json::Value>("{").expect_err("invalid JSON"),
                ),
                Diagnostic::general(DiagnosticReason::SerializationFailed),
                &[][..],
            ),
        }
    };
}

#[test]
fn persistence_diagnostics_cover_model_discovery_secrets_and_state_documents() {
    model_secret_state_cases!();
}
