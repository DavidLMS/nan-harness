use crate::app::{Cli, Command};
use crate::runner;
use nan_harness_core::{DetectedHarness, HarnessKind};
use nan_harness_runtime::{
    BridgeDiagnostic, BridgeDiagnosticReason, BridgeEndpoint as RuntimeBridgeEndpoint,
    BridgeModelPolicy as RuntimeModelPolicy, BridgeReasoningRequest as RuntimeReasoningRequest,
};
use nan_harness_runtime::{DiscoveryOptions, discover_harness};
use nan_harness_telemetry::TelemetryReporter;
use nan_harness_telemetry::analytics::{DEFAULT_USAGE_EXPORT_TIMEOUT, UmamiExporter, UsageEvent};
use nan_harness_telemetry::consent::TelemetrySettingsStore;
use nan_harness_telemetry::diagnostic::{
    BridgeEndpoint, Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason,
    DocumentKind, ModelPolicy, ReasoningRequest,
};
use nan_harness_telemetry::event::{
    CompatibilityStatus as TelemetryCompatibilityStatus, ErrorReportContext, Failure,
    FailureCategory, FailureCause, FailureStage, HarnessIdentity as TelemetryHarnessIdentity,
    HarnessKind as TelemetryHarnessKind, OperationContext, OperationKind,
    Transport as TelemetryTransport,
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
    let operation = telemetry_operation(cli).kind();
    let transport = telemetry_transport(cli);
    let mut event = UsageEvent::new(telemetry_harness(cli), operation, transport);
    if operation == OperationKind::HarnessRun && transport == Some(TelemetryTransport::DirectChat) {
        event = event.with_chat_gateway(!runner::direct_chat_gateway_disabled(cli));
    }
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
        HarnessIdentitySource::KindOnly,
    )
}

#[derive(Clone, Copy)]
pub(crate) enum HarnessIdentitySource<'a> {
    Detect,
    Known(&'a DetectedHarness),
    KindOnly,
}

pub(crate) fn enrich_telemetry_context(
    mut context: ErrorReportContext,
    cli: &Cli,
    harness_source: HarnessIdentitySource<'_>,
) -> ErrorReportContext {
    if let Some(harness) = telemetry_harness_identity(cli, harness_source) {
        context = context.with_harness(harness);
    }
    if let Some(transport) = telemetry_transport(cli) {
        context = context.with_transport(transport);
    }
    context.with_operation(telemetry_operation(cli))
}

pub(crate) fn is_harness_dry_run(cli: &Cli) -> bool {
    telemetry_operation(cli).kind() == OperationKind::HarnessDryRun
}

fn telemetry_harness_identity(
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

fn telemetry_operation(cli: &Cli) -> OperationContext {
    match &cli.command {
        Command::ChatGptDesktop(arguments) => OperationContext::new(if arguments.dry_run {
            OperationKind::HarnessDryRun
        } else {
            OperationKind::HarnessRun
        }),
        Command::ClaudeDesktop(arguments) => OperationContext::new(if arguments.dry_run {
            OperationKind::HarnessDryRun
        } else {
            OperationKind::HarnessRun
        }),
        Command::HermesDesktop(arguments) => OperationContext::new(if arguments.run.dry_run {
            OperationKind::HarnessDryRun
        } else {
            OperationKind::HarnessRun
        }),
        Command::Claude(arguments) | Command::Codex(arguments) | Command::Fx(arguments) => {
            let kind = if arguments.run.dry_run {
                OperationKind::HarnessDryRun
            } else {
                OperationKind::HarnessRun
            };
            OperationContext::new(kind)
        }
        Command::OpenCode(arguments)
        | Command::Hermes(arguments)
        | Command::Pi(arguments)
        | Command::Prime(arguments)
        | Command::DeepSeek(arguments)
        | Command::OpenClaw(arguments)
        | Command::Cline(arguments)
        | Command::Qwen(arguments)
        | Command::Kimi(arguments)
        | Command::Aider(arguments)
        | Command::Goose(arguments) => {
            let kind = if arguments.run.dry_run {
                OperationKind::HarnessDryRun
            } else {
                OperationKind::HarnessRun
            };
            OperationContext::new(kind)
        }
        Command::Doctor(_) => OperationContext::new(OperationKind::Doctor),
        Command::Update | Command::Completions { .. } | Command::RecordInstallation(_) => {
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

const fn telemetry_harness_kind(kind: HarnessKind) -> TelemetryHarnessKind {
    match kind {
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
    }
}

const fn telemetry_harness(cli: &Cli) -> Option<TelemetryHarnessKind> {
    match &cli.command {
        Command::Claude(_) => Some(TelemetryHarnessKind::ClaudeCode),
        Command::ChatGptDesktop(_) => Some(TelemetryHarnessKind::ChatGptDesktop),
        Command::ClaudeDesktop(_) => Some(TelemetryHarnessKind::ClaudeDesktop),
        Command::Codex(_) => Some(TelemetryHarnessKind::Codex),
        Command::OpenCode(_) => Some(TelemetryHarnessKind::OpenCode),
        Command::Hermes(_) => Some(TelemetryHarnessKind::Hermes),
        Command::HermesDesktop(_) => Some(TelemetryHarnessKind::HermesDesktop),
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
            Some(crate::app::DoctorTarget::Stable(kind)) => Some(telemetry_harness_kind(kind)),
            Some(crate::app::DoctorTarget::Experimental(kind)) => Some(match kind {
                nan_harness_core::DesktopHarnessKind::ChatGpt => {
                    TelemetryHarnessKind::ChatGptDesktop
                }
                nan_harness_core::DesktopHarnessKind::Claude => TelemetryHarnessKind::ClaudeDesktop,
                nan_harness_core::DesktopHarnessKind::Hermes => TelemetryHarnessKind::HermesDesktop,
            }),
            None => None,
        },
        Command::Config(arguments) => match arguments.harness {
            Some(kind) => Some(telemetry_harness_kind(kind)),
            None => None,
        },
        Command::Auth { .. }
        | Command::Update
        | Command::Uninstall(_)
        | Command::Telemetry { .. }
        | Command::Completions { .. }
        | Command::RecordInstallation(_) => None,
    }
}

const fn telemetry_transport(cli: &Cli) -> Option<TelemetryTransport> {
    match cli.command {
        Command::Claude(_) | Command::ClaudeDesktop(_) => Some(TelemetryTransport::AnthropicBridge),
        Command::Codex(_) | Command::ChatGptDesktop(_) => Some(TelemetryTransport::ResponsesBridge),
        Command::OpenCode(_)
        | Command::Hermes(_)
        | Command::HermesDesktop(_)
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
        | Command::Completions { .. }
        | Command::RecordInstallation(_) => None,
    }
}

pub(crate) async fn report_compat_error(
    reporter: &TelemetryReporter<GlitchTipExporter>,
    error: &nan_harness_runtime::CompatibilityError,
    cli: &Cli,
    interactive: bool,
) {
    let cause = match error {
        nan_harness_runtime::CompatibilityError::FetchManifest(_)
        | nan_harness_runtime::CompatibilityError::ManifestStatus(_) => FailureCause::Network,
        nan_harness_runtime::CompatibilityError::ParseManifest(_)
        | nan_harness_runtime::CompatibilityError::InvalidEmbeddedManifest(_) => {
            FailureCause::InvalidData
        }
        nan_harness_runtime::CompatibilityError::UnsupportedManifestSchema(_) => {
            FailureCause::UnsupportedVersion
        }
        nan_harness_runtime::CompatibilityError::LiveEvidenceAhead { .. }
        | nan_harness_runtime::CompatibilityError::VersionBelowMinimum { .. }
        | nan_harness_runtime::CompatibilityError::LiveVersionBelowMinimum { .. }
        | nan_harness_runtime::CompatibilityError::EmptyReleases
        | nan_harness_runtime::CompatibilityError::DuplicateRelease(_)
        | nan_harness_runtime::CompatibilityError::DuplicateHarness(_)
        | nan_harness_runtime::CompatibilityError::IncompleteEvidencePair { .. }
        | nan_harness_runtime::CompatibilityError::MissingEvidence { .. }
        | nan_harness_runtime::CompatibilityError::InvalidEvidenceTimestamp { .. }
        | nan_harness_runtime::CompatibilityError::InvalidUrl { .. }
        | nan_harness_runtime::CompatibilityError::InsecureUrl
        | nan_harness_runtime::CompatibilityError::ManifestTooLarge => FailureCause::InvalidData,
        nan_harness_runtime::CompatibilityError::MissingConfigDirectory
        | nan_harness_runtime::CompatibilityError::ReadState(_)
        | nan_harness_runtime::CompatibilityError::ParseState(_)
        | nan_harness_runtime::CompatibilityError::UnsupportedStateSchema(_)
        | nan_harness_runtime::CompatibilityError::CreateConfigDirectory(_)
        | nan_harness_runtime::CompatibilityError::SerializeState(_)
        | nan_harness_runtime::CompatibilityError::WriteState(_) => FailureCause::Filesystem,
        nan_harness_runtime::CompatibilityError::BuildClient(_)
        | nan_harness_runtime::CompatibilityError::SystemClock(_) => FailureCause::Internal,
    };
    let retryable = match error {
        nan_harness_runtime::CompatibilityError::FetchManifest(_) => true,
        nan_harness_runtime::CompatibilityError::ManifestStatus(status) => {
            matches!(status, 408 | 425 | 429 | 500..=599)
        }
        _ => false,
    };
    let failure = Failure::new(
        error.code(),
        FailureCategory::Configuration,
        FailureStage::Startup,
        retryable,
    )
    .with_cause(cause);
    let context = enrich_telemetry_context(
        ErrorReportContext::new(failure, interactive).with_diagnostic(compat_diagnostic(error)),
        cli,
        HarnessIdentitySource::KindOnly,
    );
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stderr().lock();
    let _ = reporter.report(context, &mut input, &mut output).await;
}

/// Reports bridge diagnostics (upstream transport/status/invalid-response
/// failures surfaced to a harness through the local bridge) to `GlitchTip` when
/// telemetry is enabled.
pub(crate) fn bridge_diagnostic_contexts(
    diagnostics: &[BridgeDiagnostic],
    cli: &Cli,
    interactive: bool,
) -> Vec<ErrorReportContext> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            let (category, stage, cause, retryable) = bridge_diagnostic_classification(diagnostic);
            let mut failure = Failure::new(diagnostic.code.to_owned(), category, stage, retryable)
                .with_cause(cause);
            if let Some(status) = diagnostic.http_status {
                failure = failure.with_http_status(status);
            }
            enrich_telemetry_context(
                ErrorReportContext::new(failure, interactive)
                    .with_diagnostic(bridge_diagnostic(diagnostic)),
                cli,
                HarnessIdentitySource::KindOnly,
            )
        })
        .collect()
}

fn bridge_diagnostic(diagnostic: &BridgeDiagnostic) -> Diagnostic {
    let reason = match diagnostic.reason {
        BridgeDiagnosticReason::AuthenticationRejected => DiagnosticReason::AuthenticationRejected,
        BridgeDiagnosticReason::InvalidRequest => DiagnosticReason::InvalidRequest,
        BridgeDiagnosticReason::ReasoningPolicyMismatch => {
            DiagnosticReason::ReasoningPolicyMismatch
        }
        BridgeDiagnosticReason::UpstreamTransport => DiagnosticReason::NetworkRequestFailed,
        BridgeDiagnosticReason::UpstreamStatus => DiagnosticReason::HttpRequestRejected,
        BridgeDiagnosticReason::InvalidUpstreamResponse => DiagnosticReason::InvalidResponse,
    };
    Diagnostic::new(
        reason,
        DiagnosticDetails::Bridge {
            endpoint: bridge_endpoint(diagnostic.endpoint),
            model_id: diagnostic.model_id.clone(),
            requested_reasoning: diagnostic.requested_reasoning.map(reasoning_request),
            model_policy: diagnostic.model_policy.map(model_policy),
        },
    )
}

const fn bridge_endpoint(endpoint: RuntimeBridgeEndpoint) -> BridgeEndpoint {
    match endpoint {
        RuntimeBridgeEndpoint::Models => BridgeEndpoint::Models,
        RuntimeBridgeEndpoint::Messages => BridgeEndpoint::Messages,
        RuntimeBridgeEndpoint::CountTokens => BridgeEndpoint::CountTokens,
        RuntimeBridgeEndpoint::Responses => BridgeEndpoint::Responses,
        RuntimeBridgeEndpoint::Search => BridgeEndpoint::Search,
        RuntimeBridgeEndpoint::FxGateway => BridgeEndpoint::FxGateway,
    }
}

const fn reasoning_request(request: RuntimeReasoningRequest) -> ReasoningRequest {
    match request {
        RuntimeReasoningRequest::Auto => ReasoningRequest::Auto,
        RuntimeReasoningRequest::None => ReasoningRequest::None,
        RuntimeReasoningRequest::Low => ReasoningRequest::Low,
        RuntimeReasoningRequest::Medium => ReasoningRequest::Medium,
        RuntimeReasoningRequest::High => ReasoningRequest::High,
        RuntimeReasoningRequest::Xhigh => ReasoningRequest::Xhigh,
        RuntimeReasoningRequest::Other => ReasoningRequest::Other,
    }
}

const fn model_policy(policy: RuntimeModelPolicy) -> ModelPolicy {
    match policy {
        RuntimeModelPolicy::Unsupported => ModelPolicy::Unsupported,
        RuntimeModelPolicy::Toggle => ModelPolicy::Toggle,
        RuntimeModelPolicy::Effort => ModelPolicy::Effort,
        RuntimeModelPolicy::AlwaysOn => ModelPolicy::AlwaysOn,
        RuntimeModelPolicy::Unknown => ModelPolicy::Unknown,
    }
}

fn compat_diagnostic(error: &nan_harness_runtime::CompatibilityError) -> Diagnostic {
    use nan_harness_runtime::CompatibilityError;

    match error {
        CompatibilityError::FetchManifest(_) => {
            Diagnostic::general(DiagnosticReason::NetworkRequestFailed)
        }
        CompatibilityError::ManifestStatus(status) => Diagnostic::new(
            DiagnosticReason::HttpRequestRejected,
            DiagnosticDetails::Http {
                operation: DiagnosticOperation::FetchUpdateManifest,
                status: *status,
            },
        ),
        CompatibilityError::UnsupportedManifestSchema(version) => Diagnostic::new(
            DiagnosticReason::UnsupportedVersion,
            DiagnosticDetails::Schema {
                document: DocumentKind::CompatibilityManifest,
                observed_version: Some(u16::from(*version)),
            },
        ),
        CompatibilityError::MissingConfigDirectory => {
            Diagnostic::general(DiagnosticReason::MissingDirectory)
        }
        CompatibilityError::ReadState(source)
        | CompatibilityError::CreateConfigDirectory(source)
        | CompatibilityError::WriteState(source) => Diagnostic::new(
            DiagnosticReason::FilesystemOperationFailed,
            DiagnosticDetails::Io {
                operation: DiagnosticOperation::ReadConfiguration,
                error_kind: nan_harness_telemetry::diagnostic::IoErrorKind::from_std(source.kind()),
            },
        ),
        CompatibilityError::ParseManifest(_)
        | CompatibilityError::InvalidEmbeddedManifest(_)
        | CompatibilityError::LiveEvidenceAhead { .. }
        | CompatibilityError::VersionBelowMinimum { .. }
        | CompatibilityError::LiveVersionBelowMinimum { .. }
        | CompatibilityError::EmptyReleases
        | CompatibilityError::DuplicateRelease(_)
        | CompatibilityError::DuplicateHarness(_)
        | CompatibilityError::IncompleteEvidencePair { .. }
        | CompatibilityError::MissingEvidence { .. }
        | CompatibilityError::InvalidEvidenceTimestamp { .. }
        | CompatibilityError::InvalidUrl { .. }
        | CompatibilityError::InsecureUrl
        | CompatibilityError::ManifestTooLarge
        | CompatibilityError::ParseState(_)
        | CompatibilityError::UnsupportedStateSchema(_)
        | CompatibilityError::SerializeState(_) => {
            Diagnostic::general(DiagnosticReason::InvalidManifest)
        }
        CompatibilityError::BuildClient(_) | CompatibilityError::SystemClock(_) => {
            Diagnostic::general(DiagnosticReason::InternalInvariant)
        }
    }
}

fn bridge_diagnostic_classification(
    diagnostic: &BridgeDiagnostic,
) -> (FailureCategory, FailureStage, FailureCause, bool) {
    let retryable_http = diagnostic
        .http_status
        .is_some_and(|status| matches!(status, 502..=504));
    match diagnostic.reason {
        BridgeDiagnosticReason::UpstreamTransport => (
            FailureCategory::Bridge,
            FailureStage::HarnessExecution,
            FailureCause::Network,
            true,
        ),
        BridgeDiagnosticReason::UpstreamStatus => (
            FailureCategory::Bridge,
            FailureStage::HarnessExecution,
            FailureCause::HttpStatus,
            retryable_http,
        ),
        BridgeDiagnosticReason::InvalidUpstreamResponse => (
            FailureCategory::Bridge,
            FailureStage::HarnessExecution,
            FailureCause::InvalidResponse,
            true,
        ),
        BridgeDiagnosticReason::AuthenticationRejected => (
            FailureCategory::Bridge,
            FailureStage::HarnessExecution,
            FailureCause::InvalidConfiguration,
            false,
        ),
        BridgeDiagnosticReason::InvalidRequest
        | BridgeDiagnosticReason::ReasoningPolicyMismatch => (
            FailureCategory::Bridge,
            FailureStage::HarnessExecution,
            FailureCause::InvalidData,
            false,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser as _;
    use nan_harness_core::VersionStatus;
    use nan_harness_telemetry::consent::ReportConsent;
    use nan_harness_telemetry::consent::TelemetrySettingsStore;
    use nan_harness_telemetry::event::ErrorReport;
    use nan_harness_telemetry::redaction::sanitize;
    use std::collections::BTreeSet;

    #[cfg(unix)]
    #[test]
    fn known_harness_identity_skips_executable_discovery() {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let executable = directory.path().join("sentinel-harness");
        let marker = directory.path().join("sentinel-was-run");
        std::fs::write(
            &executable,
            format!(
                "#!/bin/sh\ntouch '{}'\nprintf 'pi 9.9.9\\n'\n",
                marker.display()
            ),
        )
        .expect("sentinel executable should be written");
        let mut permissions = std::fs::metadata(&executable)
            .expect("sentinel metadata should exist")
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&executable, permissions).expect("sentinel should be executable");
        let cli = Cli::try_parse_from([
            "nan-harness",
            "pi",
            "--executable",
            executable.to_str().expect("temporary path should be UTF-8"),
        ])
        .expect("Pi command should parse");
        let detected = DetectedHarness {
            kind: HarnessKind::Pi,
            executable: executable.to_string_lossy().into_owned(),
            detected_version: "pi 1.2.3".to_owned(),
            version_status: VersionStatus::Tested,
            capabilities: BTreeSet::new(),
        };

        let identity = telemetry_harness_identity(&cli, HarnessIdentitySource::Known(&detected))
            .expect("known identity should be retained");

        assert!(
            !marker.exists(),
            "known identity must not execute the harness"
        );
        assert_eq!(identity.kind(), TelemetryHarnessKind::Pi);
        assert_eq!(identity.version(), Some("1.2.3"));
        assert_eq!(
            identity.compatibility(),
            Some(TelemetryCompatibilityStatus::Tested)
        );

        let discovered = telemetry_harness_identity(&cli, HarnessIdentitySource::Detect)
            .expect("fallback discovery should retain harness identity");
        assert!(marker.exists(), "fallback must still discover the harness");
        assert_eq!(discovered.kind(), TelemetryHarnessKind::Pi);
        assert_eq!(discovered.version(), Some("9.9.9"));
    }

    #[test]
    fn every_bridge_diagnostic_satisfies_the_telemetry_contract() {
        let cli =
            Cli::try_parse_from(["nan-harness", "codex"]).expect("Codex command should parse");
        let diagnostics = [
            diagnostic(
                "NH-BRIDGE-101",
                BridgeDiagnosticReason::AuthenticationRejected,
                None,
            ),
            diagnostic(
                "NH-BRIDGE-102",
                BridgeDiagnosticReason::InvalidRequest,
                None,
            ),
            diagnostic(
                "NH-BRIDGE-103",
                BridgeDiagnosticReason::UpstreamTransport,
                None,
            ),
            diagnostic(
                "NH-BRIDGE-104",
                BridgeDiagnosticReason::UpstreamStatus,
                Some(503),
            ),
            diagnostic(
                "NH-BRIDGE-105",
                BridgeDiagnosticReason::InvalidUpstreamResponse,
                None,
            ),
        ];
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let installation_id = TelemetrySettingsStore::new(directory.path())
            .diagnostic_installation_id()
            .expect("diagnostic installation ID should exist");

        for context in bridge_diagnostic_contexts(&diagnostics, &cli, true) {
            let report =
                ErrorReport::new(context, ReportConsent::one_time(), installation_id.clone())
                    .expect("report should build");
            sanitize(report).expect("bridge diagnostic should satisfy telemetry contract");
        }
    }

    #[test]
    fn reasoning_policy_failures_keep_only_actionable_typed_context() {
        let cli =
            Cli::try_parse_from(["nan-harness", "codex"]).expect("Codex command should parse");
        let diagnostic = BridgeDiagnostic {
            code: "NH-BRIDGE-102",
            reason: BridgeDiagnosticReason::ReasoningPolicyMismatch,
            http_status: None,
            endpoint: RuntimeBridgeEndpoint::Responses,
            model_id: Some("mimo-v2.5".to_owned()),
            requested_reasoning: Some(RuntimeReasoningRequest::None),
            model_policy: Some(RuntimeModelPolicy::AlwaysOn),
        };
        let context = bridge_diagnostic_contexts(&[diagnostic], &cli, true)
            .pop()
            .expect("bridge context should exist");
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let installation_id = TelemetrySettingsStore::new(directory.path())
            .diagnostic_installation_id()
            .expect("diagnostic installation ID should exist");
        let report = ErrorReport::new(context, ReportConsent::one_time(), installation_id)
            .expect("report should build");
        let value = serde_json::to_value(sanitize(report).expect("report should be safe"))
            .expect("report should serialize");

        assert_eq!(value["diagnostic"]["reason"], "reasoning-policy-mismatch");
        assert_eq!(value["diagnostic"]["details"]["modelId"], "mimo-v2.5");
        assert_eq!(value["diagnostic"]["details"]["requestedReasoning"], "none");
        assert_eq!(value["diagnostic"]["details"]["modelPolicy"], "always-on");
        let serialized = value.to_string();
        assert!(!serialized.contains("incompatible with model policy"));
        assert!(!serialized.contains("invalid bridge request"));
    }

    fn diagnostic(
        code: &'static str,
        reason: BridgeDiagnosticReason,
        http_status: Option<u16>,
    ) -> BridgeDiagnostic {
        BridgeDiagnostic {
            code,
            reason,
            http_status,
            endpoint: RuntimeBridgeEndpoint::Responses,
            model_id: None,
            requested_reasoning: None,
            model_policy: None,
        }
    }
}
