use crate::app::TelemetryCommand;
use nan_harness_telemetry::consent::{SettingsError, TelemetryPreference, TelemetrySettingsStore};

pub(crate) fn run(command: TelemetryCommand) -> Result<(), SettingsError> {
    let preference = match command {
        TelemetryCommand::On => TelemetryPreference::On,
        TelemetryCommand::Off => TelemetryPreference::Off,
    };
    TelemetrySettingsStore::from_environment()?.set(preference)?;
    println!(
        "Telemetry is {}.",
        if preference.enabled() { "on" } else { "off" }
    );
    Ok(())
}
