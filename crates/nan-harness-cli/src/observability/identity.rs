use crate::app::{Cli, Command};
use crate::runner;
use nan_harness_core::{DetectedHarness, HarnessKind};
use nan_harness_runtime::{DiscoveryOptions, discover_harness};
use nan_harness_telemetry::event::{
    CompatibilityStatus as TelemetryCompatibilityStatus,
    HarnessIdentity as TelemetryHarnessIdentity, HarnessKind as TelemetryHarnessKind,
};
use std::path::Path;

use super::context::HarnessIdentitySource;

pub(super) fn telemetry_harness_identity(
    cli: &Cli,
    source: HarnessIdentitySource<'_>,
) -> Option<TelemetryHarnessIdentity> {
    match source {
        HarnessIdentitySource::Known(harness) => Some(telemetry_detected_harness(harness)),
        HarnessIdentitySource::KindOnly => {
            Some(TelemetryHarnessIdentity::new(telemetry_harness(cli)?, None))
        }
        HarnessIdentitySource::Detect => {
            let kind = telemetry_harness(cli)?;
            let (core_kind, executable, options) = telemetry_discovery_input(cli)?;
            let Ok(report) = discover_harness(core_kind, executable, options) else {
                return Some(TelemetryHarnessIdentity::new(kind, None));
            };
            Some(
                TelemetryHarnessIdentity::new(
                    kind,
                    normalized_version(&report.harness.detected_version),
                )
                .with_compatibility(telemetry_compatibility(report.harness.version_status)),
            )
        }
    }
}

fn telemetry_detected_harness(harness: &DetectedHarness) -> TelemetryHarnessIdentity {
    TelemetryHarnessIdentity::new(
        telemetry_harness_kind(harness.kind),
        normalized_version(&harness.detected_version),
    )
    .with_compatibility(telemetry_compatibility(harness.version_status))
}

fn telemetry_discovery_input(cli: &Cli) -> Option<(HarnessKind, Option<&Path>, DiscoveryOptions)> {
    if let Command::Doctor(arguments) = &cli.command {
        let Some(crate::app::DoctorTarget::Stable(harness)) = arguments.harness else {
            return None;
        };
        return Some((
            harness,
            arguments.executable.as_deref(),
            DiscoveryOptions {
                allow_unsupported: true,
                allow_untested: true,
            },
        ));
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

pub(super) const fn telemetry_harness(cli: &Cli) -> Option<TelemetryHarnessKind> {
    match &cli.command {
        Command::Doctor(arguments) => telemetry_harness_for_doctor(arguments.harness),
        Command::Config(arguments) => match arguments.harness {
            Some(crate::app::ConfigTarget::Stable(kind)) => Some(telemetry_harness_kind(kind)),
            Some(crate::app::ConfigTarget::Pen) => Some(TelemetryHarnessKind::PenDesktop),
            None => None,
        },
        Command::Auth { .. }
        | Command::Update
        | Command::Uninstall(_)
        | Command::Telemetry { .. }
        | Command::Completions { .. }
        | Command::RecordInstallation(_) => None,
        command => telemetry_harness_for_command(command),
    }
}

const fn telemetry_harness_for_doctor(
    target: Option<crate::app::DoctorTarget>,
) -> Option<TelemetryHarnessKind> {
    match target {
        Some(crate::app::DoctorTarget::Stable(kind)) => Some(telemetry_harness_kind(kind)),
        Some(crate::app::DoctorTarget::Experimental(kind)) => Some(match kind {
            nan_harness_core::DesktopHarnessKind::ChatGpt => TelemetryHarnessKind::ChatGptDesktop,
            nan_harness_core::DesktopHarnessKind::Claude => TelemetryHarnessKind::ClaudeDesktop,
            nan_harness_core::DesktopHarnessKind::Hermes => TelemetryHarnessKind::HermesDesktop,
            nan_harness_core::DesktopHarnessKind::Pen => TelemetryHarnessKind::PenDesktop,
        }),
        None => None,
    }
}

const fn telemetry_harness_kind(kind: HarnessKind) -> TelemetryHarnessKind {
    match kind {
        HarnessKind::ClaudeCode => TelemetryHarnessKind::ClaudeCode,
        HarnessKind::Codex => TelemetryHarnessKind::Codex,
        HarnessKind::OpenCode => TelemetryHarnessKind::OpenCode,
        HarnessKind::Hermes => TelemetryHarnessKind::Hermes,
        HarnessKind::Pi => TelemetryHarnessKind::Pi,
        HarnessKind::Omp => TelemetryHarnessKind::Omp,
        HarnessKind::PrimeAgent => TelemetryHarnessKind::PrimeAgent,
        HarnessKind::DeepSeekHarness => TelemetryHarnessKind::DeepSeekHarness,
        HarnessKind::OpenClaw => TelemetryHarnessKind::OpenClaw,
        HarnessKind::Cline => TelemetryHarnessKind::Cline,
        HarnessKind::QwenCode => TelemetryHarnessKind::QwenCode,
        HarnessKind::KimiCode => TelemetryHarnessKind::KimiCode,
        HarnessKind::Aider => TelemetryHarnessKind::Aider,
        HarnessKind::Goose => TelemetryHarnessKind::Goose,
        HarnessKind::Fx => TelemetryHarnessKind::Fx,
    }
}

const fn telemetry_harness_for_command(command: &Command) -> Option<TelemetryHarnessKind> {
    match command {
        Command::Claude(_) => Some(TelemetryHarnessKind::ClaudeCode),
        Command::ChatGptDesktop(_) => Some(TelemetryHarnessKind::ChatGptDesktop),
        Command::ClaudeDesktop(_) => Some(TelemetryHarnessKind::ClaudeDesktop),
        Command::Codex(_) => Some(TelemetryHarnessKind::Codex),
        Command::OpenCode(_) => Some(TelemetryHarnessKind::OpenCode),
        Command::Hermes(_) => Some(TelemetryHarnessKind::Hermes),
        Command::HermesDesktop(_) => Some(TelemetryHarnessKind::HermesDesktop),
        Command::PenDesktop(_) => Some(TelemetryHarnessKind::PenDesktop),
        Command::Pi(_) => Some(TelemetryHarnessKind::Pi),
        Command::Omp(_) => Some(TelemetryHarnessKind::Omp),
        Command::Prime(_) => Some(TelemetryHarnessKind::PrimeAgent),
        Command::DeepSeek(_) => Some(TelemetryHarnessKind::DeepSeekHarness),
        Command::OpenClaw(_) => Some(TelemetryHarnessKind::OpenClaw),
        Command::Cline(_) => Some(TelemetryHarnessKind::Cline),
        Command::Qwen(_) => Some(TelemetryHarnessKind::QwenCode),
        Command::Kimi(_) => Some(TelemetryHarnessKind::KimiCode),
        Command::Aider(_) => Some(TelemetryHarnessKind::Aider),
        Command::Goose(_) => Some(TelemetryHarnessKind::Goose),
        Command::Fx(_) => Some(TelemetryHarnessKind::Fx),
        Command::Doctor(_)
        | Command::Config(_)
        | Command::Auth { .. }
        | Command::Update
        | Command::Uninstall(_)
        | Command::Telemetry { .. }
        | Command::Completions { .. }
        | Command::RecordInstallation(_) => None,
    }
}
