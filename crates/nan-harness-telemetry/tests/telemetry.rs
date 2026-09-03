#[path = "telemetry/consent.rs"]
mod consent;
#[path = "telemetry/contracts.rs"]
mod contracts;
#[path = "telemetry/exporter.rs"]
mod exporter;

use nan_harness_telemetry::TelemetryReporter;
use nan_harness_telemetry::consent::{InstallationId, TelemetryPreference, TelemetrySettingsStore};
use nan_harness_telemetry::event::{
    CompatibilityStatus, ErrorReport, ErrorReportContext, Failure, FailureCategory, FailureCause,
    FailureStage, HarnessIdentity, HarnessKind, OperationContext, OperationKind, StackFrame,
    Transport, UserGuidance,
};
use nan_harness_telemetry::glitchtip::{ErrorReportExporter, ExportError, ExportFuture};
use nan_harness_telemetry::panic::PendingReportStore;
use nan_harness_telemetry::redaction::{SanitizedErrorReport, sanitize};
use serde_json::Value;
use std::sync::{Arc, Mutex};

fn context(interactive: bool) -> ErrorReportContext {
    ErrorReportContext::new(
        Failure::new(
            "NH-TEST-001",
            FailureCategory::Bridge,
            FailureStage::RequestTranslation,
            false,
        )
        .with_cause(FailureCause::InvalidResponse),
        interactive,
    )
    .with_diagnostic(nan_harness_telemetry::diagnostic::Diagnostic::general(
        nan_harness_telemetry::diagnostic::DiagnosticReason::InvalidResponse,
    ))
    .with_harness(
        HarnessIdentity::new(HarnessKind::ClaudeCode, Some("2.1.233".to_owned()))
            .with_compatibility(CompatibilityStatus::Tested),
    )
    .with_transport(Transport::AnthropicBridge)
    .with_operation(OperationContext::new(OperationKind::HarnessRun))
    .with_stack(vec![StackFrame::new(
        "nan_harness_bridge::anthropic",
        "translate_tool_result",
        Some(true),
    )])
}

fn report(interactive: bool) -> SanitizedErrorReport {
    sanitize(
        ErrorReport::new(
            context(interactive),
            nan_harness_telemetry::consent::ReportConsent::one_time(),
            installation_id(),
        )
        .expect("report should build"),
    )
    .expect("report should satisfy the allowlist")
}

fn report_with_guidance(interactive: bool) -> SanitizedErrorReport {
    sanitize(
        ErrorReport::new(
            context(interactive).with_user_guidance(UserGuidance::reopen_terminal(true)),
            nan_harness_telemetry::consent::ReportConsent::one_time(),
            installation_id(),
        )
        .expect("report should build"),
    )
    .expect("report should satisfy the allowlist")
}

fn installation_id() -> InstallationId {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    TelemetrySettingsStore::new(directory.path())
        .diagnostic_installation_id()
        .expect("diagnostic installation ID should be generated")
}

#[derive(Debug, Clone, Default)]
struct RecordingExporter {
    reports: Arc<Mutex<Vec<Value>>>,
}

impl RecordingExporter {
    fn reports(&self) -> Vec<Value> {
        self.reports
            .lock()
            .expect("recording exporter lock should not be poisoned")
            .clone()
    }
}

impl ErrorReportExporter for RecordingExporter {
    fn export<'a>(&'a self, report: &'a SanitizedErrorReport) -> ExportFuture<'a> {
        Box::pin(async move {
            self.reports
                .lock()
                .expect("recording exporter lock should not be poisoned")
                .push(serde_json::to_value(report).expect("report should serialize"));
            Ok(())
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct FailingExporter;

impl ErrorReportExporter for FailingExporter {
    fn export<'a>(&'a self, _report: &'a SanitizedErrorReport) -> ExportFuture<'a> {
        Box::pin(async { Err(ExportError::UnsupportedDsn) })
    }
}

struct ReporterFixture {
    reporter: TelemetryReporter<RecordingExporter>,
    settings: TelemetrySettingsStore,
    pending: PendingReportStore,
    exporter: RecordingExporter,
    _directory: tempfile::TempDir,
}

impl ReporterFixture {
    fn new(enabled: bool) -> Self {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let settings = TelemetrySettingsStore::new(directory.path());
        if enabled {
            settings
                .set(TelemetryPreference::On)
                .expect("telemetry should enable");
        }
        let pending = PendingReportStore::new(directory.path());
        let exporter = RecordingExporter::default();
        let reporter = TelemetryReporter::new(settings.clone(), pending, Some(exporter.clone()));
        Self {
            reporter,
            settings,
            pending: PendingReportStore::new(directory.path()),
            exporter,
            _directory: directory,
        }
    }
}
