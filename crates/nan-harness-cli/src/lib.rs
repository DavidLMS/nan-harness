#![forbid(unsafe_code)]

mod app;
mod commands;
mod runner;

use app::{Cli, Command, HarnessRunArgs, PersistentHarnessRunArgs};
use clap::Parser;
use commands::credentials::CredentialError;
use commands::install::{
    InstallDecision, InstallError, executable_from_known_locations, install_spec, offer_install,
};
use commands::persistence::{
    IntegrationChange, PersistenceError, PersistenceManager, RemovalOutcome,
    effective_provider_base_url,
};
use commands::uninstall::UninstallError;
use nan_harness_adapters::{
    AiderAdapter, ClaudeCodeAdapter, ClineAdapter, CodexAdapter, DeepSeekHarnessAdapter, FxAdapter,
    GooseAdapter, HermesAdapter, KimiCodeAdapter, OpenClawAdapter, OpenCodeAdapter,
    PersistentAiderAdapter, PersistentDeepSeekHarnessAdapter, PersistentPiAdapter,
    PersistentPrimeAgentAdapter, PersistentQwenCodeAdapter, PiAdapter, PrimeAgentAdapter,
    QwenCodeAdapter,
};
use nan_harness_core::launch_plan::{LaunchId, ObservabilityFormat};
use nan_harness_core::model::{ModelAvailability, ProfileSource, QualificationStatus};
use nan_harness_core::{
    HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError, ResolvedModel,
    build_validated_plan,
};
use nan_harness_runtime::{
    CancellationToken, DiscoveryError, DiscoveryOptions, ProcessError, ResolvedConfig,
    RuntimeError, SignalKind, Supervisor, discover_harness,
};
use nan_harness_telemetry::TelemetryReporter;
use nan_harness_telemetry::analytics::{DEFAULT_USAGE_EXPORT_TIMEOUT, UmamiExporter, UsageEvent};
use nan_harness_telemetry::consent::{SettingsError, TelemetrySettingsStore};
use nan_harness_telemetry::event::{
    CompatibilityStatus as TelemetryCompatibilityStatus, ErrorReportContext, Failure,
    FailureCategory, FailureCause, FailureStage, HarnessIdentity as TelemetryHarnessIdentity,
    HarnessKind as TelemetryHarnessKind, OperationContext, OperationKind,
    Transport as TelemetryTransport,
};
use nan_harness_telemetry::glitchtip::{DEFAULT_EXPORT_TIMEOUT, GlitchTipExporter};
use nan_harness_telemetry::panic::{PendingReportStore, install_panic_hook};
use std::fmt::Write as _;
use std::io::IsTerminal as _;
use std::path::Path;
use std::process::ExitCode;
use thiserror::Error;

const DEFAULT_MODEL_ID: &str = "qwen3.6";

pub async fn main_entry() -> ExitCode {
    let cli = Cli::parse();
    let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    let aggregate_doctor = matches!(
        &cli.command,
        Command::Doctor(arguments) if arguments.harness.is_none()
    );
    let disables_observability = aggregate_doctor
        || matches!(
            cli.command,
            Command::Auth { .. } | Command::Uninstall(_) | Command::RecordInstallation(_)
        );
    if !matches!(
        cli.command,
        Command::Update | Command::Uninstall(_) | Command::RecordInstallation(_)
    ) && !aggregate_doctor
    {
        match commands::update::check_on_start(interactive).await {
            Ok(Some(exit_code)) => return exit_code_from_i32(exit_code),
            Ok(None) => {}
            Err(error) => eprintln!(
                "warning [{}]: update failed; continuing with the installed version: {error}",
                error.code()
            ),
        }
    }
    if !matches!(
        cli.command,
        Command::Update | Command::Uninstall(_) | Command::RecordInstallation(_)
    ) && let Err(error) = nan_harness_runtime::refresh_compatibility_manifest().await
    {
        if aggregate_doctor {
            eprintln!(
                "warning [{}]: compatibility metadata refresh failed; continuing with cached or embedded values",
                error.code()
            );
        } else {
            eprintln!(
                "warning [{}]: compatibility metadata refresh failed; continuing with cached or embedded values: {error}",
                error.code()
            );
        }
    }
    let telemetry = if disables_observability {
        None
    } else {
        telemetry_reporter()
    };
    if let Some(reporter) = &telemetry {
        let telemetry_enabled = reporter
            .settings()
            .load()
            .is_ok_and(|settings| settings.enabled());
        install_panic_hook(
            reporter.pending().clone(),
            telemetry_enabled,
            panic_telemetry_context(&cli, interactive),
        );
        if !matches!(cli.command, Command::Telemetry { .. }) {
            let mut input = std::io::stdin().lock();
            let mut output = std::io::stderr().lock();
            let _ = reporter
                .process_pending(interactive, &mut input, &mut output)
                .await;
        }
    }
    let usage_analytics_task = start_usage_analytics(&cli, telemetry.as_ref());
    let exit_code = match runner::run(&cli, interactive).await {
        Ok(exit_code) => exit_code_from_i32(exit_code),
        Err(error) => {
            eprintln!("error [{}]: {error}", error.code());
            if let Some(reporter) = &telemetry {
                let context = error.telemetry_context(&cli, interactive);
                let mut input = std::io::stdin().lock();
                let mut output = std::io::stderr().lock();
                let _ = reporter.report(context, &mut input, &mut output).await;
            }
            ExitCode::FAILURE
        }
    };
    if let Some(task) = usage_analytics_task {
        let _ = task.await;
    }
    exit_code
}

fn telemetry_reporter() -> Option<TelemetryReporter<GlitchTipExporter>> {
    let settings = TelemetrySettingsStore::from_environment().ok()?;
    let pending = PendingReportStore::new(settings.directory());
    let dsn = std::env::var("NAN_HARNESS_GLITCHTIP_DSN")
        .ok()
        .or_else(|| option_env!("NAN_HARNESS_GLITCHTIP_DSN").map(ToOwned::to_owned));
    let exporter = dsn
        .as_deref()
        .and_then(|value| GlitchTipExporter::new(value, DEFAULT_EXPORT_TIMEOUT).ok());
    Some(TelemetryReporter::new(settings, pending, exporter))
}

fn start_usage_analytics(
    cli: &Cli,
    telemetry: Option<&TelemetryReporter<GlitchTipExporter>>,
) -> Option<tokio::task::JoinHandle<()>> {
    if matches!(cli.command, Command::Telemetry { .. }) {
        return None;
    }
    let installation_id = telemetry?
        .settings()
        .active_installation_id()
        .ok()
        .flatten()?;
    let base_url = configured_value(
        "NAN_HARNESS_UMAMI_URL",
        option_env!("NAN_HARNESS_UMAMI_URL"),
    )?;
    let website_id = configured_value(
        "NAN_HARNESS_UMAMI_WEBSITE_ID",
        option_env!("NAN_HARNESS_UMAMI_WEBSITE_ID"),
    )?;
    let exporter = UmamiExporter::new(&base_url, &website_id, DEFAULT_USAGE_EXPORT_TIMEOUT).ok()?;
    let event = UsageEvent::new(
        telemetry_harness(cli),
        telemetry_operation(cli).kind(),
        telemetry_transport(cli),
    );
    Some(tokio::spawn(async move {
        let _ = exporter.export(&installation_id, event).await;
    }))
}

fn configured_value(name: &str, embedded: Option<&str>) -> Option<String> {
    match std::env::var(name) {
        Ok(value) if value.is_empty() => None,
        Ok(value) => Some(value),
        Err(_) => embedded
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
    }
}

fn exit_code_from_i32(value: i32) -> ExitCode {
    ExitCode::from(u8::try_from(value.clamp(0, 255)).unwrap_or(1))
}

#[derive(Debug, Error)]
enum CliError {
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
    const fn code(&self) -> &'static str {
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

    fn telemetry_context(&self, cli: &Cli, interactive: bool) -> ErrorReportContext {
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

fn panic_telemetry_context(cli: &Cli, interactive: bool) -> ErrorReportContext {
    enrich_telemetry_context(
        ErrorReportContext::new(Failure::panic(), interactive),
        cli,
        false,
    )
}

fn enrich_telemetry_context(
    mut context: ErrorReportContext,
    cli: &Cli,
    detect_version: bool,
) -> ErrorReportContext {
    if let Some(harness) = telemetry_harness_identity(cli, detect_version) {
        context = context.with_harness(harness);
    }
    if let Some(transport) = telemetry_transport(cli) {
        context = context.with_transport(transport);
    }
    context.with_operation(telemetry_operation(cli))
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

fn telemetry_harness_identity(cli: &Cli, detect_version: bool) -> Option<TelemetryHarnessIdentity> {
    let kind = telemetry_harness(cli)?;
    if !detect_version {
        return Some(TelemetryHarnessIdentity::new(kind, None));
    }
    let (core_kind, executable, options) = telemetry_discovery_input(cli)?;
    let Ok(report) = discover_harness(core_kind, executable, options) else {
        return Some(TelemetryHarnessIdentity::new(kind, None));
    };
    let version = normalized_version(&report.harness.detected_version);
    Some(
        TelemetryHarnessIdentity::new(kind, version)
            .with_compatibility(telemetry_compatibility(report.harness.version_status)),
    )
}

fn telemetry_discovery_input(cli: &Cli) -> Option<(HarnessKind, Option<&Path>, DiscoveryOptions)> {
    if let Command::Doctor(arguments) = &cli.command {
        return arguments.harness.map(|harness| {
            (
                harness,
                arguments.executable.as_deref(),
                DiscoveryOptions {
                    allow_unsupported: true,
                    allow_untested: true,
                },
            )
        });
    }
    let (kind, arguments) = runner::harness_run_arguments(cli)?;
    Some((
        kind,
        arguments.executable.as_deref(),
        DiscoveryOptions {
            allow_unsupported: true,
            allow_untested: true,
        },
    ))
}

fn normalized_version(output: &str) -> Option<String> {
    output.split_whitespace().find_map(|token| {
        let candidate = token
            .trim_matches(|character: char| !character.is_ascii_alphanumeric() && character != '.')
            .trim_start_matches('v');
        semver::Version::parse(candidate)
            .ok()
            .map(|version| version.to_string())
    })
}

const fn telemetry_compatibility(
    status: nan_harness_core::harness::VersionStatus,
) -> TelemetryCompatibilityStatus {
    use nan_harness_core::harness::VersionStatus;

    match status {
        VersionStatus::Tested => TelemetryCompatibilityStatus::Tested,
        VersionStatus::Supported => TelemetryCompatibilityStatus::Supported,
        VersionStatus::NewerUntested => TelemetryCompatibilityStatus::NewerUntested,
        VersionStatus::OlderUnsupported => TelemetryCompatibilityStatus::OlderUnsupported,
        VersionStatus::Unparseable => TelemetryCompatibilityStatus::Unparseable,
    }
}

fn telemetry_operation(cli: &Cli) -> OperationContext {
    match &cli.command {
        Command::OpenCode(arguments)
        | Command::Pi(arguments)
        | Command::Prime(arguments)
        | Command::DeepSeek(arguments)
        | Command::Qwen(arguments)
        | Command::Aider(arguments) => {
            let kind = if arguments.unpersist {
                OperationKind::HarnessUnpersist
            } else if arguments.persist {
                OperationKind::HarnessPersist
            } else if arguments.run.dry_run {
                OperationKind::HarnessDryRun
            } else {
                OperationKind::HarnessRun
            };
            OperationContext::new(kind)
        }
        Command::Claude(arguments)
        | Command::Codex(arguments)
        | Command::Hermes(arguments)
        | Command::OpenClaw(arguments)
        | Command::Cline(arguments)
        | Command::Kimi(arguments)
        | Command::Goose(arguments)
        | Command::Fx(arguments) => {
            let kind = if arguments.dry_run {
                OperationKind::HarnessDryRun
            } else {
                OperationKind::HarnessRun
            };
            OperationContext::new(kind)
        }
        Command::Doctor(_) => OperationContext::new(OperationKind::Doctor),
        Command::Update | Command::RecordInstallation(_) => {
            OperationContext::new(OperationKind::Update)
        }
        Command::Uninstall(_) => OperationContext::new(OperationKind::Uninstall),
        Command::Auth { .. } | Command::Telemetry { .. } => {
            OperationContext::new(OperationKind::TelemetryConfiguration)
        }
    }
}

const fn telemetry_harness(cli: &Cli) -> Option<TelemetryHarnessKind> {
    match &cli.command {
        Command::Claude(_) => Some(TelemetryHarnessKind::ClaudeCode),
        Command::Codex(_) => Some(TelemetryHarnessKind::Codex),
        Command::OpenCode(_) => Some(TelemetryHarnessKind::OpenCode),
        Command::Hermes(_) => Some(TelemetryHarnessKind::Hermes),
        Command::Pi(_) => Some(TelemetryHarnessKind::Pi),
        Command::Prime(_) => Some(TelemetryHarnessKind::PrimeAgent),
        Command::DeepSeek(_) => Some(TelemetryHarnessKind::DeepSeekHarness),
        Command::OpenClaw(_) => Some(TelemetryHarnessKind::OpenClaw),
        Command::Cline(_) => Some(TelemetryHarnessKind::Cline),
        Command::Qwen(_) => Some(TelemetryHarnessKind::QwenCode),
        Command::Kimi(_) => Some(TelemetryHarnessKind::KimiCode),
        Command::Aider(_) => Some(TelemetryHarnessKind::Aider),
        Command::Goose(_) => Some(TelemetryHarnessKind::Goose),
        Command::Fx(_) => Some(TelemetryHarnessKind::Fx),
        Command::Doctor(arguments) => match arguments.harness {
            Some(harness) => Some(match harness {
                HarnessKind::ClaudeCode => TelemetryHarnessKind::ClaudeCode,
                HarnessKind::Codex => TelemetryHarnessKind::Codex,
                HarnessKind::OpenCode => TelemetryHarnessKind::OpenCode,
                HarnessKind::Hermes => TelemetryHarnessKind::Hermes,
                HarnessKind::Pi => TelemetryHarnessKind::Pi,
                HarnessKind::PrimeAgent => TelemetryHarnessKind::PrimeAgent,
                HarnessKind::DeepSeekHarness => TelemetryHarnessKind::DeepSeekHarness,
                HarnessKind::OpenClaw => TelemetryHarnessKind::OpenClaw,
                HarnessKind::Cline => TelemetryHarnessKind::Cline,
                HarnessKind::QwenCode => TelemetryHarnessKind::QwenCode,
                HarnessKind::KimiCode => TelemetryHarnessKind::KimiCode,
                HarnessKind::Aider => TelemetryHarnessKind::Aider,
                HarnessKind::Goose => TelemetryHarnessKind::Goose,
                HarnessKind::Fx => TelemetryHarnessKind::Fx,
            }),
            None => None,
        },
        Command::Auth { .. }
        | Command::Update
        | Command::Uninstall(_)
        | Command::Telemetry { .. }
        | Command::RecordInstallation(_) => None,
    }
}

const fn telemetry_transport(cli: &Cli) -> Option<TelemetryTransport> {
    match cli.command {
        Command::Claude(_) => Some(TelemetryTransport::AnthropicBridge),
        Command::Codex(_) => Some(TelemetryTransport::ResponsesBridge),
        Command::OpenCode(_)
        | Command::Hermes(_)
        | Command::Pi(_)
        | Command::Prime(_)
        | Command::DeepSeek(_)
        | Command::OpenClaw(_)
        | Command::Cline(_)
        | Command::Qwen(_)
        | Command::Kimi(_)
        | Command::Aider(_)
        | Command::Goose(_) => Some(TelemetryTransport::DirectChat),
        Command::Fx(_) => Some(TelemetryTransport::FxGatewayBridge),
        Command::Doctor(_)
        | Command::Auth { .. }
        | Command::Update
        | Command::Uninstall(_)
        | Command::Telemetry { .. }
        | Command::RecordInstallation(_) => None,
    }
}
