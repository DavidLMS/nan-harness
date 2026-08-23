use crate::app::{Cli, Command};
use crate::runner;
use nan_harness_core::HarnessKind;
use nan_harness_runtime::{DiscoveryOptions, discover_harness};
use nan_harness_telemetry::TelemetryReporter;
use nan_harness_telemetry::analytics::{DEFAULT_USAGE_EXPORT_TIMEOUT, UmamiExporter, UsageEvent};
use nan_harness_telemetry::consent::TelemetrySettingsStore;
use nan_harness_telemetry::event::{
    CompatibilityStatus as TelemetryCompatibilityStatus, ErrorReportContext, Failure,
    HarnessIdentity as TelemetryHarnessIdentity, HarnessKind as TelemetryHarnessKind,
    OperationContext, OperationKind, Transport as TelemetryTransport,
};
use nan_harness_telemetry::glitchtip::{DEFAULT_EXPORT_TIMEOUT, GlitchTipExporter};
use nan_harness_telemetry::panic::PendingReportStore;
use std::path::Path;

pub(crate) fn telemetry_reporter() -> Option<TelemetryReporter<GlitchTipExporter>> {
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

pub(crate) fn start_usage_analytics(
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

pub(crate) fn panic_telemetry_context(cli: &Cli, interactive: bool) -> ErrorReportContext {
    enrich_telemetry_context(
        ErrorReportContext::new(Failure::panic(), interactive),
        cli,
        false,
    )
}

pub(crate) fn enrich_telemetry_context(
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
        Command::Claude(arguments)
        | Command::Codex(arguments)
        | Command::OpenCode(arguments)
        | Command::Hermes(arguments)
        | Command::Pi(arguments)
        | Command::Prime(arguments)
        | Command::DeepSeek(arguments)
        | Command::OpenClaw(arguments)
        | Command::Cline(arguments)
        | Command::Qwen(arguments)
        | Command::Kimi(arguments)
        | Command::Aider(arguments)
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
        Command::Config(arguments) => {
            OperationContext::new(if arguments.remove || arguments.remove_all {
                OperationKind::HarnessConfigRemove
            } else {
                OperationKind::HarnessConfig
            })
        }
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
        Command::Config(arguments) => match arguments.harness {
            Some(HarnessKind::ClaudeCode) => Some(TelemetryHarnessKind::ClaudeCode),
            Some(HarnessKind::Codex) => Some(TelemetryHarnessKind::Codex),
            Some(HarnessKind::OpenCode) => Some(TelemetryHarnessKind::OpenCode),
            Some(HarnessKind::Hermes) => Some(TelemetryHarnessKind::Hermes),
            Some(HarnessKind::Pi) => Some(TelemetryHarnessKind::Pi),
            Some(HarnessKind::PrimeAgent) => Some(TelemetryHarnessKind::PrimeAgent),
            Some(HarnessKind::DeepSeekHarness) => Some(TelemetryHarnessKind::DeepSeekHarness),
            Some(HarnessKind::OpenClaw) => Some(TelemetryHarnessKind::OpenClaw),
            Some(HarnessKind::Cline) => Some(TelemetryHarnessKind::Cline),
            Some(HarnessKind::QwenCode) => Some(TelemetryHarnessKind::QwenCode),
            Some(HarnessKind::KimiCode) => Some(TelemetryHarnessKind::KimiCode),
            Some(HarnessKind::Aider) => Some(TelemetryHarnessKind::Aider),
            Some(HarnessKind::Goose) => Some(TelemetryHarnessKind::Goose),
            Some(HarnessKind::Fx) => Some(TelemetryHarnessKind::Fx),
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
        | Command::Config(_)
        | Command::Auth { .. }
        | Command::Update
        | Command::Uninstall(_)
        | Command::Telemetry { .. }
        | Command::RecordInstallation(_) => None,
    }
}
