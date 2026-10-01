use clap::Subcommand;

#[derive(Debug, Clone, Copy, Subcommand)]
#[command(disable_help_subcommand = true)]
pub(crate) enum TelemetryCommand {
    #[command(about = nan_harness_i18n::messages::help_enable_anonymous_error_and_usage_telemetry(nan_harness_i18n::locale()))]
    On,
    #[command(about = nan_harness_i18n::messages::help_disable_anonymous_error_and_usage_telemetry(nan_harness_i18n::locale()))]
    Off,
}
