use super::CliError;
use crate::app::Cli;
use nan_harness_diagnostics::{RecoveryAction, UserMessage};
use nan_harness_telemetry::event::REOPEN_TERMINAL_GUIDANCE_TEXT;

impl CliError {
    pub(crate) fn user_message(&self, cli: &Cli) -> UserMessage {
        if matches!(self, Self::CurrentDirectory(_)) {
            return current_directory_message();
        }
        if let Some(message) = unavailable_model_message(self, cli) {
            return message;
        }
        if requires_setup(self) {
            return UserMessage::setup_required(self.to_string());
        }
        UserMessage::error(self.code(), self.to_string())
    }
}

fn current_directory_message() -> UserMessage {
    UserMessage::reportable_warning(REOPEN_TERMINAL_GUIDANCE_TEXT)
}

fn unavailable_model_message(error: &CliError, cli: &Cli) -> Option<UserMessage> {
    let CliError::Runtime(runtime_error) = error else {
        return None;
    };
    let (requested, available) = runtime_error.unavailable_model()?;
    let mut commands = vec!["nanh doctor".to_owned()];
    if let Some((kind, _)) = crate::runner::harness_run_arguments(cli)
        && let Some(model) = crate::runner::near_model_match(requested, available)
            .or_else(|| available.first().cloned())
    {
        commands.push(format!("nanh {} --model {model}", kind.binary_name()));
    }
    Some(
        UserMessage::error(error.code(), error.to_string()).with_action(
            RecoveryAction::new("Choose a model from your live catalog:").with_commands(commands),
        ),
    )
}

fn requires_setup(error: &CliError) -> bool {
    matches!(error, CliError::Install(error) if error.is_runtime_precondition())
        || matches!(error, CliError::Credential(_) | CliError::Configuration(_))
}
