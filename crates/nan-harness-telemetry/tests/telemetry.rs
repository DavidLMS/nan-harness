use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use nan_harness_telemetry::consent::{ConsentMode, TelemetryPreference, TelemetrySettingsStore};
use nan_harness_telemetry::event::{
    CompatibilityStatus, ErrorReport, ErrorReportContext, Failure, FailureCategory, FailureCause,
    FailureStage, HarnessIdentity, HarnessKind, OperationContext, OperationKind, StackFrame,
    Transport,
};
use nan_harness_telemetry::glitchtip::{
    ErrorReportExporter, ExportError, ExportFuture, GlitchTipExporter,
};
use nan_harness_telemetry::panic::PendingReportStore;
use nan_harness_telemetry::prompt::{
    ERROR_REPORT_PROMPT, PromptDecision, ask_to_send_error_report,
};
use nan_harness_telemetry::redaction::{RedactionError, SanitizedErrorReport, sanitize};
use nan_harness_telemetry::{
    DeliveryOutcome, ERROR_REPORT_QUEUED_MESSAGE, ERROR_REPORT_SENT_MESSAGE, TelemetryReporter,
};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

#[test]
fn generated_reports_validate_against_the_published_contract() {
    let report = report(false);
    let value = serde_json::to_value(&report).expect("report should serialize");
    let schema: Value = serde_json::from_str(include_str!(
        "../../../tests/telemetry/error-report.schema.json"
    ))
    .expect("error report schema should parse");
    let validator = jsonschema::validator_for(&schema).expect("schema should compile");

    assert!(validator.is_valid(&value));
    assert_eq!(value["schemaVersion"], 2);
    assert_eq!(value["application"]["name"], "nan-harness");
}

#[test]
fn version_one_pending_reports_remain_readable_after_the_contract_upgrade() {
    let mut value = serde_json::to_value(report(false)).expect("report should serialize");
    value["schemaVersion"] = serde_json::json!(1);
    value
        .as_object_mut()
        .expect("report should be an object")
        .remove("operation");
    value["application"]
        .as_object_mut()
        .expect("application should be an object")
        .remove("buildCommit");
    value["failure"]
        .as_object_mut()
        .expect("failure should be an object")
        .remove("cause");
    value["runtime"]
        .as_object_mut()
        .expect("runtime should be an object")
        .remove("targetEnvironment");
    let report: ErrorReport = serde_json::from_value(value).expect("v1 report should deserialize");

    sanitize(report).expect("v1 report should remain valid");
}

#[test]
fn telemetry_settings_default_to_off_and_persist_only_explicit_changes() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let store = TelemetrySettingsStore::new(directory.path());

    assert!(
        !store
            .load()
            .expect("default settings should load")
            .enabled()
    );
    assert!(!store.path().exists());

    store
        .set(TelemetryPreference::On)
        .expect("on should persist");
    let enabled = store.load().expect("on should load");
    assert!(enabled.enabled());
    let installation_id = enabled
        .installation_id()
        .expect("enabled telemetry should have an installation ID")
        .as_str()
        .to_owned();

    store
        .set(TelemetryPreference::On)
        .expect("repeated on should persist");
    assert_eq!(
        store
            .load()
            .expect("repeated on should load")
            .installation_id()
            .expect("installation ID should remain available")
            .as_str(),
        installation_id
    );

    store
        .set(TelemetryPreference::Off)
        .expect("off should persist");
    let disabled = store.load().expect("off should load");
    assert!(!disabled.enabled());
    assert!(disabled.installation_id().is_none());
}

#[cfg(unix)]
#[test]
fn telemetry_rewrites_restore_private_file_permissions() {
    use std::os::unix::fs::PermissionsExt as _;

    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let settings = TelemetrySettingsStore::new(directory.path());
    std::fs::write(settings.path(), "{\"enabled\":false}\n")
        .expect("settings fixture should exist");
    std::fs::set_permissions(settings.path(), std::fs::Permissions::from_mode(0o644))
        .expect("settings fixture should be permissive");
    settings
        .set(TelemetryPreference::Off)
        .expect("settings should be rewritten");
    assert_eq!(
        std::fs::metadata(settings.path())
            .expect("settings metadata should exist")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let pending = PendingReportStore::new(directory.path());
    pending
        .save(&report(false))
        .expect("pending report should be written");
    std::fs::set_permissions(pending.path(), std::fs::Permissions::from_mode(0o644))
        .expect("pending fixture should be permissive");
    pending
        .save(&report(false))
        .expect("pending report should be rewritten");
    assert_eq!(
        std::fs::metadata(pending.path())
            .expect("pending metadata should exist")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn enabled_legacy_settings_gain_an_installation_id_on_first_use() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let store = TelemetrySettingsStore::new(directory.path());
    std::fs::write(store.path(), "{\"enabled\":true}\n")
        .expect("legacy settings should be written");

    let installation_id = store
        .active_installation_id()
        .expect("legacy settings should migrate")
        .expect("enabled telemetry should have an installation ID");
    let persisted = store.load().expect("migrated settings should load");

    assert_eq!(
        persisted
            .installation_id()
            .expect("migrated ID should persist"),
        &installation_id
    );
}

#[test]
fn disabled_telemetry_never_creates_an_installation_id() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let store = TelemetrySettingsStore::new(directory.path());

    assert!(
        store
            .active_installation_id()
            .expect("disabled settings should load")
            .is_none()
    );
    assert!(!store.path().exists());
}

#[test]
fn prompt_sends_only_for_an_explicit_y() {
    for (answer, expected) in [
        ("y\n", PromptDecision::Send),
        ("Y\n", PromptDecision::Send),
        ("n\n", PromptDecision::Decline),
        ("\n", PromptDecision::Decline),
        ("yes\n", PromptDecision::Decline),
    ] {
        let mut input = std::io::Cursor::new(answer.as_bytes());
        let mut output = Vec::new();
        let decision =
            ask_to_send_error_report(&mut input, &mut output).expect("prompt should complete");

        assert_eq!(decision, expected);
        assert_eq!(output, ERROR_REPORT_PROMPT.as_bytes());
    }
}

#[tokio::test]
async fn off_plus_y_sends_once_and_leaves_telemetry_off() {
    let fixture = ReporterFixture::new(false);
    let mut input = std::io::Cursor::new(b"y\n");
    let mut output = Vec::new();

    let outcome = fixture
        .reporter
        .report(context(true), &mut input, &mut output)
        .await;

    assert_eq!(outcome, DeliveryOutcome::Sent);
    let output = String::from_utf8(output).expect("status should be UTF-8");
    assert!(output.starts_with(ERROR_REPORT_PROMPT));
    assert!(output.contains(ERROR_REPORT_SENT_MESSAGE));
    assert!(
        !fixture
            .settings
            .load()
            .expect("settings should load")
            .enabled()
    );
    let reports = fixture.exporter.reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0]["consent"]["mode"], "one-time");
    assert_eq!(reports[0]["consent"]["telemetryEnabled"], false);
}

#[tokio::test]
async fn a_disabled_batch_prompts_once_and_sends_every_report() {
    let fixture = ReporterFixture::new(false);
    let mut input = std::io::Cursor::new(b"y\n");
    let mut output = Vec::new();

    let outcome = fixture
        .reporter
        .report_batch([context(true), context(true)], &mut input, &mut output)
        .await;

    assert_eq!(outcome, DeliveryOutcome::Sent);
    let output = String::from_utf8(output).expect("status should be UTF-8");
    assert_eq!(output.matches(ERROR_REPORT_PROMPT).count(), 1);
    assert_eq!(output.matches(ERROR_REPORT_SENT_MESSAGE).count(), 2);
    assert_eq!(fixture.exporter.reports().len(), 2);
}

#[tokio::test]
async fn declined_and_non_interactive_failures_make_no_export_request() {
    let fixture = ReporterFixture::new(false);
    let mut declined_input = std::io::Cursor::new(b"\n");
    let mut declined_output = Vec::new();
    let declined = fixture
        .reporter
        .report(context(true), &mut declined_input, &mut declined_output)
        .await;
    let mut non_interactive_input = std::io::Cursor::new(b"y\n");
    let mut non_interactive_output = Vec::new();
    let deferred = fixture
        .reporter
        .report(
            context(false),
            &mut non_interactive_input,
            &mut non_interactive_output,
        )
        .await;

    assert_eq!(declined, DeliveryOutcome::Declined);
    assert_eq!(deferred, DeliveryOutcome::Deferred);
    assert_eq!(declined_output, ERROR_REPORT_PROMPT.as_bytes());
    assert!(non_interactive_output.is_empty());
    assert!(fixture.exporter.reports().is_empty());
}

#[tokio::test]
async fn on_sends_sanitized_reports_automatically_without_prompting() {
    let fixture = ReporterFixture::new(true);
    let mut input = std::io::Cursor::new(Vec::<u8>::new());
    let mut output = Vec::new();

    let outcome = fixture
        .reporter
        .report(context(false), &mut input, &mut output)
        .await;

    assert_eq!(outcome, DeliveryOutcome::Sent);
    let output = String::from_utf8(output).expect("status should be UTF-8");
    assert!(output.starts_with(ERROR_REPORT_SENT_MESSAGE));
    let reports = fixture.exporter.reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0]["consent"]["mode"], "automatic");
    assert_eq!(reports[0]["consent"]["telemetryEnabled"], true);
}

#[test]
fn forbidden_metadata_is_rejected_before_an_exporter_can_receive_it() {
    let context = ErrorReportContext::new(
        Failure::new(
            "NH-TEST-001",
            FailureCategory::Internal,
            FailureStage::Startup,
            false,
        ),
        false,
    )
    .with_harness(HarnessIdentity::new(
        HarnessKind::ClaudeCode,
        Some("/Users/private/project".to_owned()),
    ));
    let report = ErrorReport::new(
        context,
        nan_harness_telemetry::consent::ReportConsent::automatic(),
    )
    .expect("report should build");

    assert_eq!(
        sanitize(report).expect_err("path-like metadata must be rejected"),
        RedactionError::ForbiddenValue {
            field: "harness.version"
        }
    );
}

#[tokio::test]
async fn glitchtip_receives_a_bounded_envelope_with_only_allowlisted_context() {
    let (address, request) = start_capture_server().await;
    let exporter = GlitchTipExporter::new(
        &format!("http://public_key@{address}/42"),
        Duration::from_secs(1),
    )
    .expect("test DSN should be valid");

    exporter
        .export(&report(false))
        .await
        .expect("envelope should be accepted");
    let captured = request.await.expect("request should be captured");
    let lines = captured
        .body
        .split(|byte| *byte == b'\n')
        .collect::<Vec<_>>();
    let envelope_header: Value =
        serde_json::from_slice(lines[0]).expect("envelope header should be JSON");
    let item_header: Value = serde_json::from_slice(lines[1]).expect("item header should be JSON");
    let event: Value = serde_json::from_slice(lines[2]).expect("event should be JSON");

    assert_eq!(captured.path, "/api/42/envelope/");
    assert_eq!(
        captured.content_type.as_deref(),
        Some("application/x-sentry-envelope")
    );
    assert!(
        captured
            .authorization
            .starts_with("Sentry sentry_version=7")
    );
    assert_eq!(envelope_header["event_id"], event["event_id"]);
    assert_eq!(item_header["type"], "event");
    assert_eq!(
        event["contexts"]["nan_harness"]["failure"]["code"],
        "NH-TEST-001"
    );
    let body = String::from_utf8(captured.body).expect("envelope should be UTF-8");
    assert!(!body.contains("NAN_API_KEY"));
    assert!(!body.contains("/Users/"));
    assert!(!body.contains("prompt"));
    assert!(!body.contains("tool output"));
    assert!(!body.contains("qwen3.6"));
    assert!(event["tags"].get("operation.model").is_none());
    assert!(
        event["contexts"]["nan_harness"]["operation"]
            .get("model")
            .is_none()
    );
}

#[tokio::test]
async fn exporter_timeout_is_best_effort_and_pending_consent_is_bounded() {
    let address = start_slow_server().await;
    let exporter = GlitchTipExporter::new(
        &format!("http://public_key@{address}/42"),
        Duration::from_millis(20),
    )
    .expect("test DSN should be valid");
    let result = exporter.export(&report(false)).await;

    assert!(matches!(result, Err(ExportError::Request(_))));

    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let pending = PendingReportStore::new(directory.path());
    pending
        .save(&report(false))
        .expect("pending report should save");
    assert!(
        pending
            .load()
            .expect("pending report should load")
            .is_some()
    );
    pending.delete().expect("pending report should delete");
    assert!(!pending.path().exists());
}

#[tokio::test]
async fn pending_reports_wait_for_consent_and_are_deleted_after_the_answer() {
    let fixture = ReporterFixture::new(false);
    fixture
        .pending
        .save(&report(false))
        .expect("pending report should save");
    let mut deferred_input = std::io::Cursor::new(b"y\n");
    let mut deferred_output = Vec::new();
    let deferred = fixture
        .reporter
        .process_pending(false, &mut deferred_input, &mut deferred_output)
        .await;

    assert_eq!(deferred, DeliveryOutcome::Deferred);
    assert!(deferred_output.is_empty());
    assert!(fixture.pending.path().exists());
    assert!(fixture.exporter.reports().is_empty());

    let mut accepted_input = std::io::Cursor::new(b"y\n");
    let mut accepted_output = Vec::new();
    let accepted = fixture
        .reporter
        .process_pending(true, &mut accepted_input, &mut accepted_output)
        .await;

    assert_eq!(accepted, DeliveryOutcome::Sent);
    let accepted_output = String::from_utf8(accepted_output).expect("status should be UTF-8");
    assert!(accepted_output.starts_with(ERROR_REPORT_PROMPT));
    assert!(accepted_output.contains(ERROR_REPORT_SENT_MESSAGE));
    assert!(!fixture.pending.path().exists());
    assert!(
        !fixture
            .settings
            .load()
            .expect("settings should load")
            .enabled()
    );
}

#[tokio::test]
async fn failed_automatic_delivery_is_queued_and_retained_for_retry() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let settings = TelemetrySettingsStore::new(directory.path());
    settings
        .set(TelemetryPreference::On)
        .expect("telemetry should enable");
    let pending = PendingReportStore::new(directory.path());
    let reporter = TelemetryReporter::new(settings, pending.clone(), Some(FailingExporter));
    let mut input = std::io::Cursor::new(Vec::<u8>::new());
    let mut output = Vec::new();

    let outcome = reporter
        .report(context(false), &mut input, &mut output)
        .await;

    assert_eq!(outcome, DeliveryOutcome::Failed);
    assert!(pending.path().exists());
    assert!(
        String::from_utf8(output)
            .expect("status should be UTF-8")
            .starts_with(ERROR_REPORT_QUEUED_MESSAGE)
    );

    let mut retry_input = std::io::Cursor::new(Vec::<u8>::new());
    let mut retry_output = Vec::new();
    let retry = reporter
        .process_pending(false, &mut retry_input, &mut retry_output)
        .await;

    assert_eq!(retry, DeliveryOutcome::Failed);
    assert!(retry_output.is_empty());
    assert!(pending.path().exists());
}

#[tokio::test]
#[ignore = "requires NAN_HARNESS_GLITCHTIP_DSN and creates a real GlitchTip issue"]
async fn live_glitchtip_accepts_the_sanitized_error_contract() {
    let dsn = std::env::var("NAN_HARNESS_GLITCHTIP_DSN")
        .expect("NAN_HARNESS_GLITCHTIP_DSN should be configured");
    let exporter = GlitchTipExporter::new(&dsn, Duration::from_secs(2))
        .expect("GlitchTip DSN should be valid");

    exporter
        .export(&report(false))
        .await
        .expect("GlitchTip should accept the sanitized report");
}

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
        )
        .expect("report should build"),
    )
    .expect("report should satisfy the allowlist")
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

#[derive(Debug)]
struct CapturedRequest {
    path: String,
    content_type: Option<String>,
    authorization: String,
    body: Vec<u8>,
}

async fn start_capture_server() -> (std::net::SocketAddr, oneshot::Receiver<CapturedRequest>) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("listener should bind");
    let address = listener.local_addr().expect("address should exist");
    let (sender, receiver) = oneshot::channel();
    let sender = Arc::new(Mutex::new(Some(sender)));
    let app = Router::new()
        .route("/api/42/envelope/", post(capture_request))
        .with_state(sender);
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("capture server should run");
    });
    (address, receiver)
}

async fn capture_request(
    State(sender): State<Arc<Mutex<Option<oneshot::Sender<CapturedRequest>>>>>,
    headers: HeaderMap,
    uri: axum::http::Uri,
    body: Bytes,
) -> StatusCode {
    let request = CapturedRequest {
        path: uri.path().to_owned(),
        content_type: headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned),
        authorization: headers
            .get("x-sentry-auth")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned(),
        body: body.to_vec(),
    };
    if let Some(sender) = sender
        .lock()
        .expect("capture sender lock should not be poisoned")
        .take()
    {
        let _ = sender.send(request);
    }
    StatusCode::OK
}

async fn start_slow_server() -> std::net::SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("listener should bind");
    let address = listener.local_addr().expect("address should exist");
    let app = Router::new().route(
        "/api/42/envelope/",
        post(|| async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            StatusCode::OK
        }),
    );
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("slow server should run");
    });
    address
}

#[test]
fn report_consent_modes_match_the_contract() {
    let automatic = nan_harness_telemetry::consent::ReportConsent::automatic();
    let one_time = nan_harness_telemetry::consent::ReportConsent::one_time();

    assert_eq!(automatic.mode(), ConsentMode::Automatic);
    assert!(automatic.telemetry_enabled());
    assert_eq!(one_time.mode(), ConsentMode::OneTime);
    assert!(!one_time.telemetry_enabled());
}
