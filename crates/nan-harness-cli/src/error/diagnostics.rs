mod bridge;
mod desktop;
mod details;
mod discovery;
mod install;
mod persistence;
mod plan;
mod runtime;
mod settings;
mod uninstall;
mod update;

use super::CliError;
use nan_harness_telemetry::diagnostic::{Diagnostic, DiagnosticOperation, DiagnosticReason};

pub(super) fn typed_diagnostic(error: &CliError) -> Diagnostic {
    match error {
        CliError::Discovery(error) => discovery::typed(error),
        CliError::Install(error) => install::typed(error),
        CliError::Credential(_) | CliError::Configuration(_) => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
        CliError::ChatGptDesktop(error) => desktop::chatgpt(error),
        CliError::ClaudeDesktop(error) => desktop::claude(error),
        CliError::HermesDesktop(error) => error.diagnostic(),
        CliError::PenDesktop(error) => error.diagnostic(),
        CliError::CredentialInvariant => Diagnostic::general(DiagnosticReason::InternalInvariant),
        CliError::Runtime(error) => runtime::typed(error),
        CliError::CurrentDirectory(source) => {
            details::io(DiagnosticOperation::ReadWorkingDirectory, source)
        }
        CliError::Random(_) => Diagnostic::general(DiagnosticReason::RandomGenerationFailed),
        CliError::InvalidPlan(error) => plan::typed(error),
        CliError::SerializePlan(_) => Diagnostic::general(DiagnosticReason::SerializationFailed),
        CliError::TelemetrySettings(error) => settings::typed(error),
        CliError::Update(error) => update::typed(error),
        CliError::Persistence(error) => persistence::typed(error),
        CliError::Uninstall(error) => uninstall::typed(error),
        CliError::UsageEvidence(_) => {
            Diagnostic::general(DiagnosticReason::FilesystemOperationFailed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::typed_diagnostic;
    use crate::commands::persistence::PersistenceError;
    use crate::error::CliError;
    use nan_harness_core::{HarnessKind, PlanError};
    use nan_harness_runtime::update::UpdateError;
    use nan_harness_runtime::{BridgeError, DiscoveryError, RuntimeError, SearchPolicyError};
    use nan_harness_telemetry::consent::SettingsError;
    use nan_harness_telemetry::diagnostic::{
        Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason, DocumentKind,
        IoErrorKind, VersionComponent,
    };
    use std::io;
    use std::path::{Path, PathBuf};

    struct Case {
        name: &'static str,
        error: CliError,
        expected: Diagnostic,
    }

    fn runtime_cases(sensitive_path: &Path) -> Vec<Case> {
        vec![
            Case {
                name: "current directory",
                error: CliError::CurrentDirectory(io::Error::from(io::ErrorKind::PermissionDenied)),
                expected: Diagnostic::new(
                    DiagnosticReason::FilesystemOperationFailed,
                    DiagnosticDetails::Io {
                        operation: DiagnosticOperation::ReadWorkingDirectory,
                        error_kind: IoErrorKind::PermissionDenied,
                    },
                ),
            },
            Case {
                name: "search policy configuration",
                error: CliError::Runtime(RuntimeError::SearchPolicy(
                    SearchPolicyError::RequiresDirectGateway,
                )),
                expected: Diagnostic::general(DiagnosticReason::InvalidConfiguration),
            },
            Case {
                name: "search policy filesystem",
                error: CliError::Runtime(RuntimeError::SearchPolicy(
                    SearchPolicyError::ReadConfiguration {
                        path: sensitive_path.to_path_buf(),
                        source: io::Error::from(io::ErrorKind::PermissionDenied),
                    },
                )),
                expected: Diagnostic::new(
                    DiagnosticReason::FilesystemOperationFailed,
                    DiagnosticDetails::Io {
                        operation: DiagnosticOperation::ReadConfiguration,
                        error_kind: IoErrorKind::PermissionDenied,
                    },
                ),
            },
        ]
    }

    fn model_catalog_cases() -> Vec<Case> {
        vec![
            Case {
                name: "selected model unavailable",
                error: CliError::Runtime(RuntimeError::Bridge(
                    BridgeError::SelectedModelUnavailable {
                        model: "requested-model-secret".to_owned(),
                        available: vec!["catalog-model-secret".to_owned()],
                    },
                )),
                expected: Diagnostic::general(DiagnosticReason::ModelUnavailable),
            },
            Case {
                name: "empty model catalog",
                error: CliError::Runtime(RuntimeError::Bridge(BridgeError::NoCompatibleModels)),
                expected: Diagnostic::general(DiagnosticReason::ModelCatalogEmpty),
            },
            Case {
                name: "invalid bridge catalog",
                error: CliError::Runtime(RuntimeError::Bridge(BridgeError::ModelDiscoveryTooLarge)),
                expected: Diagnostic::general(DiagnosticReason::InvalidResponse),
            },
            Case {
                name: "bridge catalog HTTP status",
                error: CliError::Runtime(RuntimeError::Bridge(BridgeError::ModelDiscoveryStatus {
                    status: reqwest::StatusCode::SERVICE_UNAVAILABLE,
                    message: "provider response secret".to_owned(),
                })),
                expected: Diagnostic::new(
                    DiagnosticReason::HttpRequestRejected,
                    DiagnosticDetails::Http {
                        operation: DiagnosticOperation::DiscoverModels,
                        status: 503,
                    },
                ),
            },
        ]
    }

    fn persistence_cases(sensitive_path: PathBuf) -> Vec<Case> {
        vec![
            Case {
                name: "persistence empty model catalog",
                error: CliError::Persistence(PersistenceError::NoModels),
                expected: Diagnostic::general(DiagnosticReason::ModelCatalogEmpty),
            },
            Case {
                name: "persistence invalid catalog",
                error: CliError::Persistence(PersistenceError::ModelDiscoveryTooLarge),
                expected: Diagnostic::general(DiagnosticReason::InvalidResponse),
            },
            Case {
                name: "persistence catalog HTTP status",
                error: CliError::Persistence(PersistenceError::ModelDiscoveryStatus(429)),
                expected: Diagnostic::new(
                    DiagnosticReason::HttpRequestRejected,
                    DiagnosticDetails::Http {
                        operation: DiagnosticOperation::DiscoverModels,
                        status: 429,
                    },
                ),
            },
            Case {
                name: "persistence filesystem",
                error: CliError::Persistence(PersistenceError::ReadFile {
                    path: sensitive_path,
                    source: io::Error::from(io::ErrorKind::NotFound),
                }),
                expected: Diagnostic::new(
                    DiagnosticReason::FilesystemOperationFailed,
                    DiagnosticDetails::Io {
                        operation: DiagnosticOperation::ReadConfiguration,
                        error_kind: IoErrorKind::NotFound,
                    },
                ),
            },
        ]
    }

    fn remaining_cases() -> Vec<Case> {
        vec![
            Case {
                name: "missing harness executable",
                error: CliError::Discovery(DiscoveryError::ExecutableNotFound(
                    "provider response secret".to_owned(),
                )),
                expected: Diagnostic::general(DiagnosticReason::MissingExecutable),
            },
            Case {
                name: "version command failed",
                error: CliError::Discovery(DiscoveryError::VersionCommandFailed {
                    command: "provider response secret".to_owned(),
                    exit_code: Some(17),
                }),
                expected: Diagnostic::new(
                    DiagnosticReason::ProcessExited,
                    DiagnosticDetails::Process {
                        operation: DiagnosticOperation::RunVersionCommand,
                        exit_code: Some(17),
                    },
                ),
            },
            Case {
                name: "unsupported harness version",
                error: CliError::Discovery(DiscoveryError::UnsupportedVersion {
                    harness: HarnessKind::Codex,
                    detected: "codex 1.2.3 provider response secret".to_owned(),
                }),
                expected: Diagnostic::new(
                    DiagnosticReason::UnsupportedVersion,
                    DiagnosticDetails::Version {
                        component: VersionComponent::Harness,
                        detected: Some("1.2.3".to_owned()),
                        expected: None,
                    },
                ),
            },
            Case {
                name: "invalid launch plan",
                error: CliError::InvalidPlan(PlanError::InvalidField {
                    field: "model",
                    message: "requested-model-secret".to_owned(),
                }),
                expected: Diagnostic::general(DiagnosticReason::InvalidLaunchPlan),
            },
            Case {
                name: "missing telemetry directory",
                error: CliError::TelemetrySettings(SettingsError::MissingConfigDirectory),
                expected: Diagnostic::general(DiagnosticReason::MissingDirectory),
            },
        ]
    }

    fn update_cases() -> Vec<Case> {
        update_configuration_cases()
            .into_iter()
            .chain(update_network_cases())
            .chain(update_manifest_cases())
            .chain(update_artifact_cases())
            .chain(update_state_cases())
            .chain(update_prompt_cases())
            .collect()
    }

    fn update_configuration_cases() -> Vec<Case> {
        vec![
            Case {
                name: "update channel unavailable",
                error: CliError::Update(UpdateError::UpdateChannelUnavailable),
                expected: Diagnostic::general(DiagnosticReason::InvalidConfiguration),
            },
            Case {
                name: "invalid update version",
                error: CliError::Update(UpdateError::Version(
                    semver::Version::parse("not-a-version").unwrap_err(),
                )),
                expected: Diagnostic::general(DiagnosticReason::InvalidConfiguration),
            },
            Case {
                name: "invalid update URL",
                error: CliError::Update(UpdateError::InvalidUrl {
                    purpose: "update manifest",
                    source: url::Url::parse("http://[").unwrap_err(),
                }),
                expected: Diagnostic::general(DiagnosticReason::InvalidConfiguration),
            },
            Case {
                name: "insecure update URL",
                error: CliError::Update(UpdateError::InsecureUrl("update manifest")),
                expected: Diagnostic::general(DiagnosticReason::InvalidConfiguration),
            },
            Case {
                name: "missing update directory",
                error: CliError::Update(UpdateError::MissingConfigDirectory),
                expected: Diagnostic::general(DiagnosticReason::MissingDirectory),
            },
        ]
    }

    fn update_network_cases() -> Vec<Case> {
        let invalid_request = reqwest::Proxy::all("http://[").unwrap_err();
        vec![
            Case {
                name: "update client network failure",
                error: CliError::Update(UpdateError::BuildClient(invalid_request)),
                expected: Diagnostic::general(DiagnosticReason::NetworkRequestFailed),
            },
            Case {
                name: "update manifest HTTP status",
                error: CliError::Update(UpdateError::ManifestStatus(503)),
                expected: Diagnostic::new(
                    DiagnosticReason::HttpRequestRejected,
                    DiagnosticDetails::Http {
                        operation: DiagnosticOperation::FetchUpdateManifest,
                        status: 503,
                    },
                ),
            },
            Case {
                name: "update artifact HTTP status",
                error: CliError::Update(UpdateError::ArtifactStatus(404)),
                expected: Diagnostic::new(
                    DiagnosticReason::HttpRequestRejected,
                    DiagnosticDetails::Http {
                        operation: DiagnosticOperation::DownloadUpdate,
                        status: 404,
                    },
                ),
            },
        ]
    }

    fn update_manifest_cases() -> Vec<Case> {
        let invalid_json = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        vec![
            Case {
                name: "invalid update manifest",
                error: CliError::Update(UpdateError::ParseManifest(invalid_json)),
                expected: Diagnostic::new(
                    DiagnosticReason::InvalidManifest,
                    DiagnosticDetails::Schema {
                        document: DocumentKind::UpdateManifest,
                        observed_version: None,
                    },
                ),
            },
            Case {
                name: "unsupported update manifest schema",
                error: CliError::Update(UpdateError::UnsupportedManifestSchema(7)),
                expected: Diagnostic::new(
                    DiagnosticReason::InvalidManifest,
                    DiagnosticDetails::Schema {
                        document: DocumentKind::UpdateManifest,
                        observed_version: Some(7),
                    },
                ),
            },
        ]
    }

    fn update_artifact_cases() -> Vec<Case> {
        vec![
            Case {
                name: "update verification failure",
                error: CliError::Update(UpdateError::ChecksumMismatch),
                expected: Diagnostic::general(DiagnosticReason::UpdateVerificationFailed),
            },
            Case {
                name: "update candidate verification I/O",
                error: CliError::Update(UpdateError::WriteCandidate(io::Error::from(
                    io::ErrorKind::PermissionDenied,
                ))),
                expected: Diagnostic::new(
                    DiagnosticReason::FilesystemOperationFailed,
                    DiagnosticDetails::Io {
                        operation: DiagnosticOperation::VerifyUpdate,
                        error_kind: IoErrorKind::PermissionDenied,
                    },
                ),
            },
            Case {
                name: "update replacement failure",
                error: CliError::Update(UpdateError::ReplaceExecutable(io::Error::from(
                    io::ErrorKind::NotFound,
                ))),
                expected: Diagnostic::new(
                    DiagnosticReason::UpdateReplacementFailed,
                    DiagnosticDetails::Io {
                        operation: DiagnosticOperation::ReplaceExecutable,
                        error_kind: IoErrorKind::NotFound,
                    },
                ),
            },
        ]
    }

    fn update_state_cases() -> Vec<Case> {
        vec![
            Case {
                name: "update state write failure",
                error: CliError::Update(UpdateError::WriteState(io::Error::from(
                    io::ErrorKind::ReadOnlyFilesystem,
                ))),
                expected: Diagnostic::new(
                    DiagnosticReason::FilesystemOperationFailed,
                    DiagnosticDetails::Io {
                        operation: DiagnosticOperation::WriteConfiguration,
                        error_kind: IoErrorKind::Other,
                    },
                ),
            },
            Case {
                name: "unsupported update state schema",
                error: CliError::Update(UpdateError::UnsupportedStateSchema(9)),
                expected: Diagnostic::new(
                    DiagnosticReason::InvalidConfiguration,
                    DiagnosticDetails::Schema {
                        document: DocumentKind::UpdateState,
                        observed_version: Some(9),
                    },
                ),
            },
        ]
    }

    fn update_prompt_cases() -> Vec<Case> {
        let system_clock = std::time::UNIX_EPOCH
            .duration_since(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1))
            .unwrap_err();
        vec![
            Case {
                name: "update prompt failure",
                error: CliError::Update(UpdateError::Prompt(io::Error::from(
                    io::ErrorKind::BrokenPipe,
                ))),
                expected: Diagnostic::new(
                    DiagnosticReason::UserPromptFailed,
                    DiagnosticDetails::Io {
                        operation: DiagnosticOperation::ReplaceExecutable,
                        error_kind: IoErrorKind::BrokenPipe,
                    },
                ),
            },
            Case {
                name: "update system clock failure",
                error: CliError::Update(UpdateError::SystemClock(system_clock)),
                expected: Diagnostic::general(DiagnosticReason::InternalInvariant),
            },
        ]
    }

    #[test]
    fn representative_cli_errors_have_structured_sanitized_diagnostics() {
        let sensitive_path = PathBuf::from("/private/local/path/provider response secret");
        let cases = runtime_cases(&sensitive_path)
            .into_iter()
            .chain(model_catalog_cases())
            .chain(persistence_cases(sensitive_path))
            .chain(remaining_cases())
            .chain(update_cases())
            .collect::<Vec<_>>();

        assert!(cases.len() >= 12);
        for case in cases {
            let diagnostic = typed_diagnostic(&case.error);
            assert_eq!(diagnostic, case.expected, "{}", case.name);

            let serialized = serde_json::to_string(&diagnostic)
                .expect("typed diagnostic should serialize without failure");
            for sensitive in [
                "/private/local/path",
                "requested-model-secret",
                "catalog-model-secret",
                "provider response secret",
                "nan codex --model",
            ] {
                assert!(
                    !serialized.contains(sensitive),
                    "{} leaked sensitive value {sensitive:?}: {serialized}",
                    case.name
                );
            }
        }
    }
}
