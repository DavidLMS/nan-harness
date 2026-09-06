use super::typed;
use nan_harness_core::{HarnessKind, PlanError, SecretError};
use nan_harness_runtime::{
    BridgeError, PreparedError, ProcessError, RuntimeError, SearchPolicyError,
    inspect_search_configuration,
};
use nan_harness_telemetry::diagnostic::{
    Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason, IoErrorKind,
};
use std::io;
use std::path::PathBuf;

const FAKE_PATH: &str = "/private/diagnostic-fixture/fake-secret.toml";
const FAKE_MESSAGE: &str = "fixture-only sensitive message";
const FAKE_TOKEN: &str = "fixture-token-must-not-appear";

fn io_diagnostic(operation: DiagnosticOperation, kind: IoErrorKind) -> Diagnostic {
    Diagnostic::new(
        DiagnosticReason::FilesystemOperationFailed,
        DiagnosticDetails::Io {
            operation,
            error_kind: kind,
        },
    )
}

fn process_diagnostic(
    reason: DiagnosticReason,
    operation: DiagnosticOperation,
    kind: IoErrorKind,
) -> Diagnostic {
    Diagnostic::new(
        reason,
        DiagnosticDetails::Io {
            operation,
            error_kind: kind,
        },
    )
}

fn assert_safe(error: &RuntimeError, expected: &Diagnostic) {
    let diagnostic = typed(error);
    assert_eq!(&diagnostic, expected);

    let serialized = serde_json::to_string(&diagnostic).expect("diagnostic should serialize");
    for sensitive in [FAKE_PATH, FAKE_MESSAGE, FAKE_TOKEN] {
        assert!(
            !serialized.contains(sensitive),
            "diagnostic leaked fixture value {sensitive:?}: {serialized}"
        );
    }
}

#[test]
fn runtime_errors_map_to_closed_typed_diagnostics() {
    let cases = [
        (
            RuntimeError::InvalidPlan(PlanError::MissingSecretReference {
                reference: FAKE_TOKEN.to_owned(),
            }),
            Diagnostic::general(DiagnosticReason::SecretResolutionFailed),
        ),
        (
            RuntimeError::BindBridge(io::Error::new(
                io::ErrorKind::PermissionDenied,
                FAKE_MESSAGE,
            )),
            io_diagnostic(
                DiagnosticOperation::BindBridge,
                IoErrorKind::PermissionDenied,
            ),
        ),
        (
            RuntimeError::Bridge(BridgeError::ListenerAddress(io::Error::new(
                io::ErrorKind::AddrInUse,
                FAKE_MESSAGE,
            ))),
            io_diagnostic(DiagnosticOperation::RunBridge, IoErrorKind::AddressInUse),
        ),
        (
            RuntimeError::BridgeExited,
            Diagnostic::general(DiagnosticReason::BridgeExited),
        ),
        (
            RuntimeError::Prepared(PreparedError::UnresolvedPlaceholder(FAKE_TOKEN.to_owned())),
            Diagnostic::general(DiagnosticReason::LaunchPreparationFailed),
        ),
        (
            RuntimeError::Process(ProcessError::Secret(SecretError::MissingReference(
                FAKE_TOKEN.to_owned(),
            ))),
            Diagnostic::general(DiagnosticReason::SecretResolutionFailed),
        ),
        (
            RuntimeError::Secret(SecretError::InvalidReference(FAKE_TOKEN.to_owned())),
            Diagnostic::general(DiagnosticReason::SecretResolutionFailed),
        ),
        (
            RuntimeError::Process(ProcessError::Spawn(io::Error::new(
                io::ErrorKind::NotFound,
                FAKE_MESSAGE,
            ))),
            Diagnostic::new(
                DiagnosticReason::MissingExecutable,
                DiagnosticDetails::Io {
                    operation: DiagnosticOperation::StartHarness,
                    error_kind: IoErrorKind::NotFound,
                },
            ),
        ),
        (
            RuntimeError::Process(ProcessError::Spawn(io::Error::other(FAKE_MESSAGE))),
            Diagnostic::new(
                DiagnosticReason::ProcessStartFailed,
                DiagnosticDetails::Io {
                    operation: DiagnosticOperation::StartHarness,
                    error_kind: IoErrorKind::Other,
                },
            ),
        ),
        (
            RuntimeError::Random(getrandom::Error::UNEXPECTED),
            Diagnostic::general(DiagnosticReason::RandomGenerationFailed),
        ),
        (
            RuntimeError::WaitForProcess(io::Error::new(io::ErrorKind::TimedOut, FAKE_MESSAGE)),
            process_diagnostic(
                DiagnosticReason::ProcessWaitFailed,
                DiagnosticOperation::WaitForHarness,
                IoErrorKind::TimedOut,
            ),
        ),
        (
            RuntimeError::TerminateProcess(io::Error::new(io::ErrorKind::BrokenPipe, FAKE_MESSAGE)),
            process_diagnostic(
                DiagnosticReason::ProcessTerminationFailed,
                DiagnosticOperation::StopHarness,
                IoErrorKind::BrokenPipe,
            ),
        ),
        (
            RuntimeError::MissingProcessId,
            Diagnostic::general(DiagnosticReason::ProcessTerminationFailed),
        ),
    ];

    for (error, expected) in cases {
        assert_safe(&error, &expected);
    }
}

#[test]
fn every_constructible_search_policy_error_has_a_safe_configuration_diagnostic() {
    let json_source = jsonc_parser::parse_to_serde_value::<serde_json::Value>(
        "{",
        &jsonc_parser::ParseOptions::default(),
    )
    .expect_err("fixture must be invalid JSON");
    let toml_fixture = tempfile::tempdir().expect("temporary fixture directory");
    std::fs::create_dir_all(toml_fixture.path().join(".kimi-code"))
        .expect("temporary config directory");
    std::fs::write(
        toml_fixture.path().join(".kimi-code/config.toml"),
        format!("{FAKE_TOKEN} = ["),
    )
    .expect("invalid TOML fixture");
    let toml_error = inspect_search_configuration(
        HarnessKind::KimiCode,
        toml_fixture.path(),
        toml_fixture.path(),
    )
    .expect_err("fixture must produce a TOML parse error");
    let SearchPolicyError::ParseToml {
        path: _,
        source: toml_source,
    } = toml_error
    else {
        panic!("invalid TOML fixture returned an unexpected error");
    };
    let cases = [
        SearchPolicyError::MissingHomeDirectory,
        SearchPolicyError::UnsupportedHarness(HarnessKind::Aider),
        SearchPolicyError::RequiresDirectGateway,
        SearchPolicyError::McpNameCollision(PathBuf::from(FAKE_PATH)),
        SearchPolicyError::ConfigurationTooLarge(PathBuf::from(FAKE_PATH)),
        SearchPolicyError::ReadConfiguration {
            path: PathBuf::from(FAKE_PATH),
            source: io::Error::new(io::ErrorKind::PermissionDenied, FAKE_MESSAGE),
        },
        SearchPolicyError::ParseJson {
            path: PathBuf::from(FAKE_PATH),
            source: json_source,
        },
        SearchPolicyError::ParseToml {
            path: PathBuf::from(FAKE_PATH),
            source: toml_source,
        },
    ];

    for error in cases {
        let expected = if matches!(error, SearchPolicyError::ReadConfiguration { .. }) {
            io_diagnostic(
                DiagnosticOperation::ReadConfiguration,
                IoErrorKind::PermissionDenied,
            )
        } else {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        };
        let runtime_error = RuntimeError::SearchPolicy(error);
        assert_safe(&runtime_error, &expected);
    }
}
