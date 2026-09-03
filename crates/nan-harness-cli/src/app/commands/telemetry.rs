use clap::Subcommand;

#[derive(Debug, Clone, Copy, Subcommand)]
#[command(disable_help_subcommand = true)]
pub(crate) enum TelemetryCommand {
    #[command(about = "Enable anonymous error and usage telemetry")]
    On,
    #[command(about = "Disable anonymous error and usage telemetry")]
    Off,
}
