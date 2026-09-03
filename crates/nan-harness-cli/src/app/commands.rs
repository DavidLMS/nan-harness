mod auth;
mod completions;
mod telemetry;

use super::args::{
    BridgedHarnessRunArgs, ChatGptDesktopArgs, ClaudeDesktopArgs, ConfigArgs, DirectHarnessRunArgs,
    DoctorArgs, HermesDesktopArgs, PenDesktopArgs, RecordInstallationArgs, UninstallArgs,
    ZedDesktopArgs,
};
pub(crate) use auth::AuthCommand;
use clap::Subcommand;
pub(crate) use completions::CompletionShell;
pub(crate) use telemetry::TelemetryCommand;

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    #[command(
        name = "chatgpt-desktop",
        visible_alias = "codex-desktop",
        about = "Run ChatGPT Desktop through NaN (experimental)"
    )]
    ChatGptDesktop(ChatGptDesktopArgs),
    #[command(
        name = "claude-desktop",
        about = "Run Claude Desktop through NaN (experimental)"
    )]
    ClaudeDesktop(ClaudeDesktopArgs),
    #[command(
        name = "claude",
        visible_alias = "claude-code",
        about = "Run Claude Code through the local nan-harness bridge"
    )]
    Claude(BridgedHarnessRunArgs),
    #[command(about = "Run Codex through the local nan-harness Responses bridge")]
    Codex(BridgedHarnessRunArgs),
    #[command(name = "opencode", about = "Run OpenCode through NaN Chat Completions")]
    OpenCode(DirectHarnessRunArgs),
    #[command(about = "Run Hermes Agent through NaN Chat Completions")]
    Hermes(DirectHarnessRunArgs),
    #[command(
        name = "hermes-desktop",
        about = "Run Hermes Desktop through a managed NaN profile (experimental)"
    )]
    HermesDesktop(HermesDesktopArgs),
    #[command(
        name = "pen",
        visible_alias = "pen-desktop",
        about = "Run Pen Desktop through a managed NaN model provider (experimental)"
    )]
    PenDesktop(PenDesktopArgs),
    #[command(
        name = "zed",
        visible_alias = "zed-desktop",
        about = "Run Zed through a temporary NaN model provider (experimental)"
    )]
    ZedDesktop(ZedDesktopArgs),
    #[command(about = "Run Pi through a NaN provider extension")]
    Pi(DirectHarnessRunArgs),
    #[command(
        name = "omp",
        visible_alias = "oh-my-pi",
        about = "Run Oh My Pi through a NaN provider extension"
    )]
    Omp(DirectHarnessRunArgs),
    #[command(
        name = "prime-agent",
        visible_alias = "prime",
        about = "Run Prime Agent through a NaN provider extension"
    )]
    Prime(DirectHarnessRunArgs),
    #[command(
        name = "dsh",
        visible_aliases = ["deepseek", "deepseek-harness"],
        about = "Run DeepSeek Harness through a temporary NaN provider patch"
    )]
    DeepSeek(DirectHarnessRunArgs),
    #[command(
        name = "openclaw",
        about = "Run OpenClaw through a temporary linked configuration"
    )]
    OpenClaw(DirectHarnessRunArgs),
    #[command(about = "Run Cline through a temporary linked configuration")]
    Cline(DirectHarnessRunArgs),
    #[command(
        name = "qwen",
        visible_alias = "qwen-code",
        about = "Run Qwen Code through NaN Chat Completions"
    )]
    Qwen(DirectHarnessRunArgs),
    #[command(
        name = "kimi",
        visible_alias = "kimi-code",
        about = "Run Kimi Code through its in-memory NaN model configuration"
    )]
    Kimi(DirectHarnessRunArgs),
    #[command(about = "Run Aider through NaN Chat Completions")]
    Aider(DirectHarnessRunArgs),
    #[command(about = "Run Goose through NaN Chat Completions")]
    Goose(DirectHarnessRunArgs),
    #[command(about = "Run fx through the local nan-harness AI Gateway bridge")]
    Fx(BridgedHarnessRunArgs),
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
    #[command(
        about = "Generate shell completion scripts for nanh",
        after_help = "Load for the current session:\n  bash:       source <(nanh completions bash)\n  zsh:        source <(nanh completions zsh)\n  fish:       nanh completions fish | source\n  PowerShell: nanh completions powershell | Out-String | Invoke-Expression"
    )]
    Completions {
        #[arg(value_enum)]
        shell: CompletionShell,
    },
    #[command(name = "__record-installation", hide = true)]
    RecordInstallation(RecordInstallationArgs),
}
