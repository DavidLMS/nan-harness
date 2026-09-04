use super::scenarios::UxScenario;
use nan_harness_diagnostics::{MessageLevel, UserMessage};

pub(super) fn terminal_output(scenario: &UxScenario) -> String {
    scenario
        .terminal_output
        .clone()
        .unwrap_or_else(|| scenario.message.render_terminal())
}

pub(super) fn presentation(message: &UserMessage) -> (&'static str, &'static str, &'static str) {
    match message.level {
        MessageLevel::Warning => ("warning", "Warning", "No error report"),
        MessageLevel::SetupRequired => ("setup", "Setup required", "No error report"),
        MessageLevel::Error => ("error", "nan-harness error", "Report offered with consent"),
    }
}
