#![forbid(unsafe_code)]
#![cfg_attr(not(test), warn(clippy::expect_used, clippy::unwrap_used))]

mod app;
mod commands;
mod error;
mod observability;
mod runner;
mod usage_evidence;
mod usage_summary;

use app::{Cli, Command};
use nan_harness_telemetry::TelemetryReporter;
use nan_harness_telemetry::glitchtip::{ErrorReportExporter, GlitchTipExporter};
use nan_harness_telemetry::panic::install_panic_hook;
use observability::{
    bridge_diagnostic_contexts, panic_telemetry_context, report_compat_error,
    start_usage_analytics, telemetry_reporter,
};
use std::io::IsTerminal as _;
use std::process::ExitCode;

pub async fn main_entry() -> ExitCode {
    if let Some(exit_code) = commands::search_mcp::run_if_requested().await {
        return exit_code;
    }
    regular_main_entry().await
}

async fn regular_main_entry() -> ExitCode {
    let cli = Cli::parse_checked();
    if let Command::Completions { shell } = &cli.command {
        commands::completions::run(*shell);
        return ExitCode::SUCCESS;
    }
    run_cli(cli).await
}

#[allow(clippy::too_many_lines)]
async fn run_cli(cli: Cli) -> ExitCode {
    let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    let inert_dry_run = observability::is_harness_dry_run(&cli);
    let aggregate_doctor = matches!(
        &cli.command,
        Command::Doctor(arguments) if arguments.harness.is_none()
    );
    let disables_observability = aggregate_doctor
        || inert_dry_run
        || matches!(
            cli.command,
            Command::Auth { .. } | Command::Uninstall(_) | Command::RecordInstallation(_)
        );
    let update_check = async {
        if !inert_dry_run
            && !matches!(
                cli.command,
                Command::Update | Command::Uninstall(_) | Command::RecordInstallation(_)
            )
            && !aggregate_doctor
        {
            Some(commands::update::check_on_start(interactive).await)
        } else {
            None
        }
    };
    let compatibility_refresh = async {
        if inert_dry_run
            || matches!(
                cli.command,
                Command::Update | Command::Uninstall(_) | Command::RecordInstallation(_)
            )
        {
            None
        } else {
            Some(nan_harness_runtime::refresh_compatibility_manifest().await)
        }
    };
    let (update_result, compatibility_result) = tokio::join!(update_check, compatibility_refresh);
    let startup_update_error = match update_result {
        Some(Ok(Some(exit_code))) => return exit_code_from_i32(exit_code),
        Some(Ok(None)) | None => None,
        Some(Err(error)) => {
            eprintln!(
                "warning [{}]: update failed; continuing with the installed version: {error}",
                error.code()
            );
            Some(error)
        }
    };
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
        if let Ok(installation_id) = reporter.settings().diagnostic_installation_id() {
            install_panic_hook(
                reporter.pending().clone(),
                telemetry_enabled,
                installation_id,
                panic_telemetry_context(&cli, interactive),
            );
        }
        if !matches!(cli.command, Command::Telemetry { .. }) {
            let mut input = std::io::stdin().lock();
            let mut output = std::io::stderr().lock();
            let _ = reporter
                .process_pending(interactive, &mut input, &mut output)
                .await;
        }
    }
    if let Some(error) = startup_update_error {
        report_startup_update_error(telemetry.as_ref(), &cli, interactive, error).await;
    }
    if let Some(Err(error)) = compatibility_result {
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
        if let Some(reporter) = &telemetry
            && reporter.enabled()
        {
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

async fn report_startup_update_error<E>(
    telemetry: Option<&TelemetryReporter<E>>,
    cli: &Cli,
    interactive: bool,
    error: nan_harness_runtime::update::UpdateError,
) where
    E: ErrorReportExporter,
{
    let Some(reporter) = telemetry.filter(|reporter| reporter.enabled()) else {
        return;
    };
    let error = error::CliError::Update(error);
    if !error.should_report_telemetry(cli) {
        return;
    }
    report_contexts(
        Some(reporter),
        vec![error.telemetry_context(cli, interactive, None)],
    )
    .await;
}

async fn report_run_result(
    telemetry: Option<&TelemetryReporter<GlitchTipExporter>>,
    cli: &Cli,
    interactive: bool,
) -> ExitCode {
    let mut bridge_diagnostics = Vec::new();
    match runner::run(cli, interactive, &mut bridge_diagnostics).await {
        Ok(exit_code) => {
            report_contexts(
                telemetry,
                bridge_diagnostic_contexts(&bridge_diagnostics, cli, interactive),
            )
            .await;
            exit_code_from_i32(exit_code)
        }
        Err(run_error) => {
            let error = run_error.error();
            let message = error.user_message(cli);
            eprintln!("{}", message.render_terminal());
            let mut contexts = bridge_diagnostic_contexts(&bridge_diagnostics, cli, interactive);
            if message.is_reportable() && error.should_report_telemetry(cli) {
                contexts.push(error.telemetry_context(cli, interactive, run_error.harness()));
            }
            report_contexts(telemetry, contexts).await;
            ExitCode::FAILURE
        }
    }
}

async fn report_contexts<E>(
    telemetry: Option<&TelemetryReporter<E>>,
    contexts: Vec<nan_harness_telemetry::event::ErrorReportContext>,
) where
    E: ErrorReportExporter,
{
    if let Some(reporter) = telemetry
        && !contexts.is_empty()
    {
        let mut input = std::io::stdin().lock();
        let mut output = std::io::stderr().lock();
        let _ = reporter
            .report_batch(contexts, &mut input, &mut output)
            .await;
    }
}

fn exit_code_from_i32(value: i32) -> ExitCode {
    ExitCode::from(u8::try_from(value.clamp(0, 255)).unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::{Cli, Command, report_startup_update_error};
    use clap::Parser as _;
    use nan_harness_runtime::update::UpdateError;
    use nan_harness_telemetry::TelemetryReporter;
    use nan_harness_telemetry::consent::{TelemetryPreference, TelemetrySettingsStore};
    use nan_harness_telemetry::glitchtip::{ErrorReportExporter, ExportError, ExportFuture};
    use nan_harness_telemetry::panic::PendingReportStore;
    use nan_harness_telemetry::redaction::SanitizedErrorReport;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct RecordingExporter {
        reports: Arc<Mutex<Vec<SanitizedErrorReport>>>,
    }

    impl ErrorReportExporter for RecordingExporter {
        fn export<'a>(&'a self, report: &'a SanitizedErrorReport) -> ExportFuture<'a> {
            let reports = Arc::clone(&self.reports);
            let report = report.clone();
            Box::pin(async move {
                reports
                    .lock()
                    .expect("recorded reports lock should not be poisoned")
                    .push(report);
                Ok::<(), ExportError>(())
            })
        }
    }

    #[test]
    fn direct_chat_commands_accept_the_gateway_escape_hatch() {
        for harness in [
            "opencode",
            "hermes",
            "pi",
            "prime-agent",
            "dsh",
            "openclaw",
            "cline",
            "qwen",
            "kimi",
            "aider",
            "goose",
        ] {
            let cli = Cli::try_parse_from(["nan", harness, "--no-chat-gateway"])
                .unwrap_or_else(|error| panic!("{harness} should accept the option: {error}"));
            assert!(crate::runner::direct_chat_gateway_disabled(&cli));
        }
    }

    #[test]
    fn translated_bridges_reject_the_gateway_escape_hatch() {
        for harness in ["claude", "codex", "fx"] {
            let error = Cli::try_parse_checked_from(["nan", harness, "--no-chat-gateway"])
                .expect_err("translated bridges should reject the DirectChat-only option");
            assert!(error.to_string().contains(
                "`--no-chat-gateway` is available only for harnesses that use OpenAI Chat Completions"
            ));
        }
    }

    #[test]
    fn translated_bridges_can_forward_the_same_spelling_after_the_separator() {
        let cli = Cli::try_parse_checked_from(["nan", "claude", "--", "--no-chat-gateway"])
            .expect("separator should preserve the native harness argument");
        let Command::Claude(arguments) = cli.command else {
            panic!("expected Claude command");
        };
        assert_eq!(arguments.run.arguments, ["--no-chat-gateway"]);
    }

    #[tokio::test]
    async fn enabled_telemetry_reports_non_fatal_startup_update_failures() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let settings = TelemetrySettingsStore::new(directory.path());
        settings
            .set(TelemetryPreference::On)
            .expect("telemetry should enable");
        let exporter = RecordingExporter::default();
        let reports = Arc::clone(&exporter.reports);
        let reporter = TelemetryReporter::new(
            settings,
            PendingReportStore::new(directory.path()),
            Some(exporter),
        );
        let cli = Cli::try_parse_from(["nan", "pi"]).expect("CLI should parse");

        report_startup_update_error(
            Some(&reporter),
            &cli,
            true,
            UpdateError::ReplaceExecutable(std::io::Error::from(std::io::ErrorKind::NotFound)),
        )
        .await;

        let reports = reports
            .lock()
            .expect("recorded reports lock should not be poisoned");
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].as_report().failure().code(), "NH-UPDATE-006");
    }

    #[tokio::test]
    async fn disabled_telemetry_does_not_report_startup_update_failures() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let exporter = RecordingExporter::default();
        let reports = Arc::clone(&exporter.reports);
        let reporter = TelemetryReporter::new(
            TelemetrySettingsStore::new(directory.path()),
            PendingReportStore::new(directory.path()),
            Some(exporter),
        );
        let cli = Cli::try_parse_from(["nan", "pi"]).expect("CLI should parse");

        report_startup_update_error(
            Some(&reporter),
            &cli,
            true,
            UpdateError::ReplaceExecutable(std::io::Error::from(std::io::ErrorKind::NotFound)),
        )
        .await;

        assert!(
            reports
                .lock()
                .expect("recorded reports lock should not be poisoned")
                .is_empty()
        );
    }
}
