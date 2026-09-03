use nan_harness_telemetry::consent::{ConsentMode, TelemetryPreference, TelemetrySettingsStore};
use nan_harness_telemetry::diagnostic::{Diagnostic, DiagnosticReason};
use nan_harness_telemetry::event::{
    ErrorReportContext, Failure, FailureCategory, FailureStage, HarnessIdentity, HarnessKind,
};
use nan_harness_telemetry::panic::PendingReportStore;
use nan_harness_telemetry::prompt::{
    ERROR_REPORT_PROMPT, PromptDecision, ask_to_send_error_report,
};
use nan_harness_telemetry::{
    DeliveryOutcome, ERROR_REPORT_PREPARATION_FAILED_MESSAGE, ERROR_REPORT_QUEUED_MESSAGE,
    ERROR_REPORT_SENT_MESSAGE, TelemetryReporter,
};

use crate::{FailingExporter, ReporterFixture, context, report};

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
    assert_eq!(
        disabled
            .installation_id()
            .expect("diagnostic ID should remain available")
            .as_str(),
        installation_id
    );
}

#[test]
fn telemetry_rewrites_restore_private_file_permissions() {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;

    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let settings = TelemetrySettingsStore::new(directory.path());
    std::fs::write(settings.path(), "{\"enabled\":false}\n")
        .expect("settings fixture should exist");
    #[cfg(windows)]
    nan_harness_test_support::windows_acl::make_permissive_file(settings.path())
        .expect("settings ACL should be made permissive");
    #[cfg(unix)]
    std::fs::set_permissions(settings.path(), std::fs::Permissions::from_mode(0o644))
        .expect("settings fixture should be permissive");
    settings
        .set(TelemetryPreference::Off)
        .expect("settings should be rewritten");
    #[cfg(unix)]
    {
        assert_eq!(
            std::fs::metadata(settings.path())
                .expect("settings metadata should exist")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    #[cfg(windows)]
    nan_harness_test_support::windows_acl::assert_private_file(settings.path())
        .expect("settings should have a private protected DACL");

    let pending = PendingReportStore::new(directory.path());
    pending
        .save(&report(false))
        .expect("pending report should be written");
    #[cfg(windows)]
    nan_harness_test_support::windows_acl::make_permissive_file(pending.path())
        .expect("pending report ACL should be made permissive");
    #[cfg(unix)]
    std::fs::set_permissions(pending.path(), std::fs::Permissions::from_mode(0o644))
        .expect("pending fixture should be permissive");
    pending
        .save(&report(false))
        .expect("pending report should be rewritten");
    #[cfg(unix)]
    {
        assert_eq!(
            std::fs::metadata(pending.path())
                .expect("pending metadata should exist")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    #[cfg(windows)]
    nan_harness_test_support::windows_acl::assert_private_file(pending.path())
        .expect("pending report should have a private protected DACL");
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
    assert_eq!(
        reports[0]["installationId"],
        fixture
            .settings
            .load()
            .expect("settings should load")
            .installation_id()
            .expect("diagnostic installation ID should persist")
            .as_str()
    );
}

#[tokio::test]
async fn rejected_reports_explain_that_nothing_was_sent() {
    let fixture = ReporterFixture::new(false);
    let context = ErrorReportContext::new(
        Failure::new(
            "NH-TEST-001",
            FailureCategory::Internal,
            FailureStage::Startup,
            false,
        ),
        true,
    )
    .with_diagnostic(Diagnostic::general(DiagnosticReason::InvalidConfiguration))
    .with_harness(HarnessIdentity::new(
        HarnessKind::ClaudeCode,
        Some("/Users/private/project".to_owned()),
    ));
    let mut input = std::io::Cursor::new(b"y\n");
    let mut output = Vec::new();

    let outcome = fixture
        .reporter
        .report(context, &mut input, &mut output)
        .await;

    assert_eq!(outcome, DeliveryOutcome::Failed);
    let output = String::from_utf8(output).expect("status should be UTF-8");
    assert!(output.starts_with(ERROR_REPORT_PROMPT));
    assert!(output.contains(ERROR_REPORT_PREPARATION_FAILED_MESSAGE));
    assert!(fixture.exporter.reports().is_empty());
    assert!(!fixture.pending.path().exists());
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

#[test]
fn report_consent_modes_match_the_contract() {
    let automatic = nan_harness_telemetry::consent::ReportConsent::automatic();
    let one_time = nan_harness_telemetry::consent::ReportConsent::one_time();

    assert_eq!(automatic.mode(), ConsentMode::Automatic);
    assert!(automatic.telemetry_enabled());
    assert_eq!(one_time.mode(), ConsentMode::OneTime);
    assert!(!one_time.telemetry_enabled());
}
