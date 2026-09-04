use clap::Subcommand;

#[derive(Debug, Clone, Copy, Subcommand)]
pub(crate) enum LocalDiagnosticsCommand {
    On,
    Off,
    Status,
    Purge {
        #[arg(long)]
        yes: bool,
    },
}
