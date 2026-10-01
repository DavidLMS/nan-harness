use super::persistence::PreferencesStore;
use nan_harness_i18n::{Locale, locale, messages};
use std::process::ExitCode;

pub(crate) fn run(language: Option<&str>) -> ExitCode {
    let current = locale();
    let available = Locale::ALL
        .iter()
        .map(|locale| format!("{} ({})", locale.code(), locale.native_name()))
        .collect::<Vec<_>>()
        .join(", ");
    let Some(language) = language else {
        println!("{}", messages::language_current(current, &(current.code())));
        println!("{}", messages::language_available(current, &available));
        return ExitCode::SUCCESS;
    };
    let Some(selected) = Locale::parse(language) else {
        eprintln!(
            "{}",
            messages::language_unsupported(current, language, &available)
        );
        return ExitCode::from(2);
    };
    match PreferencesStore::from_environment().and_then(|store| store.set_language(language)) {
        Ok(()) => {
            println!("{}", messages::language_saved(selected, &(language)));
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!(
                "{}",
                nan_harness_i18n::TerminalMessage::terminal_message(&error, current)
            );
            ExitCode::FAILURE
        }
    }
}
