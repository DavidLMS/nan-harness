use crate::app::TelemetryCommand;
use nan_harness_telemetry::consent::{SettingsError, TelemetryPreference, TelemetrySettingsStore};

pub(crate) fn run(command: TelemetryCommand) -> Result<(), SettingsError> {
    let preference = match command {
        TelemetryCommand::On => TelemetryPreference::On,
        TelemetryCommand::Off => TelemetryPreference::Off,
    };
    TelemetrySettingsStore::from_environment()?.set(preference)?;
    println!(
        "{}",
        nan_harness_i18n::messages::telemetry_telemetry_is(
            nan_harness_i18n::locale(),
            &(if preference.enabled() {
                nan_harness_i18n::messages::terminal_on_text(nan_harness_i18n::locale())
            } else {
                nan_harness_i18n::messages::terminal_off_text(nan_harness_i18n::locale())
            })
        )
    );
    Ok(())
}
