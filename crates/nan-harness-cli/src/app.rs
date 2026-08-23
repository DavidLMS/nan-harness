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
        about = "Run Claude Code through the local nan-harness bridge"
    )]
    Claude(HarnessRunArgs),
    #[command(about = "Run Codex through the local nan-harness Responses bridge")]
    Codex(HarnessRunArgs),
    #[command(name = "opencode", about = "Run OpenCode through NaN Chat Completions")]
    OpenCode(HarnessRunArgs),
    #[command(about = "Run Hermes Agent through NaN Chat Completions")]
    Hermes(HarnessRunArgs),
    #[command(about = "Run Pi through a NaN provider extension")]
    Pi(HarnessRunArgs),
    #[command(
        name = "prime-agent",
        visible_alias = "prime",
        about = "Run Prime Agent through a NaN provider extension"
    )]
    Prime(HarnessRunArgs),
    #[command(
        name = "dsh",
        visible_aliases = ["deepseek", "deepseek-harness"],
        about = "Run DeepSeek Harness through a temporary NaN provider patch"
    )]
    DeepSeek(HarnessRunArgs),
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
    Qwen(HarnessRunArgs),
    #[command(
        name = "kimi",
        visible_alias = "kimi-code",
        about = "Run Kimi Code through its in-memory NaN model configuration"
    )]
    Kimi(HarnessRunArgs),
    #[command(about = "Run Aider through NaN Chat Completions")]
    Aider(HarnessRunArgs),
    #[command(about = "Run Goose through NaN Chat Completions")]
    Goose(HarnessRunArgs),
    #[command(about = "Run fx through the local nan-harness AI Gateway bridge")]
    Fx(HarnessRunArgs),
    #[command(about = "Diagnose nan-harness or inspect one harness in detail")]
    Doctor(DoctorArgs),
    #[command(about = "Manage the saved NaN provider API key")]
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    #[command(about = "Configure NaN natively in a supported harness")]
    Config(ConfigArgs),
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
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct ConfigArgs {
    #[arg(
        value_name = "HARNESS",
        help = "Harness whose native user configuration should be managed"
    )]
    pub(crate) harness: Option<HarnessKind>,
    #[arg(
        long,
        help = "Inspect one harness, or all harnesses when HARNESS is omitted",
        conflicts_with_all = ["refresh", "remove", "refresh_all", "remove_all"]
    )]
    pub(crate) status: bool,
    #[arg(
        long,
        help = "Refresh the copied key, model catalog, and managed defaults",
        requires = "harness",
        conflicts_with_all = ["status", "remove", "refresh_all", "remove_all"]
    )]
    pub(crate) refresh: bool,
    #[arg(
        long,
        help = "Remove this managed native configuration safely",
        requires = "harness",
        conflicts_with_all = ["status", "refresh", "refresh_all", "remove_all"]
    )]
    pub(crate) remove: bool,
    #[arg(
        long,
        help = "Refresh every native configuration managed by nan-harness",
        conflicts_with_all = ["harness", "status", "refresh", "remove", "remove_all"]
    )]
    pub(crate) refresh_all: bool,
    #[arg(
        long,
        help = "Remove every native configuration managed by nan-harness",
        conflicts_with_all = ["harness", "status", "refresh", "remove", "refresh_all"]
    )]
    pub(crate) remove_all: bool,
    #[arg(
        short = 'y',
        long,
        help = "Confirm first-time configuration or remove-all without prompting"
    )]
    pub(crate) yes: bool,
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
    #[command(about = "Remove the API key previously saved by nan-harness")]
    Logout(AuthLogoutArgs),
}

#[derive(Debug, Clone, Copy, Args)]
pub(crate) struct AuthLogoutArgs {
    #[arg(
        long,
        help = "Remove managed harness configurations before deleting the saved key",
        conflicts_with = "keep_configs"
    )]
    pub(crate) remove_configs: bool,
    #[arg(
        long,
        help = "Keep managed harness configurations and their copied keys",
        conflicts_with = "remove_configs"
    )]
    pub(crate) keep_configs: bool,
    #[arg(
        short = 'y',
        long,
        help = "Confirm the selected logout behavior without prompting"
    )]
    pub(crate) yes: bool,
}
