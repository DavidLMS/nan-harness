use crate::app::Cli;
use crate::commands::credentials::CredentialError;
use crate::commands::install::InstallError;
use crate::commands::persistence::PersistenceError;
use crate::commands::uninstall::UninstallError;
use crate::observability::enrich_telemetry_context;
use nan_harness_core::PlanError;
use nan_harness_runtime::{DiscoveryError, ProcessError, RuntimeError};
use nan_harness_telemetry::consent::SettingsError;
use nan_harness_telemetry::event::{
    ErrorReportContext, Failure, FailureCategory, FailureCause, FailureStage,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum CliError {
    #[error(transparent)]
    Discovery(#[from] DiscoveryError),
    #[error(transparent)]
    Install(#[from] InstallError),
    #[error(transparent)]
    Credential(#[from] CredentialError),
    #[error("internal credential preflight was not completed")]
    CredentialInvariant,
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error("could not read the current working directory: {0}")]
    CurrentDirectory(std::io::Error),
    #[error("could not generate a launch ID: {0}")]
    Random(getrandom::Error),
    #[error("launch plan is invalid: {0}")]
    InvalidPlan(PlanError),
    #[error("could not serialize the validated launch plan: {0}")]
    SerializePlan(serde_json::Error),
    #[error(transparent)]
    TelemetrySettings(#[from] SettingsError),
    #[error(transparent)]
    Update(#[from] nan_harness_runtime::update::UpdateError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    Uninstall(#[from] UninstallError),
}

impl CliError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Discovery(error) => error.code(),
            Self::Install(_) => InstallError::code(),
            Self::Credential(error) => error.code(),
            Self::Runtime(error) => error.code(),
            Self::SerializePlan(_) => "NH-CLI-003",
            Self::CurrentDirectory(_) | Self::Random(_) | Self::CredentialInvariant => "NH-CLI-005",
            Self::InvalidPlan(error) => error.code(),
            Self::TelemetrySettings(_) => "NH-TELEMETRY-001",
            Self::Update(error) => error.code(),
            Self::Persistence(error) => error.code(),
            Self::Uninstall(error) => error.code(),
        }
    }

    pub(crate) fn telemetry_context(&self, cli: &Cli, interactive: bool) -> ErrorReportContext {
        let (category, stage, retryable) = self.telemetry_failure();
        let (cause, http_status) = self.telemetry_diagnostics();
        let mut failure = Failure::new(self.code(), category, stage, retryable).with_cause(cause);
        if let Some(status) = http_status {
            failure = failure.with_http_status(status);
        }
        enrich_telemetry_context(ErrorReportContext::new(failure, interactive), cli, true)
    }

    const fn telemetry_failure(&self) -> (FailureCategory, FailureStage, bool) {
        match self {
            Self::Discovery(_) => (
                FailureCategory::Discovery,
                FailureStage::HarnessDetection,
                false,
            ),
            Self::Install(_) => (
                FailureCategory::Discovery,
                FailureStage::HarnessDetection,
                true,
            ),
            Self::Credential(_) => (
                FailureCategory::Configuration,
                FailureStage::CredentialResolution,
                false,
            ),
            Self::Runtime(error) => runtime_failure(error),
            Self::InvalidPlan(_) => (
                FailureCategory::Planning,
                FailureStage::LaunchValidation,
                false,
            ),
            Self::SerializePlan(_) => (
                FailureCategory::Internal,
                FailureStage::LaunchValidation,
                false,
            ),
            Self::CurrentDirectory(_) | Self::Random(_) | Self::CredentialInvariant => {
                (FailureCategory::Internal, FailureStage::Startup, false)
            }
            Self::TelemetrySettings(_) => {
                (FailureCategory::Configuration, FailureStage::Startup, false)
            }
            Self::Update(_) => (FailureCategory::Internal, FailureStage::Startup, true),
            Self::Persistence(_) => (FailureCategory::Configuration, FailureStage::Startup, false),
            Self::Uninstall(_) => (
                FailureCategory::Configuration,
                FailureStage::Shutdown,
                false,
            ),
        }
    }

    fn telemetry_diagnostics(&self) -> (FailureCause, Option<u16>) {
        match self {
            Self::Discovery(error) => discovery_diagnostics(error),
            Self::Install(error) => install_diagnostics(error),
            Self::Credential(error) => credential_diagnostics(error),
            Self::CredentialInvariant | Self::InvalidPlan(_) => {
                (FailureCause::InvalidConfiguration, None)
            }
            Self::Runtime(error) => runtime_diagnostics(error),
            Self::CurrentDirectory(source) => (io_diagnostics(source), None),
            Self::SerializePlan(_) => (FailureCause::Serialization, None),
            Self::Random(_) => (FailureCause::Internal, None),
            Self::TelemetrySettings(_) | Self::Uninstall(_) => (FailureCause::Filesystem, None),
            Self::Update(error) => update_diagnostics(error),
            Self::Persistence(error) => persistence_diagnostics(error),
        }
    }
}

const fn runtime_failure(error: &RuntimeError) -> (FailureCategory, FailureStage, bool) {
    match error {
        RuntimeError::InvalidPlan(_) => (
            FailureCategory::Planning,
            FailureStage::LaunchValidation,
            false,
        ),
        RuntimeError::BindBridge(_) => {
            (FailureCategory::Bridge, FailureStage::BridgeStartup, false)
        }
        RuntimeError::Bridge(_) | RuntimeError::BridgeExited => {
            (FailureCategory::Bridge, FailureStage::BridgeStartup, true)
        }
        RuntimeError::Prepared(_) | RuntimeError::Process(_) => (
            FailureCategory::Process,
            FailureStage::HarnessExecution,
            false,
        ),
        RuntimeError::Secret(_) | RuntimeError::Random(_) => {
            (FailureCategory::Internal, FailureStage::Startup, false)
        }
        RuntimeError::WaitForProcess(_)
        | RuntimeError::TerminateProcess(_)
        | RuntimeError::MissingProcessId => {
            (FailureCategory::Process, FailureStage::Shutdown, true)
        }
    }
}

fn discovery_diagnostics(error: &DiscoveryError) -> (FailureCause, Option<u16>) {
    match error {
        DiscoveryError::ExecutableNotFound(_) => (FailureCause::MissingExecutable, None),
        DiscoveryError::InvalidExecutable(_) => (FailureCause::PermissionDenied, None),
        DiscoveryError::VersionCommand { source, .. } => (io_diagnostics(source), None),
        DiscoveryError::VersionCommandFailed { .. } => (FailureCause::ProcessExit, None),
        DiscoveryError::UnsupportedVersion { .. } | DiscoveryError::UnparseableVersion { .. } => {
            (FailureCause::UnsupportedVersion, None)
        }
        DiscoveryError::InvalidManifest(_)
        | DiscoveryError::MissingCompatibilityEntry(_)
        | DiscoveryError::InvalidVersionCommand { .. } => (FailureCause::InvalidData, None),
    }
}

fn install_diagnostics(error: &InstallError) -> (FailureCause, Option<u16>) {
    match error {
        InstallError::Prompt(source)
        | InstallError::DownloadStart { source, .. }
        | InstallError::PrepareInstaller { source, .. }
        | InstallError::InstallerStart { source, .. }
        | InstallError::CommandStart { source, .. } => (io_diagnostics(source), None),
        InstallError::DownloadFailed { .. }
        | InstallError::InstallerFailed { .. }
        | InstallError::CommandFailed { .. } => (FailureCause::ProcessExit, None),
        InstallError::UnsupportedPlatform(_) | InstallError::UnsupportedHarness(_) => {
            (FailureCause::InvalidConfiguration, None)
        }
    }
}

fn runtime_diagnostics(error: &RuntimeError) -> (FailureCause, Option<u16>) {
    match error {
        RuntimeError::InvalidPlan(_) | RuntimeError::Prepared(_) => {
            (FailureCause::InvalidData, None)
        }
        RuntimeError::BindBridge(source)
        | RuntimeError::WaitForProcess(source)
        | RuntimeError::TerminateProcess(source) => (io_diagnostics(source), None),
        RuntimeError::Bridge(error) => {
            if let Some(status) = error.http_status() {
                (FailureCause::HttpStatus, Some(status))
            } else if error.is_timeout() {
                (FailureCause::Timeout, None)
            } else if error.is_invalid_response() {
                (FailureCause::InvalidResponse, None)
            } else if error.code() == "NH-BRIDGE-004" {
                (FailureCause::Network, None)
            } else if error.code() == "NH-BRIDGE-005" {
                (FailureCause::InvalidConfiguration, None)
            } else {
                (FailureCause::Internal, None)
            }
        }
        RuntimeError::BridgeExited | RuntimeError::MissingProcessId => {
            (FailureCause::ProcessExit, None)
        }
        RuntimeError::Process(ProcessError::Secret(_)) | RuntimeError::Secret(_) => {
            (FailureCause::MissingCredential, None)
        }
        RuntimeError::Process(ProcessError::Spawn(source)) => match io_diagnostics(source) {
            FailureCause::NotFound => (FailureCause::MissingExecutable, None),
            FailureCause::PermissionDenied => (FailureCause::PermissionDenied, None),
            _ => (FailureCause::ProcessStart, None),
        },
        RuntimeError::Random(_) => (FailureCause::Internal, None),
    }
}

fn persistence_diagnostics(error: &PersistenceError) -> (FailureCause, Option<u16>) {
    match error {
        PersistenceError::DiscoverModels(source) if source.is_timeout() => {
            (FailureCause::Timeout, None)
        }
        PersistenceError::BuildClient(_) | PersistenceError::DiscoverModels(_) => {
            (FailureCause::Network, None)
        }
        PersistenceError::ModelDiscoveryStatus(status) => (FailureCause::HttpStatus, Some(*status)),
        PersistenceError::ParseModels(_) | PersistenceError::NoModels => {
            (FailureCause::InvalidResponse, None)
        }
        PersistenceError::Secret(_) => (FailureCause::MissingCredential, None),
        PersistenceError::CreateDirectory { source, .. }
        | PersistenceError::ReadFile { source, .. }
        | PersistenceError::WriteFile { source, .. }
        | PersistenceError::RemoveFile { source, .. }
        | PersistenceError::BackupFile { source, .. } => (io_diagnostics(source), None),
        _ if error.code() == "NH-INTEGRATION-001" => (FailureCause::Filesystem, None),
        _ => (FailureCause::InvalidConfiguration, None),
    }
}

fn credential_diagnostics(error: &CredentialError) -> (FailureCause, Option<u16>) {
    match error {
        CredentialError::MissingCredential => (FailureCause::MissingCredential, None),
        CredentialError::InteractiveLoginRequired
        | CredentialError::InvalidConfigDirectory(_)
        | CredentialError::InvalidBackend(_)
        | CredentialError::NonUnicodeBackend
        | CredentialError::ParseReceipt(_)
        | CredentialError::UnsupportedReceiptSchema(_)
        | CredentialError::SerializeReceipt(_)
        | CredentialError::Secret(_)
        | CredentialError::Config(_) => (FailureCause::InvalidConfiguration, None),
        CredentialError::Prompt(error) => (io_diagnostics(error), None),
        CredentialError::Verification(error) | CredentialError::State(error) => {
            persistence_diagnostics(error)
        }
        CredentialError::VerificationTimeout => (FailureCause::Timeout, None),
        CredentialError::Keyring(_) => (FailureCause::PermissionDenied, None),
        CredentialError::ReadFile { source, .. } | CredentialError::RemoveFile { source, .. } => {
            (io_diagnostics(source), None)
        }
        CredentialError::MissingConfigDirectory => (FailureCause::Filesystem, None),
    }
}

fn update_diagnostics(
    error: &nan_harness_runtime::update::UpdateError,
) -> (FailureCause, Option<u16>) {
    use nan_harness_runtime::update::UpdateError;

    match error {
        UpdateError::FetchManifest(source) | UpdateError::DownloadArtifact(source)
            if source.is_timeout() =>
        {
            (FailureCause::Timeout, None)
        }
        UpdateError::BuildClient(_)
        | UpdateError::FetchManifest(_)
        | UpdateError::DownloadArtifact(_) => (FailureCause::Network, None),
        UpdateError::ManifestStatus(status) | UpdateError::ArtifactStatus(status) => {
            (FailureCause::HttpStatus, Some(*status))
        }
        UpdateError::ParseManifest(_)
        | UpdateError::UnsupportedManifestSchema(_)
        | UpdateError::EmptyArtifactCatalog
        | UpdateError::InvalidChecksum
        | UpdateError::ChecksumMismatch
        | UpdateError::CandidateRejected
        | UpdateError::CandidateVersionMismatch { .. } => (FailureCause::InvalidData, None),
        UpdateError::ExecuteCandidate(_) | UpdateError::Restart(_) => {
            (FailureCause::ProcessStart, None)
        }
        _ if error.code() == "NH-UPDATE-001" => (FailureCause::InvalidConfiguration, None),
        _ => (FailureCause::Filesystem, None),
    }
}

fn io_diagnostics(error: &std::io::Error) -> FailureCause {
    match error.kind() {
        std::io::ErrorKind::NotFound => FailureCause::NotFound,
        std::io::ErrorKind::PermissionDenied => FailureCause::PermissionDenied,
        std::io::ErrorKind::TimedOut => FailureCause::Timeout,
        std::io::ErrorKind::ConnectionRefused
        | std::io::ErrorKind::ConnectionReset
        | std::io::ErrorKind::ConnectionAborted
        | std::io::ErrorKind::NotConnected
        | std::io::ErrorKind::AddrInUse
        | std::io::ErrorKind::AddrNotAvailable
        | std::io::ErrorKind::BrokenPipe => FailureCause::Network,
        _ => FailureCause::Filesystem,
    }
}
