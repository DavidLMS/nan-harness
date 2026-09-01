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
