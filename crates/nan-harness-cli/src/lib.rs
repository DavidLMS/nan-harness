#![forbid(unsafe_code)]

mod app;

use app::{ClaudeCodeArgs, Cli, Command, DoctorArgs, RunHarness};
use clap::Parser;
use nan_harness_adapters::ClaudeCodeAdapter;
use nan_harness_core::launch_plan::{LaunchId, ObservabilityFormat};
use nan_harness_core::model::{ModelAvailability, ProfileSource, QualificationStatus};
use nan_harness_core::{PlanContext, PlanError, ResolvedModel, build_validated_plan};
use nan_harness_runtime::{
    CancellationToken, ConfigError, ConfigOverrides, ConfigResolver, DiscoveryError,
    DiscoveryOptions, ProcessEnvironment, RuntimeError, SignalKind, Supervisor, discover_harness,
};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use thiserror::Error;

pub async fn main_entry() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli).await {
        Ok(exit_code) => exit_code_from_i32(exit_code),
        Err(error) => {
            eprintln!("error [{}]: {error}", error.code());
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: &Cli) -> Result<i32, CliError> {
    match &cli.command {
        Command::Run { harness } => match harness {
            RunHarness::ClaudeCode(arguments) => run_claude_code(arguments).await,
        },
        Command::Doctor(arguments) => {
            run_doctor(arguments)?;
            Ok(0)
        }
        Command::ValidatePlan { path } => {
            validate_plan(path)?;
            Ok(0)
        }
    }
}

async fn run_claude_code(arguments: &ClaudeCodeArgs) -> Result<i32, CliError> {
    let discovery = discover_harness(
        nan_harness_core::HarnessKind::ClaudeCode,
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
    let plan = build_validated_plan(&ClaudeCodeAdapter, &context).map_err(CliError::InvalidPlan)?;
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
        }
    }
}
