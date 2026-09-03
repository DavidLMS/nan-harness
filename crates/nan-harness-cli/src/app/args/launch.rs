use clap::Args;
use std::path::PathBuf;

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
    #[arg(
        long,
        help = "Do not add NaN web search; preserve any existing search configuration",
        conflicts_with = "force_search"
    )]
    pub(crate) no_search: bool,
    #[arg(
        long,
        help = "Use NaN web search even when another search provider is configured",
        conflicts_with = "no_search"
    )]
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
