use clap::Args;
use std::path::PathBuf;

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
