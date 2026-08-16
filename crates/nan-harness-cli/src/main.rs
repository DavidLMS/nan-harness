use clap::{Args, Parser, Subcommand};
use nan_harness_core::{HarnessKind, LaunchPlan, LaunchPlanValidator, PlanError};
use nan_harness_runtime::{DiscoveryError, DiscoveryOptions, discover_harness};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use thiserror::Error;

#[derive(Debug, Parser)]
#[command(
    name = "nan-harness",
    version,
    about = "Run AI coding harnesses through NaN"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(about = "Detect a harness executable and check its compatibility")]
    Doctor(DoctorArgs),
    #[command(about = "Validate and normalize a launch plan without executing it")]
    ValidatePlan { path: PathBuf },
}

#[derive(Debug, Args)]
struct DoctorArgs {
    harness: HarnessKind,
    #[arg(long, value_name = "PATH")]
    executable: Option<PathBuf>,
    #[arg(long)]
    allow_unsupported: bool,
    #[arg(long)]
    allow_untested: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error [{}]: {error}", error.code());
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<(), CliError> {
    match &cli.command {
        Command::Doctor(arguments) => run_doctor(arguments),
        Command::ValidatePlan { path } => validate_plan(path),
    }
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
    let plan: LaunchPlan = serde_json::from_str(&source).map_err(|source| CliError::ParsePlan {
        path: path.to_path_buf(),
        source,
    })?;
    LaunchPlanValidator::validate(&plan).map_err(CliError::InvalidPlan)?;
    let normalized = serde_json::to_string_pretty(&plan).map_err(CliError::SerializePlan)?;
    println!("{normalized}");
    Ok(())
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

#[derive(Debug, Error)]
enum CliError {
    #[error(transparent)]
    Discovery(#[from] DiscoveryError),
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
            Self::ReadPlan { .. } => "NH-CLI-001",
            Self::ParsePlan { .. } => "NH-CLI-002",
            Self::InvalidPlan(error) => error.code(),
            Self::SerializePlan(_) => "NH-CLI-003",
        }
    }
}
