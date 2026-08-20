#![forbid(unsafe_code)]

mod app;
mod commands;

use app::{Cli, Command, DoctorArgs, HarnessRunArgs, PersistentHarnessRunArgs};
use clap::Parser;
use commands::install::{
    InstallDecision, InstallError, executable_from_known_locations, install_spec, offer_install,
};
use commands::persistence::{
    IntegrationChange, PersistenceError, PersistenceManager, RemovalOutcome,
    effective_provider_base_url,
};
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
    HarnessAdapter, HarnessKind, PlanContext, PlanError, ResolvedModel, build_validated_plan,
};
use nan_harness_runtime::{
    CancellationToken, ConfigError, ConfigOverrides, ConfigResolver, DiscoveryError,
    DiscoveryOptions, ProcessEnvironment, RuntimeError, SignalKind, Supervisor, discover_harness,
};
use nan_harness_telemetry::TelemetryReporter;
use nan_harness_telemetry::consent::{SettingsError, TelemetrySettingsStore};
use nan_harness_telemetry::event::{
    ErrorReportContext, Failure, FailureCategory, FailureStage,
    HarnessIdentity as TelemetryHarnessIdentity, HarnessKind as TelemetryHarnessKind,
    Transport as TelemetryTransport,
};
use nan_harness_telemetry::glitchtip::{DEFAULT_EXPORT_TIMEOUT, GlitchTipExporter};
use nan_harness_telemetry::panic::{PendingReportStore, install_panic_hook};
use std::fmt::Write as _;
use std::fs;
use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use thiserror::Error;

const DEFAULT_MODEL_ID: &str = "qwen3.6";

pub async fn main_entry() -> ExitCode {
    let cli = Cli::parse();
    let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    if !matches!(cli.command, Command::Update) {
        match commands::update::check_on_start(interactive).await {
            Ok(Some(exit_code)) => return exit_code_from_i32(exit_code),
            Ok(None) => {}
            Err(error) => eprintln!(
                "warning [{}]: update failed; continuing with the installed version: {error}",
                error.code()
            ),
        }
    }
    if !matches!(cli.command, Command::Update)
        && let Err(error) = nan_harness_runtime::refresh_compatibility_manifest().await
    {
        eprintln!(
            "warning [{}]: compatibility metadata refresh failed; continuing with cached or embedded values: {error}",
            error.code()
        );
    }
    let telemetry = telemetry_reporter();
    if let Some(reporter) = &telemetry {
        let telemetry_enabled = reporter
            .settings()
            .load()
            .is_ok_and(|settings| settings.enabled());
        install_panic_hook(reporter.pending().clone(), telemetry_enabled, interactive);
        if !matches!(cli.command, Command::Telemetry { .. }) {
            let mut input = std::io::stdin().lock();
            let mut output = std::io::stderr().lock();
            let _ = reporter
                .process_pending(interactive, &mut input, &mut output)
                .await;
        }
    }
    match run(&cli).await {
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
    }
}

async fn run(cli: &Cli) -> Result<i32, CliError> {
    match &cli.command {
        Command::Claude(arguments) => {
            run_harness(HarnessKind::ClaudeCode, arguments, &ClaudeCodeAdapter).await
        }
        Command::Codex(arguments) => {
            run_harness(HarnessKind::Codex, arguments, &CodexAdapter).await
        }
        Command::OpenCode(arguments) => run_opencode(arguments).await,
        Command::Hermes(arguments) => {
            run_harness(HarnessKind::Hermes, arguments, &HermesAdapter).await
        }
        Command::Pi(arguments) => run_pi(arguments).await,
        Command::Prime(arguments) => run_prime_agent(arguments).await,
        Command::DeepSeek(arguments) => run_deepseek_harness(arguments).await,
        Command::OpenClaw(arguments) => {
            run_harness(HarnessKind::OpenClaw, arguments, &OpenClawAdapter).await
        }
        Command::Cline(arguments) => {
            run_harness(HarnessKind::Cline, arguments, &ClineAdapter).await
        }
        Command::Qwen(arguments) => run_qwen_code(arguments).await,
        Command::Kimi(arguments) => {
            run_harness(HarnessKind::KimiCode, arguments, &KimiCodeAdapter).await
        }
        Command::Aider(arguments) => run_aider(arguments).await,
        Command::Goose(arguments) => {
            run_harness(HarnessKind::Goose, arguments, &GooseAdapter).await
        }
        Command::Fx(arguments) => run_harness(HarnessKind::Fx, arguments, &FxAdapter).await,
        Command::Doctor(arguments) => {
            run_doctor(arguments)?;
            Ok(0)
        }
        Command::Update => {
            commands::update::run_manual().await?;
            Ok(0)
        }
        Command::ValidatePlan { path } => {
            validate_plan(path)?;
            Ok(0)
        }
        Command::Telemetry { command } => {
            commands::telemetry::run(*command)?;
            Ok(0)
        }
    }
}

async fn run_pi(arguments: &PersistentHarnessRunArgs) -> Result<i32, CliError> {
    if arguments.unpersist {
        let manager = PersistenceManager::from_environment()?;
        print_removal("Pi", manager.unpersist_pi()?);
        return Ok(0);
    }
    let persisted = if arguments.persist {
        let manager = PersistenceManager::from_environment()?;
        let base_url = effective_provider_base_url(arguments.run.provider_base_url.as_deref());
        print_integration("Pi", manager.persist_pi(&base_url)?);
        true
    } else {
        PersistenceManager::from_environment().is_ok_and(|manager| manager.pi_is_active())
    };
    if persisted {
        run_harness(HarnessKind::Pi, &arguments.run, &PersistentPiAdapter).await
    } else {
        run_harness(HarnessKind::Pi, &arguments.run, &PiAdapter).await
    }
}

async fn run_opencode(arguments: &PersistentHarnessRunArgs) -> Result<i32, CliError> {
    if arguments.unpersist {
        let manager = PersistenceManager::from_environment()?;
        print_removal("OpenCode", manager.unpersist_opencode()?);
        return Ok(0);
    }
    if arguments.persist {
        let manager = PersistenceManager::from_environment()?;
        let config = resolve_config(&arguments.run)?;
        print_integration("OpenCode", manager.persist_opencode(&config).await?);
    }
    run_harness(HarnessKind::OpenCode, &arguments.run, &OpenCodeAdapter).await
}

async fn run_prime_agent(arguments: &PersistentHarnessRunArgs) -> Result<i32, CliError> {
    if arguments.unpersist {
        let manager = PersistenceManager::from_environment()?;
        print_removal("Prime Agent", manager.unpersist_prime_agent()?);
        return Ok(0);
    }
    let persisted = if arguments.persist {
        let manager = PersistenceManager::from_environment()?;
        let base_url = effective_provider_base_url(arguments.run.provider_base_url.as_deref());
        print_integration("Prime Agent", manager.persist_prime_agent(&base_url)?);
        true
    } else {
        PersistenceManager::from_environment().is_ok_and(|manager| manager.prime_agent_is_active())
    };
    if persisted {
        run_harness(
            HarnessKind::PrimeAgent,
            &arguments.run,
            &PersistentPrimeAgentAdapter,
        )
        .await
    } else {
        run_harness(HarnessKind::PrimeAgent, &arguments.run, &PrimeAgentAdapter).await
    }
}

async fn run_qwen_code(arguments: &PersistentHarnessRunArgs) -> Result<i32, CliError> {
    if arguments.unpersist {
        let manager = PersistenceManager::from_environment()?;
        print_removal("Qwen Code", manager.unpersist_qwen_code()?);
        return Ok(0);
    }
    let persisted = if arguments.persist {
        let manager = PersistenceManager::from_environment()?;
        let config = resolve_config(&arguments.run)?;
        print_integration("Qwen Code", manager.persist_qwen_code(&config).await?);
        true
    } else {
        arguments.run.provider_base_url.is_none()
            && PersistenceManager::from_environment()
                .is_ok_and(|manager| manager.qwen_code_is_active())
    };
    if persisted {
        run_harness(
            HarnessKind::QwenCode,
            &arguments.run,
            &PersistentQwenCodeAdapter,
        )
        .await
    } else {
        run_harness(HarnessKind::QwenCode, &arguments.run, &QwenCodeAdapter).await
    }
}

async fn run_deepseek_harness(arguments: &PersistentHarnessRunArgs) -> Result<i32, CliError> {
    if arguments.unpersist {
        let manager = PersistenceManager::from_environment()?;
        print_removal("DeepSeek Harness", manager.unpersist_deepseek_harness()?);
        return Ok(0);
    }
    let persisted = if arguments.persist {
        let manager = PersistenceManager::from_environment()?;
        let config = resolve_config(&arguments.run)?;
        print_integration(
            "DeepSeek Harness",
            manager.persist_deepseek_harness(&config).await?,
        );
        true
    } else {
        arguments.run.provider_base_url.is_none()
            && PersistenceManager::from_environment()
                .is_ok_and(|manager| manager.deepseek_harness_is_active())
    };
    if persisted {
        run_harness(
            HarnessKind::DeepSeekHarness,
            &arguments.run,
            &PersistentDeepSeekHarnessAdapter,
        )
        .await
    } else {
        run_harness(
            HarnessKind::DeepSeekHarness,
            &arguments.run,
            &DeepSeekHarnessAdapter,
        )
        .await
    }
}

async fn run_aider(arguments: &PersistentHarnessRunArgs) -> Result<i32, CliError> {
    if arguments.unpersist {
        let manager = PersistenceManager::from_environment()?;
        print_removal("Aider", manager.unpersist_aider()?);
        return Ok(0);
    }
    let persisted = if arguments.persist {
        let manager = PersistenceManager::from_environment()?;
        let config = resolve_config(&arguments.run)?;
        print_integration("Aider", manager.persist_aider(&config).await?);
        true
    } else {
        arguments.run.provider_base_url.is_none()
            && PersistenceManager::from_environment().is_ok_and(|manager| manager.aider_is_active())
    };
    if persisted {
        run_harness(HarnessKind::Aider, &arguments.run, &PersistentAiderAdapter).await
    } else {
        run_harness(HarnessKind::Aider, &arguments.run, &AiderAdapter).await
    }
}

fn print_integration(harness: &str, change: IntegrationChange) {
    if change.changed {
        println!(
            "NaN provider persisted for {harness} at '{}'.",
            change.path.display()
        );
    } else {
        println!(
            "NaN provider is already persisted for {harness} at '{}'.",
            change.path.display()
        );
    }
    if let Some(backup) = change.backup {
        println!("Backup created at '{}'.", backup.display());
    }
    for path in change.additional_paths {
        println!("Additional managed configuration: '{}'.", path.display());
    }
}

fn print_removal(harness: &str, outcome: RemovalOutcome) {
    match outcome {
        RemovalOutcome::Removed => println!("NaN provider removed from {harness}."),
        RemovalOutcome::NotConfigured => {
            println!("No persistent NaN provider is configured for {harness}.");
        }
    }
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

async fn run_harness(
    kind: HarnessKind,
    arguments: &HarnessRunArgs,
    adapter: &dyn HarnessAdapter,
) -> Result<i32, CliError> {
    let Some(discovery) = discover_or_install_harness(kind, arguments)? else {
        return Ok(0);
    };
    for warning in &discovery.warnings {
        eprintln!("warning: {warning}");
    }
    let working_directory = std::env::current_dir().map_err(CliError::CurrentDirectory)?;
    let model_id = model_for_launch(kind, arguments);
    let context = PlanContext {
        launch_id: generate_launch_id()?,
        harness: discovery.harness,
        model: requested_model(&model_id),
        working_directory: working_directory.to_string_lossy().into_owned(),
        user_arguments: arguments.arguments.clone(),
        observability_format: ObservabilityFormat::Human,
    };
    let plan = build_validated_plan(adapter, &context).map_err(CliError::InvalidPlan)?;
    if arguments.dry_run {
        let normalized = serde_json::to_string_pretty(&plan).map_err(CliError::SerializePlan)?;
        println!("{normalized}");
        return Ok(0);
    }

    let config = resolve_config(arguments)?;
    let cancellation = CancellationToken::new();
    let signal_task = install_signal_handlers(cancellation.clone());
    let result = Supervisor::new()
        .execute(&plan, &config, &cancellation)
        .await;
    signal_task.abort();
    let report = result?;
    if kind == HarnessKind::Codex
        && let Some(model) = report.selected_model.as_deref()
        && let Ok(manager) = PersistenceManager::from_environment()
        && let Err(error) = manager.save_last_codex_model(model)
    {
        eprintln!("warning: could not save the last Codex model: {error}");
    }
    Ok(report.exit_code)
}

fn model_for_launch(kind: HarnessKind, arguments: &HarnessRunArgs) -> String {
    if let Some(model) = &arguments.model {
        return model.clone();
    }
    if kind == HarnessKind::Codex
        && let Ok(manager) = PersistenceManager::from_environment()
        && let Ok(Some(model)) = manager.last_codex_model()
    {
        return model;
    }
    DEFAULT_MODEL_ID.to_owned()
}

fn discover_or_install_harness(
    kind: HarnessKind,
    arguments: &HarnessRunArgs,
) -> Result<Option<nan_harness_runtime::DiscoveryReport>, CliError> {
    let options = DiscoveryOptions {
        allow_unsupported: arguments.allow_unsupported,
        allow_untested: arguments.allow_untested,
    };
    match discover_harness(kind, arguments.executable.as_deref(), options) {
        Ok(report) => Ok(Some(report)),
        Err(DiscoveryError::ExecutableNotFound(_))
            if install_spec(kind).is_some() && arguments.executable.is_none() =>
        {
            if let Some(executable) = executable_from_known_locations(kind) {
                return discover_harness(kind, Some(&executable), options)
                    .map(Some)
                    .map_err(CliError::from);
            }
            if arguments.dry_run {
                eprintln!("{kind} was not found on PATH; dry-run does not install harnesses.");
                eprintln!("Run `nan doctor {kind}` after installing the official release.");
                return Ok(None);
            }
            match offer_install(kind)? {
                InstallDecision::NotInteractive => {
                    report_install_skipped(kind, "installation requires an interactive terminal");
                    Err(DiscoveryError::ExecutableNotFound(kind.binary_name().to_owned()).into())
                }
                InstallDecision::Declined => {
                    report_install_skipped(kind, "installation was declined");
                    Ok(None)
                }
                InstallDecision::Installed => {
                    let executable = executable_from_known_locations(kind);
                    match discover_harness(kind, executable.as_deref(), options) {
                        Ok(report) => Ok(Some(report)),
                        Err(error @ DiscoveryError::ExecutableNotFound(_)) => {
                            eprintln!(
                                "{kind} was installed, but its executable is not visible on PATH."
                            );
                            Err(error.into())
                        }
                        Err(error) => Err(error.into()),
                    }
                }
            }
        }
        Err(error) => Err(error.into()),
    }
}

fn report_install_skipped(kind: HarnessKind, reason: &str) {
    eprintln!("{kind} was not found; {reason}.");
    eprintln!(
        "Install the official release, or pass --executable /path/to/{}.",
        kind.binary_name()
    );
}

fn resolve_config(
    arguments: &HarnessRunArgs,
) -> Result<nan_harness_runtime::ResolvedConfig, CliError> {
    ConfigResolver::resolve(
        &ProcessEnvironment,
        ConfigOverrides {
            provider_base_url: arguments.provider_base_url.clone(),
            nan_api_key: None,
        },
    )
    .map_err(CliError::Config)
}

fn run_doctor(arguments: &DoctorArgs) -> Result<(), CliError> {
    let report = discover_harness(
        arguments.harness,
        arguments.executable.as_deref(),
        DiscoveryOptions {
            allow_unsupported: arguments.allow_unsupported,
            allow_untested: arguments.allow_untested,
        },
    )?;

    println!("Harness: {}", report.harness.kind);
    println!("Executable: {}", report.harness.executable);
    println!("Version output: {}", report.harness.detected_version);
    println!("Minimum supported: {}", report.minimum_supported_version);
    println!("Last verified: {}", report.last_verified_version);
    println!(
        "Compatibility: {}",
        compatibility_label(report.harness.version_status)
    );
    for warning in report.warnings {
        println!("Warning: {warning}");
    }
    Ok(())
}

fn validate_plan(path: &Path) -> Result<(), CliError> {
    let source = fs::read_to_string(path).map_err(|source| CliError::ReadPlan {
        path: path.to_path_buf(),
        source,
    })?;
    let plan: nan_harness_core::LaunchPlan =
        serde_json::from_str(&source).map_err(|source| CliError::ParsePlan {
            path: path.to_path_buf(),
            source,
        })?;
    nan_harness_core::LaunchPlanValidator::validate(&plan).map_err(CliError::InvalidPlan)?;
    let normalized = serde_json::to_string_pretty(&plan).map_err(CliError::SerializePlan)?;
    println!("{normalized}");
    Ok(())
}

fn generate_launch_id() -> Result<LaunchId, CliError> {
    let mut bytes = [0_u8; 12];
    getrandom::fill(&mut bytes).map_err(CliError::Random)?;
    let mut suffix = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut suffix, "{byte:02x}").expect("writing to a String cannot fail");
    }
    LaunchId::new(format!("launch_{suffix}")).map_err(CliError::InvalidPlan)
}

fn requested_model(model: &str) -> ResolvedModel {
    let bundled = matches!(
        model,
        "qwen3.6" | "deepseek-v4-flash" | "mimo-v2.5" | "gemma4"
    );
    ResolvedModel {
        requested_id: model.to_owned(),
        resolved_id: model.to_owned(),
        availability: ModelAvailability::Discovered,
        profile_source: if bundled {
            ProfileSource::Bundled
        } else {
            ProfileSource::Generic
        },
        qualification: if bundled {
            QualificationStatus::Qualified
        } else {
            QualificationStatus::Unknown
        },
        warnings: Vec::new(),
    }
}

fn install_signal_handlers(cancellation: CancellationToken) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            let Ok(mut terminate) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            else {
                if tokio::signal::ctrl_c().await.is_ok() {
                    cancellation.cancel(SignalKind::Interrupt);
                }
                return;
            };
            tokio::select! {
                result = tokio::signal::ctrl_c() => {
                    if result.is_ok() {
                        cancellation.cancel(SignalKind::Interrupt);
                    }
                }
                value = terminate.recv() => {
                    if value.is_some() {
                        cancellation.cancel(SignalKind::Terminate);
                    }
                }
            }
        }
        #[cfg(not(unix))]
        if tokio::signal::ctrl_c().await.is_ok() {
            cancellation.cancel(SignalKind::Interrupt);
        }
    })
}

const fn compatibility_label(status: nan_harness_core::harness::VersionStatus) -> &'static str {
    use nan_harness_core::harness::VersionStatus;

    match status {
        VersionStatus::Tested => "tested",
        VersionStatus::Supported => "supported",
        VersionStatus::NewerUntested => "newer-untested",
        VersionStatus::OlderUnsupported => "older-unsupported",
        VersionStatus::Unparseable => "unparseable",
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
    Config(#[from] ConfigError),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error("could not read the current working directory: {0}")]
    CurrentDirectory(std::io::Error),
    #[error("could not generate a launch ID: {0}")]
    Random(getrandom::Error),
    #[error("could not read launch plan '{}': {source}", path.display())]
    ReadPlan {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("launch plan '{}' is not valid JSON for schema version 1: {source}", path.display())]
    ParsePlan {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
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
}

impl CliError {
    const fn code(&self) -> &'static str {
        match self {
            Self::Discovery(error) => error.code(),
            Self::Install(_) => InstallError::code(),
            Self::Config(error) => error.code(),
            Self::Runtime(error) => error.code(),
            Self::ReadPlan { .. } => "NH-CLI-001",
            Self::ParsePlan { .. } => "NH-CLI-002",
            Self::SerializePlan(_) => "NH-CLI-003",
            Self::CurrentDirectory(_) | Self::Random(_) => "NH-CLI-005",
            Self::InvalidPlan(error) => error.code(),
            Self::TelemetrySettings(_) => "NH-TELEMETRY-001",
            Self::Update(error) => error.code(),
            Self::Persistence(error) => error.code(),
        }
    }

    fn telemetry_context(&self, cli: &Cli, interactive: bool) -> ErrorReportContext {
        let (category, stage, retryable) = self.telemetry_failure();
        let mut context = ErrorReportContext::new(
            Failure::new(self.code(), category, stage, retryable),
            interactive,
        );
        if let Some(harness) = telemetry_harness(cli) {
            context = context.with_harness(TelemetryHarnessIdentity::new(harness, None));
        }
        if let Some(transport) = telemetry_transport(cli) {
            context = context.with_transport(transport);
        }
        context
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
            Self::Config(_) => (
                FailureCategory::Configuration,
                FailureStage::CredentialResolution,
                false,
            ),
            Self::Runtime(error) => runtime_failure(error),
            Self::ReadPlan { .. } | Self::ParsePlan { .. } => (
                FailureCategory::Validation,
                FailureStage::LaunchValidation,
                false,
            ),
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
            Self::CurrentDirectory(_) | Self::Random(_) => {
                (FailureCategory::Internal, FailureStage::Startup, false)
            }
            Self::TelemetrySettings(_) => {
                (FailureCategory::Configuration, FailureStage::Startup, false)
            }
            Self::Update(_) => (FailureCategory::Internal, FailureStage::Startup, true),
            Self::Persistence(_) => (FailureCategory::Configuration, FailureStage::Startup, false),
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
        Command::Doctor(arguments) => Some(match arguments.harness {
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
        Command::Update | Command::ValidatePlan { .. } | Command::Telemetry { .. } => None,
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
        | Command::Update
        | Command::ValidatePlan { .. }
        | Command::Telemetry { .. } => None,
    }
}
