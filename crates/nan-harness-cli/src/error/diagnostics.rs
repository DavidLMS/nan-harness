use super::CliError;
use crate::commands::chatgpt_desktop::ChatGptDesktopError;
use crate::commands::claude_desktop::ClaudeDesktopError;
use crate::commands::install::InstallError;
use crate::commands::persistence::PersistenceError;
use crate::commands::uninstall::UninstallError;
use nan_harness_core::PlanError;
use nan_harness_runtime::{DiscoveryError, ProcessError, RuntimeError, SearchPolicyError};
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
        CliError::ChatGptDesktop(error) => chatgpt_desktop_typed_diagnostic(error),
        CliError::ClaudeDesktop(error) => claude_desktop_typed_diagnostic(error),
        CliError::HermesDesktop(error) => error.diagnostic(),
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
        CliError::UsageEvidence(_) => {
            Diagnostic::general(DiagnosticReason::FilesystemOperationFailed)
        }
    }
}

fn chatgpt_desktop_typed_diagnostic(error: &ChatGptDesktopError) -> Diagnostic {
    match error {
        ChatGptDesktopError::UnsupportedPlatform
        | ChatGptDesktopError::OlderUnsupported { .. }
        | ChatGptDesktopError::NewerUntested { .. }
        | ChatGptDesktopError::Compatibility(_) => {
            Diagnostic::general(DiagnosticReason::UnsupportedVersion)
        }
        ChatGptDesktopError::AppNotFound => {
            Diagnostic::general(DiagnosticReason::MissingExecutable)
        }
        ChatGptDesktopError::InvalidInstallation => {
            Diagnostic::general(DiagnosticReason::InvalidExecutable)
        }
        ChatGptDesktopError::VersionCommand(source) => {
            io_typed_diagnostic(DiagnosticOperation::RunVersionCommand, source)
        }
        ChatGptDesktopError::VersionCommandFailed => process_typed_diagnostic(
            DiagnosticReason::ProcessExited,
            DiagnosticOperation::RunVersionCommand,
            None,
        ),
        ChatGptDesktopError::UnparseableVersion => {
            Diagnostic::general(DiagnosticReason::UnparseableVersion)
        }
        ChatGptDesktopError::AppAlreadyRunning
        | ChatGptDesktopError::SingletonRace
        | ChatGptDesktopError::UnmanagedProfile
        | ChatGptDesktopError::InvalidMarker
        | ChatGptDesktopError::InvalidReceipt
        | ChatGptDesktopError::OrphanedSessionFiles => {
            Diagnostic::general(DiagnosticReason::ConfigurationConflict)
        }
        ChatGptDesktopError::AppDidNotTerminate | ChatGptDesktopError::StopApp(_) => {
            Diagnostic::general(DiagnosticReason::ProcessTerminationFailed)
        }
        ChatGptDesktopError::AppExitedDuringStartup => {
            Diagnostic::general(DiagnosticReason::ProcessExited)
        }
        ChatGptDesktopError::InspectProcess(source) => {
            io_typed_diagnostic(DiagnosticOperation::WaitForHarness, source)
        }
        ChatGptDesktopError::ProcessInspectionFailed | ChatGptDesktopError::WaitForApp(_) => {
            Diagnostic::general(DiagnosticReason::ProcessWaitFailed)
        }
        ChatGptDesktopError::State(_) | ChatGptDesktopError::Persistence(_) => {
            Diagnostic::general(DiagnosticReason::FilesystemOperationFailed)
        }
        ChatGptDesktopError::InspectProfile(source) | ChatGptDesktopError::ReadState(source) => {
            io_typed_diagnostic(DiagnosticOperation::ReadConfiguration, source)
        }
        ChatGptDesktopError::WriteState(source) => {
            io_typed_diagnostic(DiagnosticOperation::WriteConfiguration, source)
        }
        ChatGptDesktopError::ParseMarker(_) | ChatGptDesktopError::ParseReceipt(_) => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
        ChatGptDesktopError::SerializeState(_) => {
            Diagnostic::general(DiagnosticReason::SerializationFailed)
        }
        ChatGptDesktopError::Bridge(_) | ChatGptDesktopError::BridgeExited => {
            Diagnostic::general(DiagnosticReason::BridgeExited)
        }
        ChatGptDesktopError::BridgeHandshakeTimeout => {
            Diagnostic::general(DiagnosticReason::AuthenticationRejected)
        }
        ChatGptDesktopError::StartApp(source) => {
            io_typed_diagnostic(DiagnosticOperation::StartHarness, source)
        }
    }
}

fn claude_desktop_typed_diagnostic(error: &ClaudeDesktopError) -> Diagnostic {
    match error {
        ClaudeDesktopError::UnsupportedPlatform | ClaudeDesktopError::Compatibility(_) => {
            Diagnostic::general(DiagnosticReason::UnsupportedVersion)
        }
        ClaudeDesktopError::AppNotFound { .. } => {
            Diagnostic::general(DiagnosticReason::MissingExecutable)
        }
        ClaudeDesktopError::AlreadyRunning
        | ClaudeDesktopError::ConcurrentSession
        | ClaudeDesktopError::OrphanReceipt
        | ClaudeDesktopError::NoReceipt
        | ClaudeDesktopError::UnsafeSymlink
        | ClaudeDesktopError::OrphanBackup
        | ClaudeDesktopError::BackupHashMismatch
        | ClaudeDesktopError::UnsupportedReceipt => {
            Diagnostic::general(DiagnosticReason::ConfigurationConflict)
        }
        ClaudeDesktopError::DidNotStart => {
            Diagnostic::general(DiagnosticReason::ProcessStartFailed)
        }
        ClaudeDesktopError::DidNotTerminate => {
            Diagnostic::general(DiagnosticReason::ProcessTerminationFailed)
        }
        ClaudeDesktopError::Bridge(_) => Diagnostic::general(DiagnosticReason::BridgeExited),
        ClaudeDesktopError::MissingHome
        | ClaudeDesktopError::MissingPlatformDirectory(_)
        | ClaudeDesktopError::InvalidStatePath => {
            Diagnostic::general(DiagnosticReason::MissingDirectory)
        }
        ClaudeDesktopError::CreateDirectory(source)
        | ClaudeDesktopError::Permissions(source)
        | ClaudeDesktopError::CreateBackupDirectory(source)
        | ClaudeDesktopError::WriteBackup(source)
        | ClaudeDesktopError::Write(source) => {
            io_typed_diagnostic(DiagnosticOperation::WriteConfiguration, source)
        }
        ClaudeDesktopError::Lock(source)
        | ClaudeDesktopError::ReadConfig(source)
        | ClaudeDesktopError::ReadBackup(source)
        | ClaudeDesktopError::ReadReceipt(source) => {
            io_typed_diagnostic(DiagnosticOperation::ReadConfiguration, source)
        }
        ClaudeDesktopError::ProcessCheck(source) => {
            io_typed_diagnostic(DiagnosticOperation::WaitForHarness, source)
        }
        ClaudeDesktopError::ProcessCheckFailed(exit_code) => process_typed_diagnostic(
            DiagnosticReason::ProcessWaitFailed,
            DiagnosticOperation::WaitForHarness,
            *exit_code,
        ),
        ClaudeDesktopError::Launch(source) => {
            io_typed_diagnostic(DiagnosticOperation::StartHarness, source)
        }
        ClaudeDesktopError::LaunchFailed(exit_code) => process_typed_diagnostic(
            DiagnosticReason::ProcessStartFailed,
            DiagnosticOperation::StartHarness,
            *exit_code,
        ),
        ClaudeDesktopError::Terminate(source) => {
            io_typed_diagnostic(DiagnosticOperation::StopHarness, source)
        }
        ClaudeDesktopError::TerminateFailed(exit_code) => process_typed_diagnostic(
            DiagnosticReason::ProcessTerminationFailed,
            DiagnosticOperation::StopHarness,
            *exit_code,
        ),
        ClaudeDesktopError::ParseConfig(_)
        | ClaudeDesktopError::ConfigRoot
        | ClaudeDesktopError::ParseReceipt(_) => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
        ClaudeDesktopError::SerializeConfig(_) | ClaudeDesktopError::SerializeReceipt(_) => {
            Diagnostic::general(DiagnosticReason::SerializationFailed)
        }
        ClaudeDesktopError::Restore(source)
        | ClaudeDesktopError::RemoveBackup(source)
        | ClaudeDesktopError::RemoveReceipt(source) => {
            io_typed_diagnostic(DiagnosticOperation::RemoveConfiguration, source)
        }
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
        RuntimeError::SearchPolicy(error) => search_policy_typed_diagnostic(error),
    }
}

fn search_policy_typed_diagnostic(error: &SearchPolicyError) -> Diagnostic {
    match error {
        SearchPolicyError::ReadConfiguration { source, .. } => {
            io_typed_diagnostic(DiagnosticOperation::ReadConfiguration, source)
        }
        SearchPolicyError::MissingHomeDirectory
        | SearchPolicyError::UnsupportedHarness(_)
        | SearchPolicyError::RequiresDirectGateway
        | SearchPolicyError::McpNameCollision(_)
        | SearchPolicyError::ConfigurationTooLarge(_)
        | SearchPolicyError::ParseJson { .. }
        | SearchPolicyError::ParseToml { .. }
        | SearchPolicyError::ConvertToml { .. } => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
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
        BridgeError::ModelDiscoveryTooLarge | BridgeError::InvalidModelDiscoveryResponse(_) => {
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
        PersistenceError::ModelDiscoveryTooLarge | PersistenceError::ParseModels(_) => {
            Diagnostic::general(DiagnosticReason::InvalidResponse)
        }
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
        UninstallError::HermesDesktop(error) => error.diagnostic(),
        UninstallError::Persistence(error) => persistence_typed_diagnostic(error),
        UninstallError::ConfirmationRequired | UninstallError::DesktopRecoveryRequired(_) => {
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

#[cfg(test)]
mod tests {
    use super::typed_diagnostic;
    use crate::commands::persistence::PersistenceError;
    use crate::error::CliError;
    use nan_harness_core::{HarnessKind, PlanError};
    use nan_harness_runtime::{BridgeError, DiscoveryError, RuntimeError, SearchPolicyError};
    use nan_harness_telemetry::consent::SettingsError;
    use nan_harness_telemetry::diagnostic::{
        Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason, IoErrorKind,
        VersionComponent,
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

    #[test]
    fn representative_cli_errors_have_structured_sanitized_diagnostics() {
        let sensitive_path = PathBuf::from("/private/local/path/provider response secret");
        let cases = runtime_cases(&sensitive_path)
            .into_iter()
            .chain(model_catalog_cases())
            .chain(persistence_cases(sensitive_path))
            .chain(remaining_cases())
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
