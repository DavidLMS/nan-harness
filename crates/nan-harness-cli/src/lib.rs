#![forbid(unsafe_code)]

mod app;
mod commands;

use app::{Cli, Command, DoctorArgs, HarnessRunArgs, RunHarness};
use clap::Parser;
use nan_harness_adapters::{
    ClaudeCodeAdapter, DeepSeekHarnessAdapter, HermesAdapter, OpenCodeAdapter, PiAdapter,
    PrimeAgentAdapter,
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

pub async fn main_entry() -> ExitCode {
    let cli = Cli::parse();
    let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
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
        Command::Run { harness } => match harness {
            RunHarness::ClaudeCode(arguments) => {
                run_harness(HarnessKind::ClaudeCode, arguments, &ClaudeCodeAdapter).await
            }
            RunHarness::OpenCode(arguments) => {
                run_harness(HarnessKind::OpenCode, arguments, &OpenCodeAdapter).await
            }
            RunHarness::Hermes(arguments) => {
                run_harness(HarnessKind::Hermes, arguments, &HermesAdapter).await
            }
            RunHarness::Pi(arguments) => run_harness(HarnessKind::Pi, arguments, &PiAdapter).await,
            RunHarness::PrimeAgent(arguments) => {
                run_harness(HarnessKind::PrimeAgent, arguments, &PrimeAgentAdapter).await
            }
            RunHarness::DeepSeekHarness(arguments) => {
                run_harness(
                    HarnessKind::DeepSeekHarness,
                    arguments,
                    &DeepSeekHarnessAdapter,
                )
                .await
            }
        },
        Command::Doctor(arguments) => {
            run_doctor(arguments)?;
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
    let discovery = discover_harness(
        kind,
        arguments.executable.as_deref(),
        DiscoveryOptions {
            allow_unsupported: arguments.allow_unsupported,
            allow_untested: arguments.allow_untested,
        },
    )?;
    for warning in &discovery.warnings {
        eprintln!("warning: {warning}");
    }
    let working_directory = std::env::current_dir().map_err(CliError::CurrentDirectory)?;
    let context = PlanContext {
        launch_id: generate_launch_id()?,
        harness: discovery.harness,
        model: requested_model(&arguments.model),
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

    let config = ConfigResolver::resolve(
        &ProcessEnvironment,
        ConfigOverrides {
            provider_base_url: arguments.provider_base_url.clone(),
            nan_api_key: None,
        },
    )?;
    let cancellation = CancellationToken::new();
    let signal_task = install_signal_handlers(cancellation.clone());
    let result = Supervisor::new()
        .execute(&plan, &config, &cancellation)
        .await;
    signal_task.abort();
    Ok(result?.exit_code)
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
}

impl CliError {
    const fn code(&self) -> &'static str {
        match self {
            Self::Discovery(error) => error.code(),
            Self::Config(error) => error.code(),
            Self::Runtime(error) => error.code(),
            Self::ReadPlan { .. } => "NH-CLI-001",
            Self::ParsePlan { .. } => "NH-CLI-002",
            Self::SerializePlan(_) => "NH-CLI-003",
            Self::CurrentDirectory(_) | Self::Random(_) => "NH-CLI-005",
            Self::InvalidPlan(error) => error.code(),
            Self::TelemetrySettings(_) => "NH-TELEMETRY-001",
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
        RuntimeError::UnsupportedBridge | RuntimeError::BindBridge(_) => {
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
        Command::Run {
            harness: RunHarness::ClaudeCode(_),
        } => Some(TelemetryHarnessKind::ClaudeCode),
        Command::Run {
            harness: RunHarness::OpenCode(_),
        } => Some(TelemetryHarnessKind::OpenCode),
        Command::Run {
            harness: RunHarness::Hermes(_),
        } => Some(TelemetryHarnessKind::Hermes),
        Command::Run {
            harness: RunHarness::Pi(_),
        } => Some(TelemetryHarnessKind::Pi),
        Command::Run {
            harness: RunHarness::PrimeAgent(_),
        } => Some(TelemetryHarnessKind::PrimeAgent),
        Command::Run {
            harness: RunHarness::DeepSeekHarness(_),
        } => Some(TelemetryHarnessKind::DeepSeekHarness),
        Command::Doctor(arguments) => Some(match arguments.harness {
            HarnessKind::ClaudeCode => TelemetryHarnessKind::ClaudeCode,
            HarnessKind::Codex => TelemetryHarnessKind::Codex,
            HarnessKind::OpenCode => TelemetryHarnessKind::OpenCode,
            HarnessKind::Hermes => TelemetryHarnessKind::Hermes,
            HarnessKind::Pi => TelemetryHarnessKind::Pi,
            HarnessKind::PrimeAgent => TelemetryHarnessKind::PrimeAgent,
            HarnessKind::DeepSeekHarness => TelemetryHarnessKind::DeepSeekHarness,
        }),
        Command::ValidatePlan { .. } | Command::Telemetry { .. } => None,
    }
}

const fn telemetry_transport(cli: &Cli) -> Option<TelemetryTransport> {
    match cli.command {
        Command::Run {
            harness: RunHarness::ClaudeCode(_),
        } => Some(TelemetryTransport::AnthropicBridge),
        Command::Run {
            harness:
                RunHarness::OpenCode(_)
                | RunHarness::Hermes(_)
                | RunHarness::Pi(_)
                | RunHarness::PrimeAgent(_)
                | RunHarness::DeepSeekHarness(_),
        } => Some(TelemetryTransport::DirectChat),
        Command::Doctor(_) | Command::ValidatePlan { .. } | Command::Telemetry { .. } => None,
    }
}
