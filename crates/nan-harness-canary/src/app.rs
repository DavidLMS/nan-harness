use crate::report::{CanaryTier, CanaryTrigger, FailureClass};
use clap::{Args, Parser, Subcommand};
use nan_harness_core::HarnessKind;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "nan-canary",
    version,
    about = "Run private NaN Harness compatibility canaries"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    #[command(about = "Validate dependencies and store the existing NaN API key in Keychain")]
    Setup(SetupArgs),
    #[command(about = "Render the user-facing diagnostics catalog")]
    Ux(UxArgs),
    #[command(about = "Aggregate cell reports and track repeated failures")]
    Aggregate(AggregateArgs),
    #[command(about = "Run one isolated Tart canary cell")]
    Cell(CellArgs),
    #[command(about = "Re-run the cell that produced an existing report")]
    Reproduce(ReproduceArgs),
    #[command(about = "Validate and print one canary report")]
    ValidateReport(ValidateReportArgs),
    #[command(about = "Create a typed report from an external canary check")]
    Record(Box<RecordArgs>),
}

#[derive(Debug, Args)]
pub(crate) struct SetupArgs {
    #[arg(long, help = "Validate the API key without writing it to Keychain")]
    pub(crate) check_only: bool,
    #[arg(
        long,
        help = "Skip the local Tart dependency check while bootstrapping the repository"
    )]
    pub(crate) skip_tart: bool,
    #[arg(
        long,
        default_value = "https://api.nan.builders/v1",
        value_name = "URL"
    )]
    pub(crate) provider_base_url: String,
    #[arg(
        long,
        value_name = "URL",
        help = "Validate and store an optional private ntfy token"
    )]
    pub(crate) ntfy_url: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct UxArgs {
    #[arg(long, value_name = "ID")]
    pub(crate) scenario: Option<String>,
    #[arg(long, value_name = "PATH")]
    pub(crate) html: Option<PathBuf>,
    #[arg(long, help = "Print only the scenario identifiers")]
    pub(crate) list: bool,
}

#[derive(Debug, Args)]
pub(crate) struct AggregateArgs {
    #[arg(long, value_name = "DIRECTORY")]
    pub(crate) reports: PathBuf,
    #[arg(long, value_name = "PATH")]
    pub(crate) state: PathBuf,
    #[arg(long, value_name = "PATH")]
    pub(crate) summary: PathBuf,
}

#[derive(Debug, Args)]
pub(crate) struct ValidateReportArgs {
    #[arg(value_name = "PATH")]
    pub(crate) report: PathBuf,
}

#[derive(Debug, Args)]
pub(crate) struct CellArgs {
    #[arg(long, value_name = "PATH")]
    pub(crate) spec: PathBuf,
    #[arg(long, value_name = "PATH")]
    pub(crate) output: PathBuf,
    #[arg(long, value_name = "DIRECTORY")]
    pub(crate) private_log_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct ReproduceArgs {
    #[arg(long, value_name = "PATH")]
    pub(crate) spec: PathBuf,
    #[arg(long, value_name = "PATH")]
    pub(crate) report: PathBuf,
    #[arg(long, value_name = "PATH")]
    pub(crate) output: PathBuf,
    #[arg(long, value_name = "DIRECTORY")]
    pub(crate) private_log_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct RecordArgs {
    #[arg(long, value_name = "PATH")]
    pub(crate) output: PathBuf,
    #[arg(long)]
    pub(crate) run_id: String,
    #[arg(long)]
    pub(crate) cell_id: String,
    #[arg(long)]
    pub(crate) spec_sha256: String,
    #[arg(long, value_enum)]
    pub(crate) trigger: CanaryTrigger,
    #[arg(long, value_enum)]
    pub(crate) tier: CanaryTier,
    #[arg(long)]
    pub(crate) scenario: String,
    #[arg(long)]
    pub(crate) nan_version: String,
    #[arg(long)]
    pub(crate) nan_source: String,
    #[arg(long)]
    pub(crate) nan_sha256: String,
    #[arg(long)]
    pub(crate) operating_system: String,
    #[arg(long)]
    pub(crate) architecture: String,
    #[arg(long)]
    pub(crate) image: String,
    #[arg(long)]
    pub(crate) profile: String,
    #[arg(long)]
    pub(crate) harness: HarnessKind,
    #[arg(long)]
    pub(crate) harness_version: String,
    #[arg(long)]
    pub(crate) model: Option<String>,
    #[arg(long)]
    pub(crate) check: String,
    #[arg(long, default_value_t = 0)]
    pub(crate) duration_milliseconds: u64,
    #[arg(long)]
    pub(crate) passed: bool,
    #[arg(long, value_enum, requires = "failure_summary")]
    pub(crate) failure_class: Option<FailureClass>,
    #[arg(long, requires = "failure_class")]
    pub(crate) failure_phase: Option<String>,
    #[arg(long, requires = "failure_class")]
    pub(crate) failure_summary: Option<String>,
}
