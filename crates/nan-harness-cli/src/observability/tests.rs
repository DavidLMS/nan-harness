use super::identity as identity_mapping;
use super::*;
use crate::app::Cli;
use clap::Parser as _;
use nan_harness_core::{DetectedHarness, HarnessKind, VersionStatus};
use nan_harness_runtime::{
    BridgeDiagnostic, BridgeDiagnosticReason, BridgeEndpoint as RuntimeBridgeEndpoint,
    BridgeModelPolicy as RuntimeModelPolicy, BridgeReasoningRequest as RuntimeReasoningRequest,
};
use nan_harness_telemetry::consent::ReportConsent;
use nan_harness_telemetry::consent::TelemetrySettingsStore;
use nan_harness_telemetry::event::{
    CompatibilityStatus as TelemetryCompatibilityStatus, ErrorReport,
    HarnessKind as TelemetryHarnessKind, OperationKind, Transport as TelemetryTransport,
};
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

    let identity =
        identity_mapping::telemetry_harness_identity(&cli, HarnessIdentitySource::Known(&detected))
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

    let discovered =
        identity_mapping::telemetry_harness_identity(&cli, HarnessIdentitySource::Detect)
            .expect("fallback discovery should retain harness identity");
    assert!(marker.exists(), "fallback must still discover the harness");
    assert_eq!(discovered.kind(), TelemetryHarnessKind::Pi);
    assert_eq!(discovered.version(), Some("9.9.9"));
}

#[test]
fn every_bridge_diagnostic_satisfies_the_telemetry_contract() {
    let cli = Cli::try_parse_from(["nan-harness", "codex"]).expect("Codex command should parse");
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
        diagnostic(
            "NH-BRIDGE-108",
            BridgeDiagnosticReason::CoordinatorQueueTimeout,
            None,
        ),
    ];
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let installation_id = TelemetrySettingsStore::new(directory.path())
        .diagnostic_installation_id()
        .expect("diagnostic installation ID should exist");

    for context in bridge_diagnostic_contexts(&diagnostics, &cli, true) {
        let report = ErrorReport::new(context, ReportConsent::one_time(), installation_id.clone())
            .expect("report should build");
        sanitize(report).expect("bridge diagnostic should satisfy telemetry contract");
    }
}

#[test]
fn reasoning_policy_failures_keep_only_actionable_typed_context() {
    let cli = Cli::try_parse_from(["nan-harness", "codex"]).expect("Codex command should parse");
    let diagnostic = BridgeDiagnostic {
        code: "NH-BRIDGE-102",
        reason: BridgeDiagnosticReason::ReasoningPolicyMismatch,
        http_status: None,
        endpoint: RuntimeBridgeEndpoint::Responses,
        model_id: Some("mimo-v2.5".to_owned()),
        requested_reasoning: Some(RuntimeReasoningRequest::None),
        model_policy: Some(RuntimeModelPolicy::AlwaysOn),
        timeout_phase: None,
        recovery_outcome: None,
        attempt: None,
        priority: None,
        cache_replay_detected: None,
        cache_bypass_attempted: None,
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

#[test]
fn zed_telemetry_is_limited_to_typed_identity_transport_and_operation() {
    let cli = Cli::try_parse_from([
        "nan-harness",
        "zed",
        "--model",
        "private-model-marker",
        "--dry-run",
        "/private/workspace-marker",
    ])
    .expect("Zed command should parse");
    let identity =
        identity_mapping::telemetry_harness_identity(&cli, HarnessIdentitySource::KindOnly)
            .expect("Zed identity should exist");

    assert_eq!(identity.kind(), TelemetryHarnessKind::ZedDesktop);
    assert_eq!(identity.version(), None);
    assert_eq!(identity.compatibility(), None);
    assert_eq!(
        context::telemetry_transport(&cli),
        Some(TelemetryTransport::DirectChat)
    );
    assert_eq!(
        context::telemetry_operation(&cli).kind(),
        OperationKind::HarnessDryRun
    );
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
        timeout_phase: None,
        recovery_outcome: None,
        attempt: None,
        priority: None,
        cache_replay_detected: None,
        cache_bypass_attempted: None,
    }
}

#[derive(Clone, Default)]
struct BudgetTestExporter(std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>);

impl nan_harness_telemetry::glitchtip::ErrorReportExporter for BudgetTestExporter {
    fn export<'a>(
        &'a self,
        report: &'a nan_harness_telemetry::redaction::SanitizedErrorReport,
    ) -> nan_harness_telemetry::glitchtip::ExportFuture<'a> {
        Box::pin(async move {
            self.0
                .lock()
                .expect("exporter lock")
                .push(serde_json::to_value(report).expect("report JSON"));
            Ok(())
        })
    }
}

#[tokio::test]
async fn enabled_telemetry_keeps_budget_stops_local_and_exports_independent_failures() {
    use nan_harness_telemetry::consent::TelemetryPreference;
    use nan_harness_telemetry::panic::PendingReportStore;
    use nan_harness_telemetry::{DeliveryOutcome, TelemetryReporter};

    let directory = tempfile::tempdir().expect("isolated telemetry settings");
    let settings = TelemetrySettingsStore::new(directory.path());
    settings
        .set(TelemetryPreference::On)
        .expect("enable telemetry");
    let exporter = BudgetTestExporter::default();
    let reporter = TelemetryReporter::new(
        settings,
        PendingReportStore::new(directory.path()),
        Some(exporter.clone()),
    );
    assert!(reporter.enabled());
    let cli = Cli::try_parse_from(["nanh", "codex"]).expect("CLI");
    let mut stops = Vec::new();
    for endpoint in [
        RuntimeBridgeEndpoint::Responses,
        RuntimeBridgeEndpoint::Messages,
        RuntimeBridgeEndpoint::FxGateway,
    ] {
        let mut stop = diagnostic(
            "NH-BRIDGE-109",
            BridgeDiagnosticReason::SessionBudgetReached {
                consumed: 1_018_613,
                limit: 1_000_000,
            },
            None,
        );
        stop.endpoint = endpoint;
        stops.push(stop);
    }
    let mut input = std::io::Cursor::new(Vec::<u8>::new());
    let mut output = Vec::new();
    assert_eq!(
        reporter
            .report_batch(
                bridge_diagnostic_contexts(&stops, &cli, true),
                &mut input,
                &mut output
            )
            .await,
        DeliveryOutcome::Deferred
    );
    assert!(exporter.0.lock().expect("exporter lock").is_empty());
    assert!(
        output.is_empty(),
        "no error report or consent prompt for a budget stop"
    );
    stops.push(diagnostic(
        "NH-BRIDGE-105",
        BridgeDiagnosticReason::InvalidUpstreamResponse,
        None,
    ));
    assert_eq!(
        reporter
            .report_batch(
                bridge_diagnostic_contexts(&stops, &cli, true),
                &mut input,
                &mut output
            )
            .await,
        DeliveryOutcome::Sent
    );
    let reports = exporter.0.lock().expect("exporter lock");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0]["failure"]["code"], "NH-BRIDGE-105");
    assert!(!reports[0].to_string().contains("NH-BRIDGE-109"));
}
