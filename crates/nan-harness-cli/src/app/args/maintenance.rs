use clap::Args;
use std::path::PathBuf;

#[derive(Debug, clap::Subcommand)]
pub(crate) enum SearchCommand {
    #[command(about = "Configure and, when selected, install a SearXNG backend")]
    Setup(SearchSetupArgs),
    #[command(about = "Show the configured SearXNG backend without starting it")]
    Status(SearchStatusArgs),
    #[command(about = "Disable NaN web search without removing its backend")]
    Disable,
    #[command(about = "Update the configured managed SearXNG backend")]
    Update,
    #[command(about = "Remove the configured backend and its saved endpoint")]
    Remove,
}

#[derive(Debug, Args)]
#[command(group(
    clap::ArgGroup::new("backend")
        .required(true)
        .multiple(false)
        .args(["local", "docker", "url"])
))]
pub(crate) struct SearchSetupArgs {
    #[arg(long, help = "Install and supervise a private local SearXNG instance")]
    pub(crate) local: bool,
    #[arg(long, help = "Create and manage a SearXNG Docker container")]
    pub(crate) docker: bool,
    #[arg(
        long,
        value_name = "URL",
        help = "Use an HTTPS SearXNG endpoint managed elsewhere"
    )]
    pub(crate) url: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct SearchStatusArgs {
    #[arg(long, help = "Render machine-readable JSON status")]
    pub(crate) json: bool,
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
