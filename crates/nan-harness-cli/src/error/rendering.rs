use super::CliError;
use crate::app::Cli;
use nan_harness_diagnostics::MessageLevel;
use nan_harness_diagnostics::{RecoveryAction, UserMessage};
use nan_harness_i18n::{Locale, TerminalMessage, messages};

impl CliError {
    pub(crate) fn user_message(&self, cli: &Cli) -> UserMessage {
        self.user_message_for(cli, Locale::En)
    }

    fn user_message_for(&self, cli: &Cli, locale: Locale) -> UserMessage {
        if matches!(self, Self::CurrentDirectory(_)) {
            return current_directory_message(locale);
        }
        if let Some(message) = unavailable_model_message(self, cli, locale) {
            return message;
        }
        if requires_setup(self) {
            return UserMessage::setup_required(self.terminal_message(locale));
        }
        UserMessage::error(self.code(), self.terminal_message(locale))
    }

    pub(crate) fn render_terminal(&self, cli: &Cli) -> String {
        let locale = nan_harness_i18n::locale();
        let message = self.user_message_for(cli, locale);
        let mut rendered = match (message.level, message.code.as_deref()) {
            (MessageLevel::Warning, _) => messages::diagnostic_warning(locale, &message.summary),
            (MessageLevel::SetupRequired, _) => {
                messages::diagnostic_setup(locale, &message.summary)
            }
            (MessageLevel::Error, Some(code)) => {
                messages::diagnostic_coded_error(locale, code, &message.summary)
            }
            (MessageLevel::Error, None) => messages::diagnostic_error(locale, &message.summary),
        };
        for action in message.actions {
            rendered.push_str("\n\n");
            rendered.push_str(&action.title);
            for command in action.commands {
                rendered.push_str("\n  ");
                rendered.push_str(&command);
            }
            if let Some(detail) = action.detail {
                rendered.push_str("\n\n");
                rendered.push_str(&detail);
            }
        }
        rendered
    }
}

fn current_directory_message(locale: Locale) -> UserMessage {
    UserMessage::reportable_warning(messages::diagnostic_reopen_terminal(locale))
}

fn unavailable_model_message(error: &CliError, cli: &Cli, locale: Locale) -> Option<UserMessage> {
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
        UserMessage::error(error.code(), error.terminal_message(locale)).with_action(
            RecoveryAction::new(messages::diagnostic_choose_model(locale)).with_commands(commands),
        ),
    )
}

fn requires_setup(error: &CliError) -> bool {
    matches!(error, CliError::Install(error) if error.is_runtime_precondition())
        || matches!(
            error,
            CliError::HarnessWindowsUnavailable(_)
                | CliError::Credential(_)
                | CliError::Configuration(_)
        )
}
