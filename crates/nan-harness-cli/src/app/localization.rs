use clap::{
    ArgAction, Command,
    error::{ContextKind, ContextValue, ErrorKind},
};
use nan_harness_i18n::{Locale, locale, messages};

pub(super) fn command(mut command: Command) -> Command {
    if locale() == Locale::En {
        return command;
    }
    command.build();
    let command = localize_aliases(command);
    command
        .help_template(messages::parser_help_template(
            locale(),
            "{about-with-newline}",
            "{after-help}",
            "{all-args}",
            "{before-help}",
            "{usage}",
        ))
        .subcommand_help_heading(messages::parser_commands(locale()))
        .mut_args(localize_argument)
        .mut_subcommands(|subcommand| {
            let subcommand = if subcommand.get_name() == "help" {
                subcommand.about(messages::parser_help_command(locale()))
            } else {
                subcommand
            };
            self::command(subcommand)
        })
}

fn localize_aliases(command: Command) -> Command {
    let visible = command.get_visible_aliases().collect::<Vec<_>>().join(", ");
    if visible.is_empty() {
        return command;
    }
    let aliases = command
        .get_all_aliases()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let about = command
        .get_about()
        .map(ToString::to_string)
        .unwrap_or_default();
    let about = messages::parser_help_aliases(locale(), &visible, &about);
    command.alias(None::<&str>).aliases(aliases).about(about)
}

fn localize_argument(argument: clap::Arg) -> clap::Arg {
    let heading = if argument.is_positional() {
        messages::parser_arguments(locale())
    } else {
        messages::parser_options(locale())
    };
    let argument = argument.help_heading(heading);
    let argument = match argument.get_action() {
        ArgAction::Help | ArgAction::HelpShort | ArgAction::HelpLong => {
            argument.help(messages::parser_help(locale()))
        }
        ArgAction::Version => argument.help(messages::parser_version(locale())),
        _ => argument,
    };
    let defaults = if argument.get_action().takes_values() && !argument.is_hide_default_value_set()
    {
        argument
            .get_default_values()
            .iter()
            .map(|value| value.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        String::new()
    };
    let values = if argument.is_hide_possible_values_set() {
        String::new()
    } else {
        argument
            .get_possible_values()
            .iter()
            .filter(|value| !value.is_hide_set())
            .map(clap::builder::PossibleValue::get_name)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let extend = |mut help: String| {
        if !defaults.is_empty() {
            help = messages::parser_help_default(locale(), &help, &defaults);
        }
        if !values.is_empty() {
            help = messages::parser_help_values(locale(), &help, &values);
        }
        help
    };
    let short = extend(
        argument
            .get_help()
            .map(ToString::to_string)
            .unwrap_or_default(),
    );
    let long = argument
        .get_long_help()
        .map(|help| extend(help.to_string()));
    let argument = if let Some(long) = long {
        argument.long_help(long)
    } else {
        argument
    };
    argument
        .help(short)
        .hide_default_value(true)
        .hide_possible_values(true)
}

pub(super) fn exit(error: &clap::Error) -> ! {
    if locale() == Locale::En
        || matches!(
            error.kind(),
            ErrorKind::DisplayHelp
                | ErrorKind::DisplayVersion
                | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        )
    {
        error.exit();
    }
    eprintln!("{}", render_error(error));
    std::process::exit(error.exit_code());
}

fn context(error: &clap::Error, kind: ContextKind) -> String {
    error.get(kind).map(ToString::to_string).unwrap_or_default()
}

fn summary(error: &clap::Error) -> String {
    let language = locale();
    let argument = context(error, ContextKind::InvalidArg);
    let value = context(error, ContextKind::InvalidValue);
    let command = context(error, ContextKind::InvalidSubcommand);
    match error.kind() {
        ErrorKind::UnknownArgument if argument.is_empty() => messages::parser_gateway(language),
        ErrorKind::UnknownArgument => messages::parser_unknown_argument(language, &(argument)),
        ErrorKind::InvalidSubcommand => messages::parser_unknown_command(language, &(command)),
        ErrorKind::InvalidValue | ErrorKind::ValueValidation => {
            messages::parser_invalid_value(language, &(argument), &(value))
        }
        ErrorKind::MissingRequiredArgument => messages::parser_required(language, &(argument)),
        ErrorKind::MissingSubcommand => messages::parser_missing_command(language, &(command)),
        ErrorKind::ArgumentConflict => {
            let prior = context(error, ContextKind::PriorArg);
            if argument == prior {
                messages::parser_repeated(language, &(argument))
            } else {
                messages::parser_conflict(language, &(argument), &(prior))
            }
        }
        ErrorKind::NoEquals => messages::parser_equals(language, &(argument)),
        ErrorKind::TooManyValues => messages::parser_excess(language, &(argument), &(value)),
        ErrorKind::TooFewValues => messages::parser_minimum(
            language,
            &(context(error, ContextKind::ActualNumValues)),
            &(argument),
            &(context(error, ContextKind::MinValues)),
        ),
        ErrorKind::WrongNumberOfValues => messages::parser_count(
            language,
            &(context(error, ContextKind::ActualNumValues)),
            &(argument),
            &(context(error, ContextKind::ExpectedNumValues)),
        ),
        ErrorKind::InvalidUtf8 => messages::parser_encoding(language),
        _ => messages::parser_failure(language),
    }
}

fn render_error(error: &clap::Error) -> String {
    let mut output = messages::diagnostic_error(locale(), &summary(error));
    output.push('\n');
    for kind in [ContextKind::ValidValue, ContextKind::ValidSubcommand] {
        if let Some(value) = error.get(kind) {
            output.push_str(&messages::parser_allowed(locale(), &(value)));
            output.push('\n');
        }
    }
    for kind in [
        ContextKind::SuggestedArg,
        ContextKind::SuggestedSubcommand,
        ContextKind::SuggestedValue,
    ] {
        if let Some(value) = error.get(kind) {
            output.push_str(&messages::parser_suggestion(locale(), &(value)));
            output.push('\n');
        }
    }
    if error.kind() == ErrorKind::ValueValidation {
        use nan_harness_i18n::TerminalMessage as _;
        use std::error::Error as _;
        if let Some(cause) = error
            .source()
            .and_then(|source| source.downcast_ref::<nan_harness_i18n::DiagnosticText>())
        {
            output.push_str(&cause.terminal_message(locale()));
            output.push('\n');
        }
        let argument = context(error, ContextKind::InvalidArg);
        let guidance = if argument.starts_with("--startup-timeout") {
            Some(messages::parser_timeout_range(locale()))
        } else if argument.starts_with("--context") || argument.starts_with("--session-max-tokens")
        {
            Some(messages::parser_positive_integer(locale()))
        } else {
            None
        };
        if let Some(guidance) = guidance {
            output.push_str(&guidance);
            output.push('\n');
        }
    }
    output.push_str(&messages::parser_more_help(locale()));
    output
}

pub(super) fn suggests_private_command(error: &clap::Error) -> bool {
    match error.get(ContextKind::SuggestedSubcommand) {
        Some(ContextValue::String(value)) => {
            matches!(value.as_str(), "diagnostics" | "__coordinator")
        }
        Some(ContextValue::Strings(values)) => values
            .iter()
            .any(|value| matches!(value.as_str(), "diagnostics" | "__coordinator")),
        _ => false,
    }
}
