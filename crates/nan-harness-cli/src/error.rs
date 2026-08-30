use crate::app::Cli;
use crate::commands::configuration::ConfigurationError;
use crate::commands::credentials::CredentialError;
use crate::commands::install::InstallError;
use crate::commands::persistence::PersistenceError;
use crate::commands::uninstall::UninstallError;
use crate::observability::{enrich_telemetry_context, is_harness_dry_run};
use crate::usage_evidence::UsageEvidenceError;
mod diagnostics;
use nan_harness_core::PlanError;
use nan_harness_diagnostics::{RecoveryAction, UserMessage};
use nan_harness_runtime::{DiscoveryError, ProcessError, RuntimeError};
use nan_harness_telemetry::consent::SettingsError;
use nan_harness_telemetry::diagnostic::Diagnostic;
use nan_harness_telemetry::event::{
    ErrorReportContext, Failure, FailureCategory, FailureCause, FailureStage,
    REOPEN_TERMINAL_GUIDANCE_TEXT, UserGuidance,
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
    #[error(transparent)]
    Configuration(#[from] ConfigurationError),
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
    #[error(transparent)]
    UsageEvidence(UsageEvidenceError),
}

impl CliError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Discovery(error) => error.code(),
            Self::Install(_) => InstallError::code(),
            Self::Credential(error) => error.code(),
            Self::Configuration(error) => error.code(),
            Self::Runtime(error) => error.code(),
            Self::SerializePlan(_) => "NH-CLI-003",
            Self::CurrentDirectory(_) | Self::Random(_) | Self::CredentialInvariant => "NH-CLI-005",
            Self::InvalidPlan(error) => error.code(),
            Self::TelemetrySettings(_) => "NH-TELEMETRY-001",
            Self::Update(error) => error.code(),
            Self::Persistence(error) => error.code(),
            Self::Uninstall(error) => error.code(),
            Self::UsageEvidence(_) => "NH-CLI-006",
        }
    }

    pub(crate) fn telemetry_context(&self, cli: &Cli, interactive: bool) -> ErrorReportContext {
        let (category, stage, retryable) = self.telemetry_failure();
        let (cause, http_status) = self.telemetry_diagnostics();
        let mut failure = Failure::new(self.code(), category, stage, retryable).with_cause(cause);
        if let Some(status) = http_status {
            failure = failure.with_http_status(status);
        }
        let mut context = enrich_telemetry_context(
            ErrorReportContext::new(failure, interactive).with_diagnostic(self.typed_diagnostic()),
            cli,
            !matches!(self, Self::CurrentDirectory(_)),
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

    pub(crate) fn user_message(&self, cli: &Cli) -> UserMessage {
        if matches!(self, Self::CurrentDirectory(_)) {
            UserMessage::reportable_warning(REOPEN_TERMINAL_GUIDANCE_TEXT)
        } else if let Self::Runtime(error) = self
            && let Some((requested, available)) = error.unavailable_model()
        {
            let mut commands = vec!["nan doctor".to_owned()];
            if let Some((kind, _)) = crate::runner::harness_run_arguments(cli)
                && let Some(model) = crate::runner::near_model_match(requested, available)
                    .or_else(|| available.first().cloned())
            {
                commands.push(format!("nan {} --model {model}", kind.binary_name()));
            }
            UserMessage::error(self.code(), self.to_string()).with_action(
                RecoveryAction::new("Choose a model from your live catalog:")
                    .with_commands(commands),
            )
        } else if matches!(self, Self::Install(error) if error.is_runtime_precondition())
            || matches!(self, Self::Credential(_) | Self::Configuration(_))
        {
            UserMessage::setup_required(self.to_string())
        } else {
            UserMessage::error(self.code(), self.to_string())
        }
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
            Self::CredentialInvariant | Self::InvalidPlan(_) => {
                (FailureCause::InvalidConfiguration, None)
            }
            Self::Runtime(error) => runtime_diagnostics(error),
            Self::CurrentDirectory(source) => (io_diagnostics(source), None),
            Self::SerializePlan(_) => (FailureCause::Serialization, None),
            Self::Random(_) => (FailureCause::Internal, None),
            Self::TelemetrySettings(_) | Self::Uninstall(_) | Self::UsageEvidence(_) => {
                (FailureCause::Filesystem, None)
            }
            Self::Update(error) => update_diagnostics(error),
            Self::Persistence(error) => persistence_diagnostics(error),
        }
    }

    fn typed_diagnostic(&self) -> Diagnostic {
        diagnostics::typed_diagnostic(self)
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
        | RuntimeError::TerminateProcess(source)
        | RuntimeError::SearchPolicy(nan_harness_runtime::SearchPolicyError::ReadConfiguration {
            source,
            ..
        }) => (io_diagnostics(source), None),
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
        RuntimeError::SearchPolicy(_) => (FailureCause::InvalidConfiguration, None),
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
    use super::super::usage_evidence::UsageEvidenceError;
    use super::{CliError, REOPEN_TERMINAL_GUIDANCE_TEXT};
    use crate::app::{Cli, Command, DirectHarnessRunArgs, HarnessRunArgs, WebSearchArgs};
    use crate::commands::credentials::CredentialError;
    use crate::commands::install::InstallError;
    use nan_harness_core::{HarnessKind, PlanError};
    use nan_harness_runtime::update::UpdateError;
    use nan_harness_runtime::{BridgeError, DiscoveryError, RuntimeError};
    use semver::Version;
    use std::path::PathBuf;

    #[test]
    fn local_runtime_preconditions_are_not_reportable() {
        let error = CliError::Install(InstallError::RuntimeUnsupported {
            harness: HarnessKind::DeepSeekHarness,
            detected: "v20.19.4".to_owned(),
            minimum: Version::new(22, 19, 0),
            hint: "actionable guidance".to_owned(),
        });

        let message = error.user_message(&dry_run_cli());
        assert_eq!(message.code, None);
        assert!(!message.is_reportable());
    }

    #[test]
    fn installer_failures_remain_reportable() {
        let error = CliError::Install(InstallError::InstallerFailed {
            harness: HarnessKind::DeepSeekHarness,
            interpreter: "npm",
            exit_code: Some(1),
        });

        let message = error.user_message(&dry_run_cli());
        assert_eq!(message.code.as_deref(), Some("NH-INSTALL-001"));
        assert!(message.is_reportable());
    }

    #[test]
    fn credential_guidance_is_not_reportable() {
        let message =
            CliError::Credential(CredentialError::MissingCredential).user_message(&dry_run_cli());

        assert_eq!(message.code, None);
        assert!(!message.is_reportable());
    }

    #[test]
    fn expected_dry_run_validation_errors_are_not_reportable_to_telemetry() {
        let cli = dry_run_cli();
        let discovery = CliError::Discovery(DiscoveryError::InvalidExecutable(PathBuf::from(
            "/tmp/kimi",
        )));
        let plan = CliError::InvalidPlan(PlanError::InvalidField {
            field: "process.arguments",
            message: "argument conflicts with routing".to_owned(),
        });

        assert!(!discovery.should_report_telemetry(&cli));
        assert!(!plan.should_report_telemetry(&cli));
    }

    #[test]
    fn missing_update_channel_is_not_reportable_to_telemetry() {
        let cli = Cli {
            command: Command::Update,
        };
        let error = CliError::Update(UpdateError::UpdateChannelUnavailable);

        assert!(!error.should_report_telemetry(&cli));
    }

    #[test]
    fn private_usage_evidence_failures_are_generic_and_not_reportable() {
        let error = CliError::UsageEvidence(UsageEvidenceError);
        let message = error.user_message(&dry_run_cli()).render_terminal();

        assert!(!error.should_report_telemetry(&dry_run_cli()));
        assert_eq!(
            message,
            "error [NH-CLI-006]: could not write private usage evidence"
        );
        assert!(!message.contains("NAN_HARNESS_INTERNAL_CANARY_USAGE_FILE"));
        assert!(!message.contains("/private"));
    }

    #[test]
    fn real_discovery_failures_remain_reportable_during_dry_run() {
        let cli = dry_run_cli();
        let error = CliError::Discovery(DiscoveryError::VersionCommandFailed {
            command: "kimi --version".to_owned(),
            exit_code: Some(1),
        });

        assert!(error.should_report_telemetry(&cli));
    }

    #[test]
    fn current_directory_failures_show_recovery_without_an_error_code() {
        let error =
            CliError::CurrentDirectory(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        let message = error.user_message(&dry_run_cli());

        assert!(message.is_reportable());
        assert_eq!(message.code, None);
        assert_eq!(
            message.render_terminal(),
            "warning: The current terminal session cannot access the project directory. Please close this terminal, open a new terminal in the project directory, and try again."
        );
    }

    #[test]
    fn current_directory_reports_include_the_exact_guidance_and_skip_discovery() {
        let error =
            CliError::CurrentDirectory(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        let cli = Cli {
            command: Command::Pi(DirectHarnessRunArgs {
                run: HarnessRunArgs {
                    model: None,
                    executable: None,
                    provider_base_url: None,
                    allow_unsupported: false,
                    allow_untested: false,
                    search: WebSearchArgs::default(),
                    dry_run: false,
                    arguments: Vec::new(),
                },
                no_chat_gateway: false,
            }),
        };
        let context = error.telemetry_context(&cli, true);
        let guidance = context
            .user_guidance()
            .expect("current directory failures should include user guidance");

        assert!(guidance.shown());
        assert_eq!(guidance.id(), "reopen-terminal");
        assert_eq!(guidance.text(), REOPEN_TERMINAL_GUIDANCE_TEXT);
        assert_eq!(
            context.diagnostic_reason().as_str(),
            "filesystem-operation-failed"
        );
    }

    #[test]
    fn unavailable_models_offer_harness_specific_recovery() {
        for (harness, command) in [
            ("claude", "nan claude --model qwen3.6"),
            ("codex", "nan codex --model qwen3.6"),
            ("qwen", "nan qwen --model qwen3.6"),
            ("dsh", "nan dsh --model qwen3.6"),
            ("fx", "nan fx --model qwen3.6"),
        ] {
            let cli = Cli::try_parse_checked_from(["nan", harness])
                .expect("harness command should parse");
            let error = CliError::Runtime(RuntimeError::Bridge(
                BridgeError::SelectedModelUnavailable {
                    model: "qwen36".to_owned(),
                    available: vec!["qwen3.6".to_owned(), "glm5.3-flash".to_owned()],
                },
            ));
            let rendered = error.user_message(&cli).render_terminal();
            assert!(rendered.contains(&format!(
                "Choose a model from your live catalog:\n  nan doctor\n  {command}"
            )));
        }
    }

    #[test]
    fn empty_model_catalog_recovery_only_runs_doctor() {
        let cli =
            Cli::try_parse_checked_from(["nan", "codex"]).expect("Codex command should parse");
        let error = CliError::Runtime(RuntimeError::Bridge(
            BridgeError::SelectedModelUnavailable {
                model: "old-model".to_owned(),
                available: Vec::new(),
            },
        ));
        let rendered = error.user_message(&cli).render_terminal();
        assert!(rendered.ends_with("Choose a model from your live catalog:\n  nan doctor"));
        assert!(!rendered.contains(" --model "));
    }

    fn dry_run_cli() -> Cli {
        Cli {
            command: Command::Kimi(DirectHarnessRunArgs {
                run: HarnessRunArgs {
                    model: None,
                    executable: None,
                    provider_base_url: None,
                    allow_unsupported: false,
                    allow_untested: false,
                    search: WebSearchArgs::default(),
                    dry_run: true,
                    arguments: Vec::new(),
                },
                no_chat_gateway: false,
            }),
        }
    }
}
