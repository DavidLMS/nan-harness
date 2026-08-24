#![forbid(unsafe_code)]

mod app;
mod commands;
mod error;
mod observability;
mod runner;

use app::{Cli, Command};
use clap::Parser;
use nan_harness_runtime::BridgeDiagnostic;
use nan_harness_telemetry::TelemetryReporter;
use nan_harness_telemetry::glitchtip::GlitchTipExporter;
use nan_harness_telemetry::panic::install_panic_hook;
use observability::{
    panic_telemetry_context, report_bridge_diagnostics, report_compat_error, start_usage_analytics,
    telemetry_reporter,
};
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
        if let Some(reporter) = &telemetry {
            report_compat_error(reporter, &error, &cli, interactive).await;
        }
    }
    let usage_analytics_task = start_usage_analytics(&cli, telemetry.as_ref());
    let exit_code = report_run_result(telemetry.as_ref(), &cli, interactive).await;
    if let Some(task) = usage_analytics_task {
        let _ = task.await;
    }
    exit_code
}

async fn report_run_result(
    telemetry: Option<&TelemetryReporter<GlitchTipExporter>>,
    cli: &Cli,
    interactive: bool,
) -> ExitCode {
    let mut bridge_diagnostics = Vec::new();
    match runner::run(cli, interactive, &mut bridge_diagnostics).await {
        Ok(exit_code) => {
            report_bridge_diagnostics_if_any(telemetry, &bridge_diagnostics, cli, interactive)
                .await;
            exit_code_from_i32(exit_code)
        }
        Err(error) => {
            report_bridge_diagnostics_if_any(telemetry, &bridge_diagnostics, cli, interactive)
                .await;
            let message = error.user_message();
            eprintln!("{}", message.render_terminal());
            if message.is_reportable()
                && let Some(reporter) = telemetry
            {
                let context = error.telemetry_context(cli, interactive);
                let mut input = std::io::stdin().lock();
                let mut output = std::io::stderr().lock();
                let _ = reporter.report(context, &mut input, &mut output).await;
            }
            ExitCode::FAILURE
        }
    }
}

async fn report_bridge_diagnostics_if_any(
    telemetry: Option<&TelemetryReporter<GlitchTipExporter>>,
    diagnostics: &[BridgeDiagnostic],
    cli: &Cli,
    interactive: bool,
) {
    if let Some(reporter) = telemetry
        && !diagnostics.is_empty()
    {
        report_bridge_diagnostics(reporter, diagnostics, cli, interactive).await;
    }
}

fn exit_code_from_i32(value: i32) -> ExitCode {
    ExitCode::from(u8::try_from(value.clamp(0, 255)).unwrap_or(1))
}
