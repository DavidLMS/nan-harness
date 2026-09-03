use super::arguments::command_working_directory;
use super::harness::run_simple_harness;
#[allow(clippy::wildcard_imports)]
use super::*;

#[allow(clippy::too_many_lines)]
pub(super) async fn dispatch(
    cli: &Cli,
    interactive: bool,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<i32, RunError> {
    let working_directory = command_working_directory(cli)?;
    match &cli.command {
        Command::ChatGptDesktop(arguments) => {
            return commands::chatgpt_desktop::run(arguments, interactive, bridge_diagnostics)
                .await
                .map_err(Into::into);
        }
        Command::ClaudeDesktop(arguments) => {
            return commands::claude_desktop::run(arguments, interactive, bridge_diagnostics)
                .await
                .map_err(Into::into);
        }
        Command::HermesDesktop(arguments) => {
            let working_directory = working_directory.as_deref().ok_or_else(|| {
                CliError::CurrentDirectory(std::io::Error::other(
                    "Hermes Desktop launch requires a working directory",
                ))
            })?;
            return commands::hermes_desktop::run(
                arguments,
                interactive,
                working_directory,
                bridge_diagnostics,
            )
            .await
            .map_err(Into::into);
        }
        Command::PenDesktop(arguments) => {
            return commands::pen_desktop::run(arguments, interactive, bridge_diagnostics)
                .await
                .map_err(Into::into);
        }
        Command::ZedDesktop(arguments) => {
            return commands::zed_desktop::run(arguments, interactive, bridge_diagnostics)
                .await
                .map_err(Into::into);
        }
        _ => {}
    }
    if let Some(working_directory) = working_directory.as_deref()
        && let Some(result) =
            run_simple_harness(cli, interactive, working_directory, bridge_diagnostics).await
    {
        return result;
    }
    match &cli.command {
        Command::Doctor(arguments) => commands::doctor::run(arguments)
            .await
            .map_err(CliError::from)
            .map_err(Into::into),
        Command::Auth { command } => {
            commands::credentials::run(command, interactive)
                .await
                .map_err(CliError::from)?;
            Ok(0)
        }
        Command::Config(arguments) => {
            commands::configuration::run(arguments, interactive)
                .await
                .map_err(CliError::from)?;
            Ok(0)
        }
        Command::Update => {
            commands::update::run_manual()
                .await
                .map_err(CliError::from)?;
            Ok(0)
        }
        Command::Uninstall(arguments) => {
            commands::uninstall::run(arguments, interactive).map_err(CliError::from)?;
            Ok(0)
        }
        Command::Telemetry { command } => {
            commands::telemetry::run(*command).map_err(CliError::from)?;
            Ok(0)
        }
        Command::RecordInstallation(arguments) => {
            commands::uninstall::record_installation(arguments).map_err(CliError::from)?;
            Ok(0)
        }
        Command::Completions { .. } => {
            unreachable!("completion generation returns before runner dispatch")
        }
        Command::Claude(_)
        | Command::ChatGptDesktop(_)
        | Command::ClaudeDesktop(_)
        | Command::Codex(_)
        | Command::OpenCode(_)
        | Command::Hermes(_)
        | Command::HermesDesktop(_)
        | Command::PenDesktop(_)
        | Command::ZedDesktop(_)
        | Command::Pi(_)
        | Command::Omp(_)
        | Command::Prime(_)
        | Command::DeepSeek(_)
        | Command::OpenClaw(_)
        | Command::Cline(_)
        | Command::Qwen(_)
        | Command::Kimi(_)
        | Command::Aider(_)
        | Command::Goose(_)
        | Command::Fx(_) => unreachable!("simple harness commands are dispatched first"),
    }
}
