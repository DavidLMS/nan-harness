use clap::{Args, Parser, Subcommand};
use nan_harness_core::HarnessKind;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "nan-harness",
    version,
    about = "Run AI coding harnesses through NaN"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    #[command(about = "Run a coding harness through NaN")]
    Run {
        #[command(subcommand)]
        harness: RunHarness,
    },
    #[command(about = "Detect a harness executable and check its compatibility")]
    Doctor(DoctorArgs),
    #[command(about = "Validate and normalize a launch plan without executing it")]
    ValidatePlan { path: PathBuf },
}

#[derive(Debug, Subcommand)]
pub(crate) enum RunHarness {
    #[command(
        name = "claude-code",
        about = "Run Claude Code through the local NaN bridge"
    )]
    ClaudeCode(ClaudeCodeArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ClaudeCodeArgs {
    #[arg(long, default_value = "qwen3.6")]
    pub(crate) model: String,
    #[arg(long, value_name = "PATH")]
    pub(crate) executable: Option<PathBuf>,
    #[arg(long, value_name = "URL")]
    pub(crate) provider_base_url: Option<String>,
    #[arg(long)]
    pub(crate) allow_unsupported: bool,
    #[arg(long)]
    pub(crate) allow_untested: bool,
    #[arg(long, help = "Print the safe launch plan without starting Claude Code")]
    pub(crate) dry_run: bool,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(crate) arguments: Vec<String>,
}

#[derive(Debug, Args)]
pub(crate) struct DoctorArgs {
    pub(crate) harness: HarnessKind,
    #[arg(long, value_name = "PATH")]
    pub(crate) executable: Option<PathBuf>,
    #[arg(long)]
    pub(crate) allow_unsupported: bool,
    #[arg(long)]
    pub(crate) allow_untested: bool,
}
