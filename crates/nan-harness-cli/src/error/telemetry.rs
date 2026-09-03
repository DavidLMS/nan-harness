use super::CliError;
use crate::app::Cli;
use crate::commands::configuration::ConfigurationError;
use crate::commands::credentials::CredentialError;
use crate::commands::install::InstallError;
use crate::commands::persistence::PersistenceError;
use crate::observability::{HarnessIdentitySource, enrich_telemetry_context, is_harness_dry_run};
use nan_harness_core::{DetectedHarness, PlanError};
use nan_harness_runtime::{DiscoveryError, ProcessError, RuntimeError, SearchPolicyError};
use nan_harness_telemetry::diagnostic::Diagnostic;
use nan_harness_telemetry::event::{
    ErrorReportContext, Failure, FailureCategory, FailureCause, FailureStage, UserGuidance,
};

impl CliError {
    pub(crate) fn telemetry_context(
        &self,
        cli: &Cli,
        interactive: bool,
        harness: Option<&DetectedHarness>,
    ) -> ErrorReportContext {
        let (category, stage, retryable) = self.telemetry_failure();
        let (cause, http_status) = self.telemetry_diagnostics();
        let mut failure = Failure::new(self.code(), category, stage, retryable).with_cause(cause);
        if let Some(status) = http_status {
            failure = failure.with_http_status(status);
        }
        let harness_source = harness.map_or_else(
            || {
                if matches!(self, Self::CurrentDirectory(_)) {
                    HarnessIdentitySource::KindOnly
                } else {
                    HarnessIdentitySource::Detect
                }
            },
            HarnessIdentitySource::Known,
        );
        let mut context = enrich_telemetry_context(
            ErrorReportContext::new(failure, interactive).with_diagnostic(self.typed_diagnostic()),
            cli,
            harness_source,
        );
        if matches!(self, Self::CurrentDirectory(_)) {
            context = context.with_user_guidance(UserGuidance::reopen_terminal(true));
        }
        context
    }

    pub(crate) fn should_report_telemetry(&self, cli: &Cli) -> bool {
        if matches!(
            self,
            Self::Update(nan_harness_runtime::update::UpdateError::UpdateChannelUnavailable)
                | Self::UsageEvidence(_)
        ) {
            return false;
        }

        if is_harness_dry_run(cli)
            && matches!(
                self,
                Self::Discovery(DiscoveryError::InvalidExecutable(_))
                    | Self::InvalidPlan(PlanError::InvalidField { .. })
            )
        {
            return false;
        }

        true
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
            Self::Configuration(_) | Self::TelemetrySettings(_) => {
                (FailureCategory::Configuration, FailureStage::Startup, false)
            }
            Self::ChatGptDesktop(_)
            | Self::ClaudeDesktop(_)
            | Self::HermesDesktop(_)
            | Self::PenDesktop(_) => (
                FailureCategory::Configuration,
                FailureStage::HarnessExecution,
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
            Self::CurrentDirectory(_)
            | Self::Random(_)
            | Self::CredentialInvariant
            | Self::PreflightTaskFailed(_) => {
                (FailureCategory::Internal, FailureStage::Startup, false)
            }
            Self::Update(_) => (FailureCategory::Internal, FailureStage::Startup, true),
            Self::Persistence(_) => (FailureCategory::Configuration, FailureStage::Startup, false),
            Self::Uninstall(_) => (
                FailureCategory::Configuration,
                FailureStage::Shutdown,
                false,
            ),
            Self::UsageEvidence(_) => (FailureCategory::Internal, FailureStage::Shutdown, false),
        }
    }

    fn telemetry_diagnostics(&self) -> (FailureCause, Option<u16>) {
        match self {
            Self::Discovery(error) => discovery_diagnostics(error),
            Self::Install(error) => install_diagnostics(error),
            Self::Credential(error) => credential_diagnostics(error),
            Self::Configuration(error) => configuration_diagnostics(error),
            Self::ChatGptDesktop(_)
            | Self::ClaudeDesktop(_)
            | Self::HermesDesktop(_)
            | Self::PenDesktop(_)
            | Self::CredentialInvariant
            | Self::InvalidPlan(_) => (FailureCause::InvalidConfiguration, None),
            Self::Runtime(error) => runtime_diagnostics(error),
            Self::CurrentDirectory(source) => (io_diagnostics(source), None),
            Self::SerializePlan(_) => (FailureCause::Serialization, None),
            Self::Random(_) | Self::PreflightTaskFailed(_) => (FailureCause::Internal, None),
            Self::TelemetrySettings(_) | Self::Uninstall(_) | Self::UsageEvidence(_) => {
                (FailureCause::Filesystem, None)
            }
            Self::Update(error) => update_diagnostics(error),
            Self::Persistence(error) => persistence_diagnostics(error),
        }
    }

    fn typed_diagnostic(&self) -> Diagnostic {
        super::diagnostics::typed_diagnostic(self)
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
        RuntimeError::SearchPolicy(_) => (
            FailureCategory::Configuration,
            FailureStage::LaunchValidation,
            false,
        ),
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
        | DiscoveryError::InvalidManifestContract(_)
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
        | InstallError::CommandStart { source, .. }
        | InstallError::RuntimeCommandStart { source, .. }
        | InstallError::PostInstallCheckStart { source, .. }
        | InstallError::PostInstallCheckPrepare { source, .. } => (io_diagnostics(source), None),
        InstallError::DownloadFailed { .. }
        | InstallError::InstallerFailed { .. }
        | InstallError::CommandFailed { .. }
        | InstallError::RuntimeCommandFailed { .. }
        | InstallError::PostInstallCheckFailed { .. } => (FailureCause::ProcessExit, None),
        InstallError::RuntimeUnsupported { .. } | InstallError::RuntimeUnparseable { .. } => {
            (FailureCause::UnsupportedVersion, None)
        }
        InstallError::CompatibilityManifest(_)
        | InstallError::InvalidRuntimeCommand { .. }
        | InstallError::UnsupportedPlatform(_)
        | InstallError::UnsupportedHarness(_) => (FailureCause::InvalidConfiguration, None),
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
        RuntimeError::Bridge(error) => runtime_bridge_diagnostics(error),
        RuntimeError::BridgeExited | RuntimeError::MissingProcessId => {
            (FailureCause::ProcessExit, None)
        }
        RuntimeError::Process(error) => runtime_process_diagnostics(error),
        RuntimeError::Secret(_) => (FailureCause::MissingCredential, None),
        RuntimeError::SearchPolicy(error) => runtime_search_policy_diagnostics(error),
        RuntimeError::Random(_) => (FailureCause::Internal, None),
    }
}

fn runtime_bridge_diagnostics(
    error: &nan_harness_runtime::BridgeError,
) -> (FailureCause, Option<u16>) {
    if let Some(diagnostics) = runtime_bridge_http_diagnostics(error) {
        return diagnostics;
    }
    runtime_bridge_code_diagnostics(error).unwrap_or((FailureCause::Internal, None))
}

fn runtime_bridge_http_diagnostics(
    error: &nan_harness_runtime::BridgeError,
) -> Option<(FailureCause, Option<u16>)> {
    if let Some(status) = error.http_status() {
        return Some((FailureCause::HttpStatus, Some(status)));
    }
    if error.is_timeout() {
        return Some((FailureCause::Timeout, None));
    }
    if error.is_invalid_response() {
        return Some((FailureCause::InvalidResponse, None));
    }
    None
}

fn runtime_bridge_code_diagnostics(
    error: &nan_harness_runtime::BridgeError,
) -> Option<(FailureCause, Option<u16>)> {
    match error.code() {
        "NH-BRIDGE-004" => Some((FailureCause::Network, None)),
        "NH-BRIDGE-005" => Some((FailureCause::InvalidConfiguration, None)),
        _ => None,
    }
}

fn runtime_process_diagnostics(error: &ProcessError) -> (FailureCause, Option<u16>) {
    match error {
        ProcessError::Secret(_) => (FailureCause::MissingCredential, None),
        ProcessError::Spawn(source) => match io_diagnostics(source) {
            FailureCause::NotFound => (FailureCause::MissingExecutable, None),
            FailureCause::PermissionDenied => (FailureCause::PermissionDenied, None),
            _ => (FailureCause::ProcessStart, None),
        },
    }
}

fn runtime_search_policy_diagnostics(error: &SearchPolicyError) -> (FailureCause, Option<u16>) {
    match error {
        SearchPolicyError::ReadConfiguration { source, .. } => (io_diagnostics(source), None),
        SearchPolicyError::MissingHomeDirectory
        | SearchPolicyError::UnsupportedHarness(_)
        | SearchPolicyError::RequiresDirectGateway
        | SearchPolicyError::McpNameCollision(_)
        | SearchPolicyError::ConfigurationTooLarge(_)
        | SearchPolicyError::ParseJson { .. }
        | SearchPolicyError::ParseToml { .. }
        | SearchPolicyError::ConvertToml { .. } => (FailureCause::InvalidConfiguration, None),
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
        PersistenceError::ModelDiscoveryTooLarge
        | PersistenceError::ParseModels(_)
        | PersistenceError::NoModels => (FailureCause::InvalidResponse, None),
        PersistenceError::Secret(_) => (FailureCause::MissingCredential, None),
        PersistenceError::CreateDirectory { source, .. }
        | PersistenceError::ReadFile { source, .. }
        | PersistenceError::WriteFile { source, .. }
        | PersistenceError::RemoveFile { source, .. } => (io_diagnostics(source), None),
        _ if error.code() == "NH-INTEGRATION-001" => (FailureCause::Filesystem, None),
        _ => (FailureCause::InvalidConfiguration, None),
    }
}

fn credential_diagnostics(error: &CredentialError) -> (FailureCause, Option<u16>) {
    match error {
        CredentialError::MissingCredential | CredentialError::MissingSavedCredential => {
            (FailureCause::MissingCredential, None)
        }
        CredentialError::InteractiveLoginRequired
        | CredentialError::InvalidConfigDirectory(_)
        | CredentialError::InvalidBackend(_)
        | CredentialError::NonUnicodeBackend
        | CredentialError::ParseReceipt(_)
        | CredentialError::UnsupportedReceiptSchema(_)
        | CredentialError::SerializeReceipt(_)
        | CredentialError::ParseVerificationReceipt(_)
        | CredentialError::SerializeVerificationReceipt(_)
        | CredentialError::LogoutConfirmationRequired
        | CredentialError::LogoutModeRequired
        | CredentialError::InvalidLogoutChoice
        | CredentialError::ConfigurationOperation(_)
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
        CredentialError::SystemTime(_) => (FailureCause::Internal, None),
        CredentialError::MissingConfigDirectory => (FailureCause::Filesystem, None),
    }
}

fn configuration_diagnostics(error: &ConfigurationError) -> (FailureCause, Option<u16>) {
    match error {
        ConfigurationError::Credential(error) => credential_diagnostics(error),
        ConfigurationError::Persistence(error) => persistence_diagnostics(error),
        ConfigurationError::ReadDocument { source, .. }
        | ConfigurationError::RemoveDocument { source, .. }
        | ConfigurationError::ReadState { source, .. }
        | ConfigurationError::Prompt(source) => (io_diagnostics(source), None),
        ConfigurationError::ParseDocument { .. }
        | ConfigurationError::InvalidUtf8 { .. }
        | ConfigurationError::ParseState(_)
        | ConfigurationError::UnsupportedStateSchema(_)
        | ConfigurationError::SerializeState(_)
        | ConfigurationError::SerializeDocument(_) => (FailureCause::InvalidData, None),
        ConfigurationError::MissingStateDirectory | ConfigurationError::MissingHomeDirectory => {
            (FailureCause::Filesystem, None)
        }
        _ => (FailureCause::InvalidConfiguration, None),
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

#[cfg(test)]
mod tests {
    use super::{
        runtime_diagnostics, runtime_process_diagnostics, runtime_search_policy_diagnostics,
    };
    use crate::error::CliError;
    use nan_harness_runtime::{BridgeError, ProcessError, RuntimeError, SearchPolicyError};
    use nan_harness_telemetry::event::{FailureCategory, FailureStage};

    #[test]
    fn runtime_diagnostics_preserve_process_and_search_policy_classification() {
        assert_eq!(
            runtime_diagnostics(&RuntimeError::Process(ProcessError::Spawn(
                std::io::Error::from(std::io::ErrorKind::NotFound),
            ))),
            (
                nan_harness_telemetry::event::FailureCause::MissingExecutable,
                None
            ),
        );
        assert_eq!(
            runtime_process_diagnostics(&ProcessError::Spawn(std::io::Error::from(
                std::io::ErrorKind::PermissionDenied,
            ))),
            (
                nan_harness_telemetry::event::FailureCause::PermissionDenied,
                None,
            ),
        );
        assert_eq!(
            runtime_search_policy_diagnostics(&SearchPolicyError::RequiresDirectGateway),
            (
                nan_harness_telemetry::event::FailureCause::InvalidConfiguration,
                None,
            ),
        );
    }

    #[test]
    fn runtime_diagnostics_preserve_bridge_status_and_codes() {
        assert_eq!(
            runtime_diagnostics(&RuntimeError::Bridge(BridgeError::ModelDiscoveryStatus {
                status: reqwest::StatusCode::SERVICE_UNAVAILABLE,
                message: "redacted provider response".to_owned(),
            },)),
            (
                nan_harness_telemetry::event::FailureCause::HttpStatus,
                Some(503),
            ),
        );
        assert_eq!(
            runtime_diagnostics(&RuntimeError::Bridge(BridgeError::NoCompatibleModels)),
            (
                nan_harness_telemetry::event::FailureCause::InvalidConfiguration,
                None,
            ),
        );
    }

    #[tokio::test]
    async fn preflight_task_failures_are_internal_and_sanitized() {
        let task = tokio::spawn(std::future::pending::<()>());
        task.abort();
        let source = task.await.expect_err("aborted task should fail to join");
        let error = CliError::PreflightTaskFailed(source);

        assert_eq!(
            error.telemetry_failure(),
            (FailureCategory::Internal, FailureStage::Startup, false)
        );
        assert_eq!(
            error.telemetry_diagnostics(),
            (nan_harness_telemetry::event::FailureCause::Internal, None)
        );
    }
}
