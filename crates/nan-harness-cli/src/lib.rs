#![forbid(unsafe_code)]

mod app;
mod commands;
mod error;
mod runner;

use error::CliError;

use app::{Cli, Command, HarnessRunArgs, PersistentHarnessRunArgs};
use clap::Parser;
use commands::credentials::CredentialError;
use commands::install::{
    InstallDecision, InstallError, executable_from_known_locations, install_spec, offer_install,
};
use commands::persistence::{
    IntegrationChange, PersistenceError, PersistenceManager, RemovalOutcome,
    effective_provider_base_url,
};
use commands::uninstall::UninstallError;
use nan_harness_adapters::{
    AiderAdapter, ClaudeCodeAdapter, ClineAdapter, CodexAdapter, DeepSeekHarnessAdapter, FxAdapter,
    GooseAdapter, HermesAdapter, KimiCodeAdapter, OpenClawAdapter, OpenCodeAdapter,
    PersistentAiderAdapter, PersistentDeepSeekHarnessAdapter, PersistentPiAdapter,
    PersistentPrimeAgentAdapter, PersistentQwenCodeAdapter, PiAdapter, PrimeAgentAdapter,
    QwenCodeAdapter,
};
use nan_harness_core::launch_plan::{LaunchId, ObservabilityFormat};
use nan_harness_core::model::{ModelAvailability, ProfileSource, QualificationStatus};
use nan_harness_core::{
    HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError, ResolvedModel,
    build_validated_plan,
};
use nan_harness_runtime::{
    CancellationToken, DiscoveryError, DiscoveryOptions, ProcessError, ResolvedConfig,
    RuntimeError, SignalKind, Supervisor, discover_harness,
};
use nan_harness_telemetry::TelemetryReporter;
use nan_harness_telemetry::analytics::{DEFAULT_USAGE_EXPORT_TIMEOUT, UmamiExporter, UsageEvent};
use nan_harness_telemetry::consent::{SettingsError, TelemetrySettingsStore};
use nan_harness_telemetry::event::{
    CompatibilityStatus as TelemetryCompatibilityStatus, ErrorReportContext, Failure,
    FailureCategory, FailureCause, FailureStage, HarnessIdentity as TelemetryHarnessIdentity,
    HarnessKind as TelemetryHarnessKind, OperationContext, OperationKind,
    Transport as TelemetryTransport,
};
use nan_harness_telemetry::glitchtip::{DEFAULT_EXPORT_TIMEOUT, GlitchTipExporter};
use nan_harness_telemetry::panic::{PendingReportStore, install_panic_hook};
use std::fmt::Write as _;
use std::io::IsTerminal as _;
use std::path::Path;
use std::process::ExitCode;
use thiserror::Error;

const DEFAULT_MODEL_ID: &str = "qwen3.6";

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
            eprintln!("error [{}]: {error}", error.code());
            if let Some(reporter) = &telemetry {
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

fn telemetry_reporter() -> Option<TelemetryReporter<GlitchTipExporter>> {
    let settings = TelemetrySettingsStore::from_environment().ok()?;
    let pending = PendingReportStore::new(settings.directory());
    let dsn = std::env::var("NAN_HARNESS_GLITCHTIP_DSN")
        .ok()
        .or_else(|| option_env!("NAN_HARNESS_GLITCHTIP_DSN").map(ToOwned::to_owned));
    let exporter = dsn
        .as_deref()
        .and_then(|value| GlitchTipExporter::new(value, DEFAULT_EXPORT_TIMEOUT).ok());
    Some(TelemetryReporter::new(settings, pending, exporter))
}

fn start_usage_analytics(
    cli: &Cli,
    telemetry: Option<&TelemetryReporter<GlitchTipExporter>>,
) -> Option<tokio::task::JoinHandle<()>> {
    if matches!(cli.command, Command::Telemetry { .. }) {
        return None;
    }
    let installation_id = telemetry?
        .settings()
        .active_installation_id()
        .ok()
        .flatten()?;
    let base_url = configured_value(
        "NAN_HARNESS_UMAMI_URL",
        option_env!("NAN_HARNESS_UMAMI_URL"),
    )?;
    let website_id = configured_value(
        "NAN_HARNESS_UMAMI_WEBSITE_ID",
        option_env!("NAN_HARNESS_UMAMI_WEBSITE_ID"),
    )?;
    let exporter = UmamiExporter::new(&base_url, &website_id, DEFAULT_USAGE_EXPORT_TIMEOUT).ok()?;
    let event = UsageEvent::new(
        telemetry_harness(cli),
        telemetry_operation(cli).kind(),
        telemetry_transport(cli),
    );
    Some(tokio::spawn(async move {
        let _ = exporter.export(&installation_id, event).await;
    }))
}

fn configured_value(name: &str, embedded: Option<&str>) -> Option<String> {
    match std::env::var(name) {
        Ok(value) if value.is_empty() => None,
        Ok(value) => Some(value),
        Err(_) => embedded
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
    }
}

fn exit_code_from_i32(value: i32) -> ExitCode {
    ExitCode::from(u8::try_from(value.clamp(0, 255)).unwrap_or(1))
}

fn panic_telemetry_context(cli: &Cli, interactive: bool) -> ErrorReportContext {
    enrich_telemetry_context(
        ErrorReportContext::new(Failure::panic(), interactive),
        cli,
        false,
    )
}

fn enrich_telemetry_context(
    mut context: ErrorReportContext,
    cli: &Cli,
    detect_version: bool,
) -> ErrorReportContext {
    if let Some(harness) = telemetry_harness_identity(cli, detect_version) {
        context = context.with_harness(harness);
    }
    if let Some(transport) = telemetry_transport(cli) {
        context = context.with_transport(transport);
    }
    context.with_operation(telemetry_operation(cli))
}

fn telemetry_harness_identity(cli: &Cli, detect_version: bool) -> Option<TelemetryHarnessIdentity> {
    let kind = telemetry_harness(cli)?;
    if !detect_version {
        return Some(TelemetryHarnessIdentity::new(kind, None));
    }
    let (core_kind, executable, options) = telemetry_discovery_input(cli)?;
    let Ok(report) = discover_harness(core_kind, executable, options) else {
        return Some(TelemetryHarnessIdentity::new(kind, None));
    };
    let version = normalized_version(&report.harness.detected_version);
    Some(
        TelemetryHarnessIdentity::new(kind, version)
            .with_compatibility(telemetry_compatibility(report.harness.version_status)),
    )
}

fn telemetry_discovery_input(cli: &Cli) -> Option<(HarnessKind, Option<&Path>, DiscoveryOptions)> {
    if let Command::Doctor(arguments) = &cli.command {
        return arguments.harness.map(|harness| {
            (
                harness,
                arguments.executable.as_deref(),
                DiscoveryOptions {
                    allow_unsupported: true,
                    allow_untested: true,
                },
            )
        });
    }
    let (kind, arguments) = runner::harness_run_arguments(cli)?;
    Some((
        kind,
        arguments.executable.as_deref(),
        DiscoveryOptions {
            allow_unsupported: true,
            allow_untested: true,
        },
    ))
}

fn normalized_version(output: &str) -> Option<String> {
    output.split_whitespace().find_map(|token| {
        let candidate = token
            .trim_matches(|character: char| !character.is_ascii_alphanumeric() && character != '.')
            .trim_start_matches('v');
        semver::Version::parse(candidate)
            .ok()
            .map(|version| version.to_string())
    })
}

const fn telemetry_compatibility(
    status: nan_harness_core::harness::VersionStatus,
) -> TelemetryCompatibilityStatus {
    use nan_harness_core::harness::VersionStatus;

    match status {
        VersionStatus::Tested => TelemetryCompatibilityStatus::Tested,
        VersionStatus::Supported => TelemetryCompatibilityStatus::Supported,
        VersionStatus::NewerUntested => TelemetryCompatibilityStatus::NewerUntested,
        VersionStatus::OlderUnsupported => TelemetryCompatibilityStatus::OlderUnsupported,
        VersionStatus::Unparseable => TelemetryCompatibilityStatus::Unparseable,
    }
}

fn telemetry_operation(cli: &Cli) -> OperationContext {
    match &cli.command {
        Command::OpenCode(arguments)
        | Command::Pi(arguments)
        | Command::Prime(arguments)
        | Command::DeepSeek(arguments)
        | Command::Qwen(arguments)
        | Command::Aider(arguments) => {
            let kind = if arguments.unpersist {
                OperationKind::HarnessUnpersist
            } else if arguments.persist {
                OperationKind::HarnessPersist
            } else if arguments.run.dry_run {
                OperationKind::HarnessDryRun
            } else {
                OperationKind::HarnessRun
            };
            OperationContext::new(kind)
        }
        Command::Claude(arguments)
        | Command::Codex(arguments)
        | Command::Hermes(arguments)
        | Command::OpenClaw(arguments)
        | Command::Cline(arguments)
        | Command::Kimi(arguments)
        | Command::Goose(arguments)
        | Command::Fx(arguments) => {
            let kind = if arguments.dry_run {
                OperationKind::HarnessDryRun
            } else {
                OperationKind::HarnessRun
            };
            OperationContext::new(kind)
        }
        Command::Doctor(_) => OperationContext::new(OperationKind::Doctor),
        Command::Update | Command::RecordInstallation(_) => {
            OperationContext::new(OperationKind::Update)
        }
        Command::Uninstall(_) => OperationContext::new(OperationKind::Uninstall),
        Command::Auth { .. } | Command::Telemetry { .. } => {
            OperationContext::new(OperationKind::TelemetryConfiguration)
        }
    }
}

const fn telemetry_harness(cli: &Cli) -> Option<TelemetryHarnessKind> {
    match &cli.command {
        Command::Claude(_) => Some(TelemetryHarnessKind::ClaudeCode),
        Command::Codex(_) => Some(TelemetryHarnessKind::Codex),
        Command::OpenCode(_) => Some(TelemetryHarnessKind::OpenCode),
        Command::Hermes(_) => Some(TelemetryHarnessKind::Hermes),
        Command::Pi(_) => Some(TelemetryHarnessKind::Pi),
        Command::Prime(_) => Some(TelemetryHarnessKind::PrimeAgent),
        Command::DeepSeek(_) => Some(TelemetryHarnessKind::DeepSeekHarness),
        Command::OpenClaw(_) => Some(TelemetryHarnessKind::OpenClaw),
        Command::Cline(_) => Some(TelemetryHarnessKind::Cline),
        Command::Qwen(_) => Some(TelemetryHarnessKind::QwenCode),
        Command::Kimi(_) => Some(TelemetryHarnessKind::KimiCode),
        Command::Aider(_) => Some(TelemetryHarnessKind::Aider),
        Command::Goose(_) => Some(TelemetryHarnessKind::Goose),
        Command::Fx(_) => Some(TelemetryHarnessKind::Fx),
        Command::Doctor(arguments) => match arguments.harness {
            Some(harness) => Some(match harness {
                HarnessKind::ClaudeCode => TelemetryHarnessKind::ClaudeCode,
                HarnessKind::Codex => TelemetryHarnessKind::Codex,
                HarnessKind::OpenCode => TelemetryHarnessKind::OpenCode,
                HarnessKind::Hermes => TelemetryHarnessKind::Hermes,
                HarnessKind::Pi => TelemetryHarnessKind::Pi,
                HarnessKind::PrimeAgent => TelemetryHarnessKind::PrimeAgent,
                HarnessKind::DeepSeekHarness => TelemetryHarnessKind::DeepSeekHarness,
                HarnessKind::OpenClaw => TelemetryHarnessKind::OpenClaw,
                HarnessKind::Cline => TelemetryHarnessKind::Cline,
                HarnessKind::QwenCode => TelemetryHarnessKind::QwenCode,
                HarnessKind::KimiCode => TelemetryHarnessKind::KimiCode,
                HarnessKind::Aider => TelemetryHarnessKind::Aider,
                HarnessKind::Goose => TelemetryHarnessKind::Goose,
                HarnessKind::Fx => TelemetryHarnessKind::Fx,
            }),
            None => None,
        },
        Command::Auth { .. }
        | Command::Update
        | Command::Uninstall(_)
        | Command::Telemetry { .. }
        | Command::RecordInstallation(_) => None,
    }
}

const fn telemetry_transport(cli: &Cli) -> Option<TelemetryTransport> {
    match cli.command {
        Command::Claude(_) => Some(TelemetryTransport::AnthropicBridge),
        Command::Codex(_) => Some(TelemetryTransport::ResponsesBridge),
        Command::OpenCode(_)
        | Command::Hermes(_)
        | Command::Pi(_)
        | Command::Prime(_)
        | Command::DeepSeek(_)
        | Command::OpenClaw(_)
        | Command::Cline(_)
        | Command::Qwen(_)
        | Command::Kimi(_)
        | Command::Aider(_)
        | Command::Goose(_) => Some(TelemetryTransport::DirectChat),
        Command::Fx(_) => Some(TelemetryTransport::FxGatewayBridge),
        Command::Doctor(_)
        | Command::Auth { .. }
        | Command::Update
        | Command::Uninstall(_)
        | Command::Telemetry { .. }
        | Command::RecordInstallation(_) => None,
    }
}
