use super::{Command, localization};
use clap::{CommandFactory, FromArgMatches, Parser, error::ContextKind};

#[derive(Debug, Parser)]
#[command(
    name = "nan-harness",
    bin_name = "nan-harness",
    version,
    about = nan_harness_i18n::messages::help_run_ai_coding_harnesses_through_the_nan_provider(nan_harness_i18n::locale()),
    arg_required_else_help = true,
    after_help = nan_harness_i18n::messages::help_examples_nanh_claude_launch_claude_code_through_the_nan_bridge_nanh_codex_model_qwen3_6_pi(nan_harness_i18n::locale())
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

impl Cli {
    pub(crate) fn parse_checked() -> Self {
        Self::try_parse_checked_from(std::env::args_os())
            .unwrap_or_else(|error| localization::exit(&error))
    }

    pub(crate) fn try_parse_checked_from<I, T>(arguments: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let arguments = arguments
            .into_iter()
            .map(Into::into)
            .collect::<Vec<std::ffi::OsString>>();
        let parsed = match localization::command(Self::command())
            .try_get_matches_from(arguments.clone())
            .and_then(|matches| {
                validate_image_model_harness(&matches)?;
                Self::from_arg_matches(&matches)
            }) {
            Ok(parsed) => parsed,
            Err(mut error) if localization::suggests_private_command(&error) => {
                error.remove(ContextKind::SuggestedSubcommand);
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        let invalid = match &parsed.command {
            Command::Claude(arguments) | Command::Codex(arguments) | Command::Fx(arguments) => {
                arguments.no_chat_gateway
            }
            _ => false,
        };
        if invalid {
            return Err(Self::command().error(
                clap::error::ErrorKind::UnknownArgument,
                "`--no-chat-gateway` is available only for harnesses that use OpenAI Chat Completions",
            ));
        }
        Ok(parsed)
    }
}

fn validate_image_model_harness(matches: &clap::ArgMatches) -> Result<(), clap::Error> {
    let Some((name, args)) = matches.subcommand() else {
        return Ok(());
    };
    if args
        .try_get_one::<String>("image_model")
        .ok()
        .flatten()
        .is_none()
    {
        return Ok(());
    }
    let supported = matches!(name, "hermes" | "hermes-desktop" | "openclaw")
        || name == "config"
            && matches!(
                args.get_one::<super::ConfigTarget>("harness"),
                Some(super::ConfigTarget::Stable(
                    nan_harness_core::HarnessKind::Hermes | nan_harness_core::HarnessKind::OpenClaw
                ))
            );
    if supported {
        Ok(())
    } else {
        Err(Cli::command().error(
            clap::error::ErrorKind::InvalidValue,
            nan_harness_i18n::messages::error_image_model_unsupported_harness(
                nan_harness_i18n::locale(),
            ),
        ))
    }
}
