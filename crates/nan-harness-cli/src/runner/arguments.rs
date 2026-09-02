#[allow(clippy::wildcard_imports)]
use super::*;

pub(super) fn command_working_directory(cli: &Cli) -> Result<Option<PathBuf>, CliError> {
    if harness_run_arguments(cli).is_some() || matches!(cli.command, Command::Doctor(_)) {
        return std::env::current_dir()
            .map(Some)
            .map_err(CliError::CurrentDirectory);
    }
    Ok(None)
}

pub(crate) fn harness_run_arguments(cli: &Cli) -> Option<(HarnessKind, &HarnessRunArgs)> {
    match &cli.command {
        Command::Claude(arguments) => Some((HarnessKind::ClaudeCode, &arguments.run)),
        Command::Codex(arguments) => Some((HarnessKind::Codex, &arguments.run)),
        Command::OpenCode(arguments) => Some((HarnessKind::OpenCode, &arguments.run)),
        Command::Hermes(arguments) => Some((HarnessKind::Hermes, &arguments.run)),
        Command::HermesDesktop(arguments) => Some((HarnessKind::Hermes, &arguments.run)),
        Command::Pi(arguments) => Some((HarnessKind::Pi, &arguments.run)),
        Command::Omp(arguments) => Some((HarnessKind::Omp, &arguments.run)),
        Command::Prime(arguments) => Some((HarnessKind::PrimeAgent, &arguments.run)),
        Command::DeepSeek(arguments) => Some((HarnessKind::DeepSeekHarness, &arguments.run)),
        Command::OpenClaw(arguments) => Some((HarnessKind::OpenClaw, &arguments.run)),
        Command::Cline(arguments) => Some((HarnessKind::Cline, &arguments.run)),
        Command::Qwen(arguments) => Some((HarnessKind::QwenCode, &arguments.run)),
        Command::Kimi(arguments) => Some((HarnessKind::KimiCode, &arguments.run)),
        Command::Aider(arguments) => Some((HarnessKind::Aider, &arguments.run)),
        Command::Goose(arguments) => Some((HarnessKind::Goose, &arguments.run)),
        Command::Fx(arguments) => Some((HarnessKind::Fx, &arguments.run)),
        Command::ChatGptDesktop(_)
        | Command::ClaudeDesktop(_)
        | Command::PenDesktop(_)
        | Command::Doctor(_)
        | Command::Auth { .. }
        | Command::Config(_)
        | Command::Update
        | Command::Uninstall(_)
        | Command::Telemetry { .. }
        | Command::Completions { .. }
        | Command::RecordInstallation(_) => None,
    }
}

pub(crate) fn interactive_mode(cli: &Cli, terminal_interactive: bool) -> bool {
    terminal_interactive
        && !harness_run_arguments(cli)
            .is_some_and(|(kind, arguments)| non_interactive_mode(kind, arguments))
}

fn non_interactive_mode(kind: HarnessKind, arguments: &HarnessRunArgs) -> bool {
    match kind {
        HarnessKind::ClaudeCode | HarnessKind::Pi | HarnessKind::Omp | HarnessKind::PrimeAgent => {
            has_any_flag(&arguments.arguments, &["-p", "--print"])
        }
        HarnessKind::Hermes => {
            has_subcommand(&arguments.arguments, &["chat"])
                && has_any_flag(&arguments.arguments, &["-q", "--query"])
        }
        HarnessKind::DeepSeekHarness => {
            has_option_value(&arguments.arguments, "--profile", "headless")
        }
        HarnessKind::OpenClaw => {
            has_subcommand(&arguments.arguments, &["agent"])
                && has_any_flag(&arguments.arguments, &["-m", "--message"])
        }
        HarnessKind::Cline => has_any_flag(&arguments.arguments, &["--json"]),
        HarnessKind::QwenCode | HarnessKind::KimiCode => {
            has_any_flag(&arguments.arguments, &["-p", "--prompt"])
        }
        HarnessKind::Aider => has_any_flag(&arguments.arguments, &["-m", "--message"]),
        HarnessKind::Codex => has_subcommand(&arguments.arguments, &["exec", "review"]),
        HarnessKind::OpenCode | HarnessKind::Goose => {
            has_subcommand(&arguments.arguments, &["run"])
        }
        HarnessKind::Fx => has_subcommand(&arguments.arguments, &["ask"]),
    }
}

fn has_any_flag(arguments: &[String], flags: &[&str]) -> bool {
    arguments.iter().any(|argument| {
        flags.iter().any(|flag| {
            argument == flag
                || (flag.starts_with("--")
                    && argument
                        .strip_prefix(flag)
                        .is_some_and(|suffix| suffix.starts_with('=')))
        })
    })
}

fn has_subcommand(arguments: &[String], subcommands: &[&str]) -> bool {
    arguments
        .first()
        .is_some_and(|argument| subcommands.iter().any(|subcommand| argument == subcommand))
}

fn has_option_value(arguments: &[String], option: &str, value: &str) -> bool {
    let inline = format!("{option}={value}");
    arguments
        .windows(2)
        .any(|pair| pair[0] == option && pair[1] == value)
        || arguments.iter().any(|argument| argument == &inline)
}

pub(crate) const fn direct_chat_gateway_disabled(cli: &Cli) -> bool {
    match &cli.command {
        Command::OpenCode(arguments)
        | Command::Hermes(arguments)
        | Command::Pi(arguments)
        | Command::Omp(arguments)
        | Command::Prime(arguments)
        | Command::DeepSeek(arguments)
        | Command::OpenClaw(arguments)
        | Command::Cline(arguments)
        | Command::Qwen(arguments)
        | Command::Kimi(arguments)
        | Command::Aider(arguments)
        | Command::Goose(arguments) => arguments.no_chat_gateway,
        Command::HermesDesktop(arguments) => arguments.no_chat_gateway,
        Command::Claude(_)
        | Command::ChatGptDesktop(_)
        | Command::ClaudeDesktop(_)
        | Command::PenDesktop(_)
        | Command::Codex(_)
        | Command::Fx(_)
        | Command::Doctor(_)
        | Command::Auth { .. }
        | Command::Config(_)
        | Command::Update
        | Command::Uninstall(_)
        | Command::Telemetry { .. }
        | Command::Completions { .. }
        | Command::RecordInstallation(_) => false,
    }
}

pub(super) const fn direct_chat_gateway_notice(
    disabled: bool,
    dry_run: bool,
) -> Option<&'static str> {
    if !disabled {
        None
    } else if dry_run {
        Some(
            "note: Chat Completions gateway would be disabled for this launch. The harness would receive the provider credential directly; usage accounting and gateway-dependent features would be unavailable.",
        )
    } else {
        Some(
            "warning: Chat Completions gateway disabled for this launch. The harness will receive the provider credential directly; usage accounting and gateway-dependent features are unavailable.",
        )
    }
}

pub(super) fn credential_arguments(cli: &Cli) -> Option<&HarnessRunArgs> {
    harness_run_arguments(cli)
        .map(|(_, arguments)| arguments)
        .filter(|arguments| !arguments.dry_run)
}

#[cfg(test)]
mod tests {
    use super::interactive_mode;
    use crate::app::Cli;
    use clap::Parser as _;

    #[test]
    fn detects_explicit_non_interactive_modes() {
        for (harness, arguments) in [
            ("claude", ["-p", "Hello"].as_slice()),
            ("pi", ["--print", "Hello"].as_slice()),
            ("omp", ["-p", "Hello"].as_slice()),
            ("prime-agent", ["--print", "Hello"].as_slice()),
            ("hermes", ["chat", "--query", "Hello"].as_slice()),
            ("dsh", ["--profile", "headless", "Hello"].as_slice()),
            ("openclaw", ["agent", "--message", "Hello"].as_slice()),
            ("cline", ["--json", "Hello"].as_slice()),
            ("qwen", ["--prompt=Hello"].as_slice()),
            ("kimi", ["--prompt", "Hello"].as_slice()),
            ("aider", ["--message", "Hello"].as_slice()),
            ("goose", ["run", "--text", "Hello"].as_slice()),
            ("codex", ["exec", "Hello"].as_slice()),
            ("codex", ["review", "HEAD~1"].as_slice()),
            ("opencode", ["run", "Hello"].as_slice()),
            ("fx", ["ask", "Hello"].as_slice()),
        ] {
            let mut argv = vec!["nan", harness, "--"];
            argv.extend(arguments);
            let cli = Cli::try_parse_from(argv).expect("harness arguments should parse");
            assert!(
                !interactive_mode(&cli, true),
                "expected {harness} {arguments:?} to be non-interactive"
            );
        }
    }

    #[test]
    fn keeps_interactive_modes_interactive() {
        for argv in [
            vec!["nan", "claude"],
            vec!["nan", "pi"],
            vec!["nan", "pi", "--", "Hello"],
            vec!["nan", "omp"],
            vec!["nan", "prime-agent"],
            vec!["nan", "codex", "--", "Hello"],
            vec!["nan", "opencode", "--", "Hello"],
            vec!["nan", "hermes", "--", "chat"],
            vec!["nan", "dsh", "--", "--profile", "default"],
            vec!["nan", "openclaw", "--", "agent"],
            vec!["nan", "cline", "--", "--timeout", "60"],
            vec!["nan", "qwen", "--", "--safe-mode"],
            vec!["nan", "fx", "--", "Hello"],
            vec!["nan", "kimi", "--", "--model", "Hello"],
            vec!["nan", "aider", "--", "--no-auto-commits"],
            vec!["nan", "goose", "--", "session"],
        ] {
            let cli = Cli::try_parse_from(argv).expect("harness arguments should parse");
            assert!(interactive_mode(&cli, true));
        }
    }

    #[test]
    fn terminal_state_still_controls_interactivity() {
        let cli = Cli::try_parse_from(["nan", "pi", "--", "-p", "Hello"])
            .expect("harness arguments should parse");

        assert!(!interactive_mode(&cli, false));
    }
}
