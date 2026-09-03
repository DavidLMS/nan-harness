use super::launch::{HarnessRunArgs, WebSearchArgs};
use clap::Args;
use std::path::PathBuf;

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct ChatGptDesktopArgs {
    #[arg(long)]
    pub(crate) model: Option<String>,
    #[arg(long, value_name = "MODEL")]
    pub(crate) aux_model: Option<String>,
    #[arg(long, value_name = "URL")]
    pub(crate) provider_base_url: Option<String>,
    #[arg(long, value_name = "PATH")]
    pub(crate) executable: Option<PathBuf>,
    #[arg(long)]
    pub(crate) allow_unsupported: bool,
    #[arg(long)]
    pub(crate) allow_untested: bool,
    #[command(flatten)]
    pub(crate) search: WebSearchArgs,
    #[arg(long, help = "Show verbose, potentially private ChatGPT Desktop logs")]
    pub(crate) debug: bool,
    #[arg(long, help = "Print the inert launch plan without changing state")]
    pub(crate) dry_run: bool,
    #[arg(
        long,
        help = "Restore receipt-backed state from an interrupted launch",
        conflicts_with_all = ["model", "aux_model", "provider_base_url", "executable", "allow_unsupported", "allow_untested", "no_search", "force_search", "debug", "dry_run"]
    )]
    pub(crate) restore: bool,
}

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct ClaudeDesktopArgs {
    #[arg(long)]
    pub(crate) model: Option<String>,
    #[arg(long, value_name = "URL")]
    pub(crate) provider_base_url: Option<String>,
    #[arg(long, value_name = "PATH")]
    pub(crate) executable: Option<PathBuf>,
    #[arg(long)]
    pub(crate) allow_unsupported: bool,
    #[arg(long)]
    pub(crate) allow_untested: bool,
    #[command(flatten)]
    pub(crate) search: WebSearchArgs,
    #[arg(long, help = "Print the inert launch plan without changing state")]
    pub(crate) dry_run: bool,
    #[arg(
        long,
        help = "Show Auto requests and responses that may contain private data"
    )]
    pub(crate) show_auto: bool,
    #[arg(
        long,
        help = "Restore receipt-backed state from an interrupted launch",
        conflicts_with_all = ["model", "provider_base_url", "executable", "allow_unsupported", "allow_untested", "no_search", "force_search", "dry_run", "show_auto"]
    )]
    pub(crate) restore: bool,
}

#[derive(Debug, Args)]
pub(crate) struct HermesDesktopArgs {
    #[command(flatten)]
    pub(crate) run: HarnessRunArgs,
    #[arg(long, help = "Bypass the local gateway in a diagnostic profile")]
    pub(crate) no_chat_gateway: bool,
    #[arg(
        long,
        help = "Restore receipt-backed state from an interrupted launch",
        conflicts_with_all = ["model", "executable", "provider_base_url", "allow_unsupported", "allow_untested", "no_search", "force_search", "dry_run", "no_chat_gateway", "arguments"]
    )]
    pub(crate) restore: bool,
}

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct PenDesktopArgs {
    #[arg(long)]
    pub(crate) model: Option<String>,
    #[arg(long, value_name = "URL")]
    pub(crate) provider_base_url: Option<String>,
    #[arg(long, value_name = "PATH")]
    pub(crate) executable: Option<PathBuf>,
    #[arg(long)]
    pub(crate) allow_unsupported: bool,
    #[arg(long)]
    pub(crate) allow_untested: bool,
    #[arg(long, help = "Print the inert launch plan without changing state")]
    pub(crate) dry_run: bool,
    #[arg(
        long,
        help = "Restore receipt-backed state from an interrupted launch",
        conflicts_with_all = ["model", "provider_base_url", "executable", "allow_unsupported", "allow_untested", "dry_run"]
    )]
    pub(crate) restore: bool,
}

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct ZedDesktopArgs {
    #[arg(long)]
    pub(crate) model: Option<String>,
    #[arg(long, value_name = "PATH")]
    pub(crate) executable: Option<PathBuf>,
    #[arg(long)]
    pub(crate) allow_unsupported: bool,
    #[arg(long)]
    pub(crate) allow_untested: bool,
    #[arg(long, help = "Print the inert launch plan without changing state")]
    pub(crate) dry_run: bool,
    #[arg(
        long,
        help = "Restore receipt-backed state from an interrupted launch",
        conflicts_with_all = ["model", "executable", "allow_unsupported", "allow_untested", "dry_run", "workspace", "arguments"]
    )]
    pub(crate) restore: bool,
    #[arg(value_name = "WORKSPACE", conflicts_with = "restore")]
    pub(crate) workspace: Option<PathBuf>,
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        conflicts_with = "restore"
    )]
    pub(crate) arguments: Vec<String>,
}
