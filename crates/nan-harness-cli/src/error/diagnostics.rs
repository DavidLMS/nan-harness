use super::CliError;
use crate::commands::install::InstallError;
use crate::commands::persistence::PersistenceError;
use crate::commands::uninstall::UninstallError;
use nan_harness_core::PlanError;
use nan_harness_runtime::{DiscoveryError, ProcessError, RuntimeError};
use nan_harness_telemetry::consent::SettingsError;
use nan_harness_telemetry::diagnostic::{
    Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason, DocumentKind,
    IoErrorKind, VersionComponent,
};

pub(super) fn typed_diagnostic(error: &CliError) -> Diagnostic {
    match error {
        CliError::Discovery(error) => discovery_typed_diagnostic(error),
        CliError::Install(error) => install_typed_diagnostic(error),
        CliError::Credential(_) | CliError::Configuration(_) => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
        CliError::CredentialInvariant => Diagnostic::general(DiagnosticReason::InternalInvariant),
        CliError::Runtime(error) => runtime_typed_diagnostic(error),
        CliError::CurrentDirectory(source) => {
            io_typed_diagnostic(DiagnosticOperation::ReadWorkingDirectory, source)
        }
        CliError::Random(_) => Diagnostic::general(DiagnosticReason::RandomGenerationFailed),
        CliError::InvalidPlan(error) => plan_typed_diagnostic(error),
        CliError::SerializePlan(_) => Diagnostic::general(DiagnosticReason::SerializationFailed),
        CliError::TelemetrySettings(error) => telemetry_settings_typed_diagnostic(error),
        CliError::Update(error) => update_typed_diagnostic(error),
        CliError::Persistence(error) => persistence_typed_diagnostic(error),
        CliError::Uninstall(error) => uninstall_typed_diagnostic(error),
    }
}

fn io_typed_diagnostic(operation: DiagnosticOperation, error: &std::io::Error) -> Diagnostic {
    Diagnostic::new(
        DiagnosticReason::FilesystemOperationFailed,
        DiagnosticDetails::Io {
            operation,
            error_kind: IoErrorKind::from_std(error.kind()),
        },
    )
}

fn process_typed_diagnostic(
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

fn version_typed_diagnostic(
    reason: DiagnosticReason,
    component: VersionComponent,
    detected: Option<String>,
    expected: Option<String>,
) -> Diagnostic {
    Diagnostic::new(
        reason,
        DiagnosticDetails::Version {
            component,
            detected,
            expected,
        },
    )
}

fn safe_version(value: &str) -> Option<String> {
    value.split_whitespace().find_map(|token| {
        let candidate = token
            .trim_matches(|character: char| !character.is_ascii_alphanumeric() && character != '.')
            .trim_start_matches('v');
        semver::Version::parse(candidate)
            .ok()
            .map(|version| version.to_string())
    })
}

fn discovery_typed_diagnostic(error: &DiscoveryError) -> Diagnostic {
    match error {
        DiscoveryError::InvalidManifest(_) | DiscoveryError::InvalidManifestContract(_) => {
            Diagnostic::new(
                DiagnosticReason::InvalidManifest,
                DiagnosticDetails::Schema {
                    document: DocumentKind::CompatibilityManifest,
                    observed_version: None,
                },
            )
        }
        DiscoveryError::MissingCompatibilityEntry(_) => {
            Diagnostic::general(DiagnosticReason::MissingManifestEntry)
        }
        DiscoveryError::InvalidVersionCommand { .. } => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
        DiscoveryError::ExecutableNotFound(_) => {
            Diagnostic::general(DiagnosticReason::MissingExecutable)
        }
        DiscoveryError::InvalidExecutable(_) => {
            Diagnostic::general(DiagnosticReason::InvalidExecutable)
        }
        DiscoveryError::VersionCommand { source, .. } => {
            io_typed_diagnostic(DiagnosticOperation::RunVersionCommand, source)
        }
        DiscoveryError::VersionCommandFailed { exit_code, .. } => process_typed_diagnostic(
            DiagnosticReason::ProcessExited,
            DiagnosticOperation::RunVersionCommand,
            *exit_code,
        ),
        DiscoveryError::UnsupportedVersion { detected, .. } => version_typed_diagnostic(
            DiagnosticReason::UnsupportedVersion,
            VersionComponent::Harness,
            safe_version(detected),
            None,
        ),
        DiscoveryError::UnparseableVersion { .. } => version_typed_diagnostic(
            DiagnosticReason::UnparseableVersion,
            VersionComponent::Harness,
            None,
            None,
        ),
    }
}

fn install_typed_diagnostic(error: &InstallError) -> Diagnostic {
    match error {
        InstallError::Prompt(source) => Diagnostic::new(
            DiagnosticReason::UserPromptFailed,
            DiagnosticDetails::Io {
                operation: DiagnosticOperation::RunInstaller,
                error_kind: IoErrorKind::from_std(source.kind()),
            },
        ),
        InstallError::UnsupportedPlatform(_)
        | InstallError::UnsupportedHarness(_)
        | InstallError::CompatibilityManifest(_)
        | InstallError::InvalidRuntimeCommand { .. } => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
        InstallError::RuntimeCommandStart { source, .. }
        | InstallError::PrepareInstaller { source, .. }
        | InstallError::InstallerStart { source, .. }
        | InstallError::CommandStart { source, .. } => {
            io_typed_diagnostic(DiagnosticOperation::RunInstaller, source)
        }
        InstallError::RuntimeCommandFailed { exit_code, .. }
        | InstallError::InstallerFailed { exit_code, .. }
        | InstallError::CommandFailed { exit_code, .. } => process_typed_diagnostic(
            DiagnosticReason::ProcessExited,
            DiagnosticOperation::RunInstaller,
            *exit_code,
        ),
        InstallError::RuntimeUnsupported {
            detected, minimum, ..
        } => version_typed_diagnostic(
            DiagnosticReason::UnsupportedVersion,
            VersionComponent::Runtime,
            safe_version(detected),
            Some(minimum.to_string()),
        ),
        InstallError::RuntimeUnparseable { minimum, .. } => version_typed_diagnostic(
            DiagnosticReason::UnparseableVersion,
            VersionComponent::Runtime,
            None,
            Some(minimum.to_string()),
        ),
        InstallError::DownloadStart { source, .. } => {
            io_typed_diagnostic(DiagnosticOperation::DownloadInstaller, source)
        }
        InstallError::DownloadFailed { exit_code, .. } => process_typed_diagnostic(
            DiagnosticReason::ProcessExited,
            DiagnosticOperation::DownloadInstaller,
            *exit_code,
        ),
        InstallError::PostInstallCheckStart { source, .. }
        | InstallError::PostInstallCheckPrepare { source, .. } => {
            io_typed_diagnostic(DiagnosticOperation::RunPostInstallCheck, source)
        }
        InstallError::PostInstallCheckFailed { exit_code, .. } => process_typed_diagnostic(
            DiagnosticReason::ProcessExited,
            DiagnosticOperation::RunPostInstallCheck,
            *exit_code,
        ),
    }
}

fn runtime_typed_diagnostic(error: &RuntimeError) -> Diagnostic {
    match error {
        RuntimeError::InvalidPlan(error) => plan_typed_diagnostic(error),
        RuntimeError::BindBridge(source) => {
            io_typed_diagnostic(DiagnosticOperation::BindBridge, source)
        }
        RuntimeError::Bridge(error) => bridge_startup_typed_diagnostic(error),
        RuntimeError::BridgeExited => Diagnostic::general(DiagnosticReason::BridgeExited),
        RuntimeError::Prepared(_) => Diagnostic::general(DiagnosticReason::LaunchPreparationFailed),
        RuntimeError::Process(ProcessError::Secret(_)) | RuntimeError::Secret(_) => {
            Diagnostic::general(DiagnosticReason::SecretResolutionFailed)
        }
        RuntimeError::Process(ProcessError::Spawn(source)) => {
            let reason = if source.kind() == std::io::ErrorKind::NotFound {
                DiagnosticReason::MissingExecutable
            } else {
                DiagnosticReason::ProcessStartFailed
            };
            Diagnostic::new(
                reason,
                DiagnosticDetails::Io {
                    operation: DiagnosticOperation::StartHarness,
                    error_kind: IoErrorKind::from_std(source.kind()),
                },
            )
        }
        RuntimeError::Random(_) => Diagnostic::general(DiagnosticReason::RandomGenerationFailed),
        RuntimeError::WaitForProcess(source) => Diagnostic::new(
            DiagnosticReason::ProcessWaitFailed,
            DiagnosticDetails::Io {
                operation: DiagnosticOperation::WaitForHarness,
                error_kind: IoErrorKind::from_std(source.kind()),
            },
        ),
        RuntimeError::TerminateProcess(source) => Diagnostic::new(
            DiagnosticReason::ProcessTerminationFailed,
            DiagnosticDetails::Io {
                operation: DiagnosticOperation::StopHarness,
                error_kind: IoErrorKind::from_std(source.kind()),
            },
        ),
        RuntimeError::MissingProcessId => {
            Diagnostic::general(DiagnosticReason::ProcessTerminationFailed)
        }
    }
}

fn bridge_startup_typed_diagnostic(error: &nan_harness_runtime::BridgeError) -> Diagnostic {
    use nan_harness_runtime::BridgeError;

    match error {
        BridgeError::ListenerAddress(source) | BridgeError::Serve(source) => {
            io_typed_diagnostic(DiagnosticOperation::RunBridge, source)
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
        BridgeError::InvalidModelDiscoveryResponse(_) => {
            Diagnostic::general(DiagnosticReason::InvalidResponse)
        }
        BridgeError::NoCompatibleModels => Diagnostic::general(DiagnosticReason::ModelCatalogEmpty),
        BridgeError::SelectedModelUnavailable { .. } => {
            Diagnostic::general(DiagnosticReason::ModelUnavailable)
        }
        BridgeError::TaskJoin(_) => Diagnostic::general(DiagnosticReason::BridgeExited),
    }
}

fn plan_typed_diagnostic(error: &PlanError) -> Diagnostic {
    match error {
        PlanError::InvalidField { .. }
        | PlanError::AdapterMismatch { .. }
        | PlanError::TransportMismatch { .. } => {
            Diagnostic::general(DiagnosticReason::InvalidLaunchPlan)
        }
        PlanError::MissingSecretReference { .. } => {
            Diagnostic::general(DiagnosticReason::SecretResolutionFailed)
        }
        PlanError::ConflictingEnvironment { .. } | PlanError::UnsafeTemporaryArtifact { .. } => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
    }
}

fn telemetry_settings_typed_diagnostic(error: &SettingsError) -> Diagnostic {
    match error {
        SettingsError::MissingConfigDirectory => {
            Diagnostic::general(DiagnosticReason::MissingDirectory)
        }
        SettingsError::CreateDirectory(source) | SettingsError::Write(source) => {
            io_typed_diagnostic(DiagnosticOperation::ConfigureTelemetry, source)
        }
        SettingsError::Read(source) => {
            io_typed_diagnostic(DiagnosticOperation::ReadConfiguration, source)
        }
        SettingsError::Parse(_) => Diagnostic::new(
            DiagnosticReason::InvalidConfiguration,
            DiagnosticDetails::Schema {
                document: DocumentKind::TelemetrySettings,
                observed_version: None,
            },
        ),
        SettingsError::Serialize(_) => Diagnostic::general(DiagnosticReason::SerializationFailed),
        SettingsError::Random(_) => Diagnostic::general(DiagnosticReason::RandomGenerationFailed),
    }
}

fn update_typed_diagnostic(error: &nan_harness_runtime::update::UpdateError) -> Diagnostic {
    use nan_harness_runtime::update::UpdateError;

    match error {
        UpdateError::UpdateChannelUnavailable
        | UpdateError::Version(_)
        | UpdateError::InvalidUrl { .. }
        | UpdateError::InsecureUrl(_) => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
        UpdateError::MissingConfigDirectory => {
            Diagnostic::general(DiagnosticReason::MissingDirectory)
        }
        UpdateError::BuildClient(_)
        | UpdateError::FetchManifest(_)
        | UpdateError::DownloadArtifact(_) => {
            Diagnostic::general(DiagnosticReason::NetworkRequestFailed)
        }
        UpdateError::ManifestStatus(status) => Diagnostic::new(
            DiagnosticReason::HttpRequestRejected,
            DiagnosticDetails::Http {
                operation: DiagnosticOperation::FetchUpdateManifest,
                status: *status,
            },
        ),
        UpdateError::ManifestTooLarge
        | UpdateError::ParseManifest(_)
        | UpdateError::UnsupportedManifestSchema(_)
        | UpdateError::EmptyArtifactCatalog
        | UpdateError::InvalidChecksum
        | UpdateError::MissingArtifact(_) => Diagnostic::new(
            DiagnosticReason::InvalidManifest,
            DiagnosticDetails::Schema {
                document: DocumentKind::UpdateManifest,
                observed_version: match error {
                    UpdateError::UnsupportedManifestSchema(version) => Some(u16::from(*version)),
                    _ => None,
                },
            },
        ),
        UpdateError::ArtifactStatus(status) => Diagnostic::new(
            DiagnosticReason::HttpRequestRejected,
            DiagnosticDetails::Http {
                operation: DiagnosticOperation::DownloadUpdate,
                status: *status,
            },
        ),
        UpdateError::ArtifactTooLarge
        | UpdateError::ChecksumMismatch
        | UpdateError::CandidateRejected
        | UpdateError::CandidateVersionMismatch { .. } => {
            Diagnostic::general(DiagnosticReason::UpdateVerificationFailed)
        }
        UpdateError::CreateCandidate(source)
        | UpdateError::WriteCandidate(source)
        | UpdateError::SetCandidatePermissions(source)
        | UpdateError::ExecuteCandidate(source) => {
            io_typed_diagnostic(DiagnosticOperation::VerifyUpdate, source)
        }
        UpdateError::ReplaceExecutable(source)
        | UpdateError::RemoveCandidate(source)
        | UpdateError::Restart(source) => Diagnostic::new(
            DiagnosticReason::UpdateReplacementFailed,
            DiagnosticDetails::Io {
                operation: DiagnosticOperation::ReplaceExecutable,
                error_kind: IoErrorKind::from_std(source.kind()),
            },
        ),
        UpdateError::CreateConfigDirectory(source) | UpdateError::WriteState(source) => {
            io_typed_diagnostic(DiagnosticOperation::WriteConfiguration, source)
        }
        UpdateError::ReadState(source) => {
            io_typed_diagnostic(DiagnosticOperation::ReadConfiguration, source)
        }
        UpdateError::ParseState(_) | UpdateError::UnsupportedStateSchema(_) => Diagnostic::new(
            DiagnosticReason::InvalidConfiguration,
            DiagnosticDetails::Schema {
                document: DocumentKind::UpdateState,
                observed_version: match error {
                    UpdateError::UnsupportedStateSchema(version) => Some(u16::from(*version)),
                    _ => None,
                },
            },
        ),
        UpdateError::SerializeState(_) => {
            Diagnostic::general(DiagnosticReason::SerializationFailed)
        }
        UpdateError::SystemClock(_) => Diagnostic::general(DiagnosticReason::InternalInvariant),
        UpdateError::Prompt(source) => Diagnostic::new(
            DiagnosticReason::UserPromptFailed,
            DiagnosticDetails::Io {
                operation: DiagnosticOperation::ReplaceExecutable,
                error_kind: IoErrorKind::from_std(source.kind()),
            },
        ),
    }
}

fn persistence_typed_diagnostic(error: &PersistenceError) -> Diagnostic {
    match error {
        PersistenceError::MissingConfigDirectory | PersistenceError::MissingHomeDirectory => {
            Diagnostic::general(DiagnosticReason::MissingDirectory)
        }
        PersistenceError::CreateDirectory { source, .. }
        | PersistenceError::WriteFile { source, .. }
        | PersistenceError::CreateStateDirectory(source) => {
            io_typed_diagnostic(DiagnosticOperation::WriteConfiguration, source)
        }
        PersistenceError::ReadFile { source, .. }
        | PersistenceError::ReadState(source)
        | PersistenceError::ReadPreferences(source) => {
            io_typed_diagnostic(DiagnosticOperation::ReadConfiguration, source)
        }
        PersistenceError::RemoveFile { source, .. } => {
            io_typed_diagnostic(DiagnosticOperation::RemoveConfiguration, source)
        }
        PersistenceError::ManagedFileChanged(_)
        | PersistenceError::AmbiguousOpenCodeConfig(_)
        | PersistenceError::UnmanagedProviderConflict(_)
        | PersistenceError::ManagedProviderChanged(_)
        | PersistenceError::UnmanagedSectionConflict(_)
        | PersistenceError::ManagedSectionChanged(_) => {
            Diagnostic::general(DiagnosticReason::ConfigurationConflict)
        }
        PersistenceError::BuildClient(_) | PersistenceError::DiscoverModels(_) => {
            Diagnostic::general(DiagnosticReason::NetworkRequestFailed)
        }
        PersistenceError::ModelDiscoveryStatus(status) => Diagnostic::new(
            DiagnosticReason::HttpRequestRejected,
            DiagnosticDetails::Http {
                operation: DiagnosticOperation::DiscoverModels,
                status: *status,
            },
        ),
        PersistenceError::ParseModels(_) => Diagnostic::general(DiagnosticReason::InvalidResponse),
        PersistenceError::NoModels => Diagnostic::general(DiagnosticReason::ModelCatalogEmpty),
        PersistenceError::Secret(_) => {
            Diagnostic::general(DiagnosticReason::SecretResolutionFailed)
        }
        PersistenceError::SerializeProvider(_)
        | PersistenceError::SerializeState(_)
        | PersistenceError::SerializePreferences(_) => {
            Diagnostic::general(DiagnosticReason::SerializationFailed)
        }
        PersistenceError::UnsupportedStateSchema(version)
        | PersistenceError::UnsupportedPreferencesSchema(version) => Diagnostic::new(
            DiagnosticReason::UnsupportedVersion,
            DiagnosticDetails::Schema {
                document: DocumentKind::IntegrationState,
                observed_version: Some(u16::from(*version)),
            },
        ),
        PersistenceError::RenderConfiguration(_)
        | PersistenceError::InvalidPath(_)
        | PersistenceError::InvalidUtf8 { .. }
        | PersistenceError::InvalidReceiptPath(_)
        | PersistenceError::RootIsNotObject(_)
        | PersistenceError::ProviderIsNotObject(_)
        | PersistenceError::InvalidManagedProvider(_)
        | PersistenceError::InvalidManagedSection(_)
        | PersistenceError::InvalidManagedBlock
        | PersistenceError::ConfigRootIsNotObject { .. }
        | PersistenceError::ConfigFieldIsNotObject { .. }
        | PersistenceError::ParseHarnessConfig { .. }
        | PersistenceError::ParseOpenCodeConfig { .. }
        | PersistenceError::GenerateOpenCodeProvider(_)
        | PersistenceError::ParseState(_)
        | PersistenceError::ParsePreferences(_) => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
    }
}

fn uninstall_typed_diagnostic(error: &UninstallError) -> Diagnostic {
    match error {
        UninstallError::Configuration(_) | UninstallError::Credential(_) => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
        UninstallError::Persistence(error) => persistence_typed_diagnostic(error),
        UninstallError::ConfirmationRequired => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
        UninstallError::InstallationNotManaged
        | UninstallError::ExecutableMismatch { .. }
        | UninstallError::UnsafeInstallationPath(_)
        | UninstallError::UnsafeAliasPath(_)
        | UninstallError::UnsafeDataDirectory(_) => {
            Diagnostic::general(DiagnosticReason::ConfigurationConflict)
        }
        UninstallError::CurrentExecutable(source)
        | UninstallError::CanonicalizeExecutable { source, .. }
        | UninstallError::InspectDataDirectory { source, .. }
        | UninstallError::InspectAlias { source, .. }
        | UninstallError::ReadReceipt { source, .. }
        | UninstallError::CreateDataDirectory { source, .. }
        | UninstallError::WriteReceipt { source, .. }
        | UninstallError::Prompt(source) => {
            io_typed_diagnostic(DiagnosticOperation::RemoveInstallation, source)
        }
        UninstallError::ParseReceipt(_) => Diagnostic::new(
            DiagnosticReason::InvalidConfiguration,
            DiagnosticDetails::Schema {
                document: DocumentKind::InstallationReceipt,
                observed_version: None,
            },
        ),
        UninstallError::UnsupportedReceiptSchema(version) => Diagnostic::new(
            DiagnosticReason::UnsupportedVersion,
            DiagnosticDetails::Schema {
                document: DocumentKind::InstallationReceipt,
                observed_version: Some(u16::from(*version)),
            },
        ),
        UninstallError::SerializeReceipt(_) => {
            Diagnostic::general(DiagnosticReason::SerializationFailed)
        }
        #[cfg(not(windows))]
        UninstallError::RemoveFile { source, .. }
        | UninstallError::RemoveDataDirectory { source, .. } => {
            io_typed_diagnostic(DiagnosticOperation::RemoveInstallation, source)
        }
        #[cfg(windows)]
        UninstallError::CreateHelper(source) | UninstallError::StartHelper(source) => {
            io_typed_diagnostic(DiagnosticOperation::RemoveInstallation, source)
        }
    }
}
