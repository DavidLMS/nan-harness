use super::compat_diagnostic;
use nan_harness_core::HarnessKind;
use nan_harness_runtime::CompatibilityError;
use nan_harness_telemetry::diagnostic::{
    Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason, DocumentKind, IoErrorKind,
};
use semver::Version;
use std::io;
use std::time::{Duration, SystemTime, SystemTimeError};

const FAKE_COMPATIBILITY_ID: &str = "placeholder-private-compatibility-id";
const FAKE_COMPATIBILITY_PATH: &str = "/private/placeholder/compatibility-state";
const FAKE_EVIDENCE_TIMESTAMP: &str = "placeholder-private-evidence-timestamp";
const FAKE_FETCH_URL: &str =
    "file:///private/placeholder/compatibility?token=placeholder-compatibility-token";

struct CompatibilityDiagnosticCase {
    error: CompatibilityError,
    expected: Diagnostic,
    injected_values: &'static [&'static str],
}

#[test]
fn compatibility_diagnostics_cover_every_typed_variant() {
    for case in compatibility_diagnostic_cases() {
        for injected_value in case.injected_values {
            assert!(
                case.error.to_string().contains(injected_value),
                "source error should retain the injected value before telemetry mapping"
            );
        }

        let actual = compat_diagnostic(&case.error);
        assert_eq!(
            actual, case.expected,
            "diagnostic mapping should preserve the expected reason and typed details"
        );

        let serialized = serde_json::to_value(&actual)
            .expect("diagnostic should serialize")
            .to_string();
        for injected_value in case.injected_values {
            assert!(
                !serialized.contains(injected_value),
                "serialized diagnostic must not retain the injected source value"
            );
        }
    }
}

fn compatibility_diagnostic_cases() -> Vec<CompatibilityDiagnosticCase> {
    compatibility_feed_cases()
        .into_iter()
        .chain(compatibility_manifest_cases())
        .chain(compatibility_evidence_cases())
        .chain(compatibility_state_cases())
        .collect()
}

fn compatibility_feed_cases() -> Vec<CompatibilityDiagnosticCase> {
    vec![
        CompatibilityDiagnosticCase {
            error: CompatibilityError::BuildClient(build_client_error()),
            expected: Diagnostic::general(DiagnosticReason::InternalInvariant),
            injected_values: &[],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::InvalidUrl {
                source: url::Url::parse("https://[invalid-compatibility-feed").unwrap_err(),
            },
            expected: Diagnostic::general(DiagnosticReason::InvalidManifest),
            injected_values: &[],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::InsecureUrl,
            expected: Diagnostic::general(DiagnosticReason::InvalidManifest),
            injected_values: &[],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::FetchManifest(fetch_manifest_error()),
            expected: Diagnostic::general(DiagnosticReason::NetworkRequestFailed),
            injected_values: &[FAKE_FETCH_URL],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::ManifestStatus(404),
            expected: Diagnostic::new(
                DiagnosticReason::HttpRequestRejected,
                DiagnosticDetails::Http {
                    operation: DiagnosticOperation::FetchUpdateManifest,
                    status: 404,
                },
            ),
            injected_values: &[],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::ManifestTooLarge,
            expected: Diagnostic::general(DiagnosticReason::InvalidManifest),
            injected_values: &[],
        },
    ]
}

fn compatibility_manifest_cases() -> Vec<CompatibilityDiagnosticCase> {
    vec![
        CompatibilityDiagnosticCase {
            error: CompatibilityError::ParseManifest(parse_json_error()),
            expected: Diagnostic::general(DiagnosticReason::InvalidManifest),
            injected_values: &[],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::UnsupportedManifestSchema(9),
            expected: Diagnostic::new(
                DiagnosticReason::UnsupportedVersion,
                DiagnosticDetails::Schema {
                    document: DocumentKind::CompatibilityManifest,
                    observed_version: Some(9),
                },
            ),
            injected_values: &[],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::EmptyReleases,
            expected: Diagnostic::general(DiagnosticReason::InvalidManifest),
            injected_values: &[],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::InvalidDesktopChecks("synthetic-private-detail"),
            expected: Diagnostic::general(DiagnosticReason::InvalidManifest),
            injected_values: &["synthetic-private-detail"],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::InvalidHostedChecks("synthetic-private-detail"),
            expected: Diagnostic::general(DiagnosticReason::InvalidManifest),
            injected_values: &["synthetic-private-detail"],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::DuplicateRelease(
                Version::parse("0.0.0").expect("synthetic release version should be valid"),
            ),
            expected: Diagnostic::general(DiagnosticReason::InvalidManifest),
            injected_values: &[],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::DuplicateHarness(HarnessKind::Codex),
            expected: Diagnostic::general(DiagnosticReason::InvalidManifest),
            injected_values: &[],
        },
    ]
}

fn compatibility_evidence_cases() -> Vec<CompatibilityDiagnosticCase> {
    vec![
        CompatibilityDiagnosticCase {
            error: CompatibilityError::IncompleteEvidencePair {
                id: FAKE_COMPATIBILITY_ID.to_owned(),
                track: "live",
            },
            expected: Diagnostic::general(DiagnosticReason::InvalidManifest),
            injected_values: &[FAKE_COMPATIBILITY_ID],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::MissingEvidence {
                id: FAKE_COMPATIBILITY_ID.to_owned(),
            },
            expected: Diagnostic::general(DiagnosticReason::InvalidManifest),
            injected_values: &[FAKE_COMPATIBILITY_ID],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::InvalidEvidenceTimestamp {
                id: FAKE_COMPATIBILITY_ID.to_owned(),
                track: "live",
                timestamp: FAKE_EVIDENCE_TIMESTAMP.to_owned(),
            },
            expected: Diagnostic::general(DiagnosticReason::InvalidManifest),
            injected_values: &[FAKE_COMPATIBILITY_ID, FAKE_EVIDENCE_TIMESTAMP],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::VersionBelowMinimum {
                harness: HarnessKind::Codex,
                version: Version::parse("0.1.0").expect("synthetic live version should be valid"),
                minimum: Version::parse("0.2.0")
                    .expect("synthetic minimum version should be valid"),
            },
            expected: Diagnostic::general(DiagnosticReason::InvalidManifest),
            injected_values: &[],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::LiveEvidenceAhead {
                harness: HarnessKind::Codex,
                live: Version::parse("0.3.0").expect("synthetic live version should be valid"),
                compatible: Version::parse("0.2.0")
                    .expect("synthetic compatible version should be valid"),
            },
            expected: Diagnostic::general(DiagnosticReason::InvalidManifest),
            injected_values: &[],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::LiveVersionBelowMinimum {
                harness: HarnessKind::Codex,
                version: Version::parse("0.1.0").expect("synthetic live version should be valid"),
                minimum: Version::parse("0.2.0")
                    .expect("synthetic minimum version should be valid"),
            },
            expected: Diagnostic::general(DiagnosticReason::InvalidManifest),
            injected_values: &[],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::InvalidEmbeddedManifest(FAKE_COMPATIBILITY_PATH.to_owned()),
            expected: Diagnostic::general(DiagnosticReason::InvalidManifest),
            injected_values: &[FAKE_COMPATIBILITY_PATH],
        },
    ]
}

fn compatibility_state_cases() -> Vec<CompatibilityDiagnosticCase> {
    vec![
        CompatibilityDiagnosticCase {
            error: CompatibilityError::MissingConfigDirectory,
            expected: Diagnostic::general(DiagnosticReason::MissingDirectory),
            injected_values: &[],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::ReadState(io_not_found()),
            expected: Diagnostic::new(
                DiagnosticReason::FilesystemOperationFailed,
                DiagnosticDetails::Io {
                    operation: DiagnosticOperation::ReadConfiguration,
                    error_kind: IoErrorKind::NotFound,
                },
            ),
            injected_values: &[],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::ParseState(parse_json_error()),
            expected: Diagnostic::general(DiagnosticReason::InvalidManifest),
            injected_values: &[],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::UnsupportedStateSchema(9),
            expected: Diagnostic::general(DiagnosticReason::InvalidManifest),
            injected_values: &[],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::CreateConfigDirectory(io_permission_denied()),
            expected: Diagnostic::new(
                DiagnosticReason::FilesystemOperationFailed,
                DiagnosticDetails::Io {
                    operation: DiagnosticOperation::ReadConfiguration,
                    error_kind: IoErrorKind::PermissionDenied,
                },
            ),
            injected_values: &[],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::SerializeState(parse_json_error()),
            expected: Diagnostic::general(DiagnosticReason::InvalidManifest),
            injected_values: &[],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::WriteState(io_timed_out()),
            expected: Diagnostic::new(
                DiagnosticReason::FilesystemOperationFailed,
                DiagnosticDetails::Io {
                    operation: DiagnosticOperation::ReadConfiguration,
                    error_kind: IoErrorKind::TimedOut,
                },
            ),
            injected_values: &[],
        },
        CompatibilityDiagnosticCase {
            error: CompatibilityError::SystemClock(system_time_error()),
            expected: Diagnostic::general(DiagnosticReason::InternalInvariant),
            injected_values: &[],
        },
    ]
}

fn build_client_error() -> reqwest::Error {
    reqwest::Proxy::all("http://[").expect_err("invalid proxy URL should be rejected in memory")
}

fn fetch_manifest_error() -> reqwest::Error {
    reqwest::Client::new()
        .get(FAKE_FETCH_URL)
        .build()
        .expect_err("unsupported URL scheme should be rejected without sending a request")
}

fn parse_json_error() -> serde_json::Error {
    serde_json::from_str::<serde_json::Value>("{\"compatibility\":").expect_err("malformed JSON")
}

fn io_not_found() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        "compatibility state is unavailable",
    )
}

fn io_permission_denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "compatibility directory is unavailable",
    )
}

fn io_timed_out() -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "compatibility write did not finish",
    )
}

fn system_time_error() -> SystemTimeError {
    SystemTime::UNIX_EPOCH
        .duration_since(SystemTime::UNIX_EPOCH + Duration::from_secs(1))
        .expect_err("one second after the Unix epoch is later than the epoch")
}
