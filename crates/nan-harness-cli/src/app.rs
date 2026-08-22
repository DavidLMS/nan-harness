use clap::{Args, Parser, Subcommand};
use nan_harness_core::HarnessKind;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "nan-harness",
    bin_name = "nan-harness",
    version,
    about = "Run AI coding harnesses through the NaN provider"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    #[command(
        name = "claude",
        visible_alias = "claude-code",
        about = "Run Claude Code through the local NaN bridge"
    )]
    Claude(HarnessRunArgs),
    #[command(about = "Run Codex through the local NaN Responses bridge")]
    Codex(HarnessRunArgs),
    #[command(name = "opencode", about = "Run OpenCode through NaN Chat Completions")]
    OpenCode(PersistentHarnessRunArgs),
    #[command(about = "Run Hermes Agent through NaN Chat Completions")]
    Hermes(HarnessRunArgs),
    #[command(about = "Run Pi through a NaN provider extension")]
    Pi(PersistentHarnessRunArgs),
    #[command(
        name = "prime-agent",
        visible_alias = "prime",
        about = "Run Prime Agent through a NaN provider extension"
    )]
    Prime(PersistentHarnessRunArgs),
    #[command(
        name = "dsh",
        visible_aliases = ["deepseek", "deepseek-harness"],
        about = "Run DeepSeek Harness through a temporary NaN provider patch"
    )]
    DeepSeek(PersistentHarnessRunArgs),
    #[command(
        name = "openclaw",
        about = "Run OpenClaw through a temporary linked configuration"
    )]
    OpenClaw(HarnessRunArgs),
    #[command(about = "Run Cline through a temporary linked configuration")]
    Cline(HarnessRunArgs),
    #[command(
        name = "qwen",
        visible_alias = "qwen-code",
        about = "Run Qwen Code through NaN Chat Completions"
    )]
    Qwen(PersistentHarnessRunArgs),
    #[command(
        name = "kimi",
        visible_alias = "kimi-code",
        about = "Run Kimi Code through its in-memory NaN model configuration"
    )]
    Kimi(HarnessRunArgs),
    #[command(about = "Run Aider through NaN Chat Completions")]
    Aider(PersistentHarnessRunArgs),
    #[command(about = "Run Goose through NaN Chat Completions")]
    Goose(HarnessRunArgs),
    #[command(about = "Run fx through the local NaN AI Gateway bridge")]
    Fx(HarnessRunArgs),
    #[command(about = "Diagnose nan-harness or inspect one harness in detail")]
    Doctor(DoctorArgs),
    #[command(about = "Manage the saved NaN provider API key")]
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    #[command(about = "Update nan-harness to the latest stable release")]
    Update,
    #[command(about = "Remove nan-harness and its managed harness integrations")]
    Uninstall(UninstallArgs),
    #[command(about = "Control anonymous telemetry")]
    Telemetry {
        #[command(subcommand)]
        command: TelemetryCommand,
    },
    #[command(name = "__record-installation", hide = true)]
    RecordInstallation(RecordInstallationArgs),
}

#[derive(Debug, Args)]
pub(crate) struct UninstallArgs {
    #[arg(short = 'y', long, help = "Uninstall without asking for confirmation")]
    pub(crate) yes: bool,
}

#[derive(Debug, Args)]
pub(crate) struct RecordInstallationArgs {
    #[arg(long, value_name = "PATH")]
    pub(crate) executable: PathBuf,
    #[arg(long, value_name = "PATH")]
    pub(crate) alias: PathBuf,
    #[arg(long)]
    pub(crate) user_path_entry_added: bool,
}

#[derive(Debug, Args)]
pub(crate) struct HarnessRunArgs {
    #[arg(long)]
    pub(crate) model: Option<String>,
    #[arg(long, value_name = "PATH")]
    pub(crate) executable: Option<PathBuf>,
    #[arg(long, value_name = "URL")]
    pub(crate) provider_base_url: Option<String>,
    #[arg(long)]
    pub(crate) allow_unsupported: bool,
    #[arg(long)]
    pub(crate) allow_untested: bool,
    #[arg(long, help = "Print the safe launch plan without starting the harness")]
    pub(crate) dry_run: bool,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(crate) arguments: Vec<String>,
}

#[derive(Debug, Args)]
pub(crate) struct PersistentHarnessRunArgs {
    #[command(flatten)]
    pub(crate) run: HarnessRunArgs,
    #[arg(
        long,
        conflicts_with_all = ["unpersist", "dry_run"],
        help = "Install the NaN provider in this harness and keep it available for direct launches"
    )]
    pub(crate) persist: bool,
    #[arg(
        long,
        conflicts_with_all = [
            "persist",
            "model",
            "executable",
            "provider_base_url",
            "allow_unsupported",
            "allow_untested",
            "dry_run",
            "arguments"
        ],
        help = "Remove the provider configuration previously managed by NaN"
    )]
    pub(crate) unpersist: bool,
}

#[derive(Debug, Args)]
pub(crate) struct DoctorArgs {
    pub(crate) harness: Option<HarnessKind>,
    #[arg(long, help = "Print a stable, safe-to-share JSON report")]
    pub(crate) json: bool,
    #[arg(long, value_name = "PATH", requires = "harness")]
    pub(crate) executable: Option<PathBuf>,
    #[arg(long, requires = "harness")]
    pub(crate) allow_unsupported: bool,
    #[arg(long, requires = "harness")]
    pub(crate) allow_untested: bool,
}

#[derive(Debug, Clone, Copy, Subcommand)]
#[command(disable_help_subcommand = true)]
pub(crate) enum TelemetryCommand {
    #[command(about = "Enable anonymous error and usage telemetry")]
    On,
    #[command(about = "Disable anonymous error and usage telemetry")]
    Off,
}

#[derive(Debug, Clone, Copy, Subcommand)]
#[command(disable_help_subcommand = true)]
pub(crate) enum AuthCommand {
    #[command(about = "Verify and save a NaN API key")]
    Login,
    #[command(about = "Show where the active NaN API key comes from")]
    Status,
    #[command(about = "Remove the API key previously saved by NaN")]
    Logout,
}
