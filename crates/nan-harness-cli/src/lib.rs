#![forbid(unsafe_code)]

mod app;
mod commands;
mod error;
mod observability;
mod runner;

use app::{Cli, Command};
use clap::Parser;
use nan_harness_telemetry::panic::install_panic_hook;
use observability::{panic_telemetry_context, start_usage_analytics, telemetry_reporter};
use std::io::IsTerminal as _;
use std::process::ExitCode;

pub async fn main_entry() -> ExitCode {
    let cli = Cli::parse();
    let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    let aggregate_doctor = matches!(
        &cli.command,
        Command::Doctor(arguments) if arguments.harness.is_none()
    );
    let disables_observability = aggregate_doctor
        || matches!(
            cli.command,
            Command::Auth { .. } | Command::Uninstall(_) | Command::RecordInstallation(_)
        );
    if !matches!(
        cli.command,
        Command::Update | Command::Uninstall(_) | Command::RecordInstallation(_)
    ) && !aggregate_doctor
    {
        match commands::update::check_on_start(interactive).await {
            Ok(Some(exit_code)) => return exit_code_from_i32(exit_code),
            Ok(None) => {}
            Err(error) => eprintln!(
                "warning [{}]: update failed; continuing with the installed version: {error}",
                error.code()
            ),
        }
    }
    if !matches!(
        cli.command,
        Command::Update | Command::Uninstall(_) | Command::RecordInstallation(_)
    ) && let Err(error) = nan_harness_runtime::refresh_compatibility_manifest().await
    {
        if aggregate_doctor {
            eprintln!(
                "warning [{}]: compatibility metadata refresh failed; continuing with cached or embedded values",
                error.code()
            );
        } else {
            eprintln!(
                "warning [{}]: compatibility metadata refresh failed; continuing with cached or embedded values: {error}",
                error.code()
            );
        }
    }
    let telemetry = if disables_observability {
        None
    } else {
        telemetry_reporter()
    };
    if let Some(reporter) = &telemetry {
        let telemetry_enabled = reporter
            .settings()
            .load()
            .is_ok_and(|settings| settings.enabled());
        install_panic_hook(
            reporter.pending().clone(),
            telemetry_enabled,
            panic_telemetry_context(&cli, interactive),
        );
        if !matches!(cli.command, Command::Telemetry { .. }) {
            let mut input = std::io::stdin().lock();
            let mut output = std::io::stderr().lock();
            let _ = reporter
                .process_pending(interactive, &mut input, &mut output)
                .await;
        }
    }
    let usage_analytics_task = start_usage_analytics(&cli, telemetry.as_ref());
    let exit_code = match runner::run(&cli, interactive).await {
        Ok(exit_code) => exit_code_from_i32(exit_code),
        Err(error) => {
            let message = error.user_message();
            eprintln!("{}", message.render_terminal());
            if message.is_reportable()
                && let Some(reporter) = &telemetry
            {
                let context = error.telemetry_context(&cli, interactive);
                let mut input = std::io::stdin().lock();
                let mut output = std::io::stderr().lock();
                let _ = reporter.report(context, &mut input, &mut output).await;
            }
            ExitCode::FAILURE
        }
    };
    if let Some(task) = usage_analytics_task {
        let _ = task.await;
    }
    exit_code
}

fn exit_code_from_i32(value: i32) -> ExitCode {
    ExitCode::from(u8::try_from(value.clamp(0, 255)).unwrap_or(1))
}
