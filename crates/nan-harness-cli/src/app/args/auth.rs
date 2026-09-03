use clap::Args;

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
