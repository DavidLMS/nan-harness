use clap::{Args, CommandFactory, Parser, Subcommand};
use nan_harness_core::HarnessKind;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "nan-harness",
    bin_name = "nan-harness",
    version,
    about = "Run AI coding harnesses through the NaN provider",
    arg_required_else_help = true,
    after_help = "Examples:\n  nan claude                          run Claude Code through the NaN bridge\n  nan codex --model qwen3.6           pick a model (see: nan doctor)\n  nan claude -- --resume              pass arguments through to the harness\n  nan doctor                          check provider, models, and harness installs"
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
    Claude(BridgedHarnessRunArgs),
    #[command(about = "Run Codex through the local nan-harness Responses bridge")]
    Codex(BridgedHarnessRunArgs),
    #[command(name = "opencode", about = "Run OpenCode through NaN Chat Completions")]
    OpenCode(DirectHarnessRunArgs),
    #[command(about = "Run Hermes Agent through NaN Chat Completions")]
    Hermes(DirectHarnessRunArgs),
    #[command(about = "Run Pi through a NaN provider extension")]
    Pi(DirectHarnessRunArgs),
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
    #[command(flatten)]
    pub(crate) search: WebSearchArgs,
    #[arg(long, help = "Print the safe launch plan without starting the harness")]
    pub(crate) dry_run: bool,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(crate) arguments: Vec<String>,
}

#[derive(Debug, Default, Args)]
pub(crate) struct WebSearchArgs {
    #[arg(long, conflicts_with = "force_search")]
    pub(crate) no_search: bool,
    #[arg(long, conflicts_with = "no_search")]
    pub(crate) force_search: bool,
}

#[derive(Debug, Args)]
pub(crate) struct DirectHarnessRunArgs {
    #[command(flatten)]
    pub(crate) run: HarnessRunArgs,
    #[arg(
        long,
        help = "Bypass the local Chat Completions gateway for this launch"
    )]
    pub(crate) no_chat_gateway: bool,
}

#[derive(Debug, Args)]
pub(crate) struct BridgedHarnessRunArgs {
    #[command(flatten)]
    pub(crate) run: HarnessRunArgs,
    #[arg(long, hide = true)]
    pub(crate) no_chat_gateway: bool,
}

impl Cli {
    pub(crate) fn parse_checked() -> Self {
        Self::try_parse_checked_from(std::env::args_os()).unwrap_or_else(|error| error.exit())
    }

    pub(crate) fn try_parse_checked_from<I, T>(arguments: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let parsed = Self::try_parse_from(arguments)?;
        let invalid = match &parsed.command {
            Command::Claude(arguments) | Command::Codex(arguments) | Command::Fx(arguments) => {
                arguments.no_chat_gateway
            }
            _ => false,
        };
        if invalid {
            return Err(Self::command().error(
                clap::error::ErrorKind::UnknownArgument,
                "`--no-chat-gateway` is available only for harnesses that use OpenAI Chat Completions",
            ));
        }
        Ok(parsed)
    }
}

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct ConfigArgs {
    #[arg(
        value_name = "HARNESS",
        help = "Harness whose native user configuration should be managed"
    )]
    pub(crate) harness: Option<HarnessKind>,
    #[command(flatten)]
    pub(crate) search: WebSearchArgs,
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

#[cfg(test)]
mod tests {
    use super::{Cli, Command};
    use clap::{CommandFactory as _, Parser as _, error::ErrorKind};

    #[test]
    fn bare_invocation_displays_full_help_with_the_existing_error_code() {
        let error = Cli::try_parse_from(["nan"]).expect_err("a subcommand is still required");

        assert_eq!(
            error.kind(),
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
        assert_eq!(error.exit_code(), 2);
        assert!(error.to_string().contains("Usage: nan-harness <COMMAND>"));
    }

    #[test]
    fn top_level_help_includes_quickstart_examples() {
        let help = Cli::command()
            .get_after_help()
            .expect("top-level help should include examples")
            .to_string();

        assert!(help.contains("Examples:"));
        assert!(help.contains("nan claude"));
        assert!(help.contains("nan doctor"));
    }

    #[test]
    fn mistyped_harness_suggests_the_nearest_command() {
        let error =
            Cli::try_parse_from(["nan", "cluade"]).expect_err("unknown command should fail");

        assert!(error.to_string().contains("claude"));
    }

    #[test]
    fn config_accepts_the_same_search_policy_flags_as_launches() {
        let disabled =
            Cli::try_parse_checked_from(["nan-harness", "config", "cline", "--no-search"])
                .expect("disabled search policy should parse");
        let Command::Config(disabled) = disabled.command else {
            panic!("config command should parse");
        };
        assert!(disabled.search.no_search);
        assert!(!disabled.search.force_search);

        let forced =
            Cli::try_parse_checked_from(["nan-harness", "config", "cline", "--force-search"])
                .expect("forced search policy should parse");
        let Command::Config(forced) = forced.command else {
            panic!("config command should parse");
        };
        assert!(!forced.search.no_search);
        assert!(forced.search.force_search);

        assert!(
            Cli::try_parse_checked_from([
                "nan-harness",
                "config",
                "cline",
                "--no-search",
                "--force-search",
            ])
            .is_err()
        );
    }
}
