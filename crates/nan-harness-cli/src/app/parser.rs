use super::Command;
use clap::{CommandFactory, Parser, error::ContextKind};

#[derive(Debug, Parser)]
#[command(
    name = "nan-harness",
    bin_name = "nan-harness",
    version,
    about = "Run AI coding harnesses through the NaN provider",
    arg_required_else_help = true,
    after_help = "Examples:\n  nanh claude                         launch Claude Code through the NaN bridge\n  nanh codex --model qwen3.6          pick a model (see: nanh doctor)\n  nanh claude -- --resume             pass arguments through to the harness\n  nanh doctor                         check provider, models, and harness installs"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

impl Cli {
    pub(crate) fn parse_checked() -> Self {
        Self::try_parse_checked_from(std::env::args_os()).unwrap_or_else(|error| error.exit())
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
        let parsed = match Self::try_parse_from(arguments.clone()) {
            Ok(parsed) => parsed,
            Err(mut error) if suggests_private_command(&error) => {
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

fn suggests_private_command(error: &clap::Error) -> bool {
    let rendered = error.to_string();
    rendered.contains("similar subcommand exists: 'diagnostics'")
        || rendered.contains("similar subcommand exists: '__coordinator'")
}
