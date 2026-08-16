#![cfg(unix)]

use nan_harness_core::launch_plan::TerminalMode;
use nan_harness_core::{LaunchPlan, SecretRef, SecretStore, SecretValue};
use nan_harness_runtime::{
    CancellationToken, ExecutionOutcome, RuntimeError, SignalKind, Supervisor,
};
use std::thread;
use std::time::Duration;

const DIRECT_PLAN: &str =
    include_str!("../../nan-harness-core/tests/fixtures/launch-plan.direct.json");
const BRIDGE_PLAN: &str =
    include_str!("../../nan-harness-core/tests/fixtures/launch-plan.bridge.json");

#[test]
fn supervisor_preserves_success_and_failure_exit_codes_and_cleans_up() {
    let success = execute_shell("exit 0", true, None);
    assert_eq!(success.outcome, ExecutionOutcome::Succeeded);
    assert_eq!(success.exit_code, 0);
    assert_removed(success.temporary_root);

    let failure = execute_shell("exit 7", true, None);
    assert_eq!(failure.outcome, ExecutionOutcome::Failed);
    assert_eq!(failure.exit_code, 7);
    assert_removed(failure.temporary_root);

    let normalized = execute_shell("exit 7", false, None);
    assert_eq!(normalized.exit_code, 1);
    assert_removed(normalized.temporary_root);
}

#[test]
fn supervisor_cancels_a_child_and_cleans_up() {
    let cancellation = CancellationToken::new();
    let trigger = cancellation.clone();
    let thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(40));
        trigger.cancel(SignalKind::Interrupt);
    });
    let report = execute_shell("while :; do :; done", true, Some(&cancellation));
    thread.join().expect("cancellation thread should finish");

    assert_eq!(
        report.outcome,
        ExecutionOutcome::Cancelled(SignalKind::Interrupt)
    );
    assert_eq!(report.exit_code, 130);
    assert_removed(report.temporary_root);
}

#[test]
fn supervisor_refuses_bridge_effects_until_phase_three() {
    let plan: LaunchPlan = serde_json::from_str(BRIDGE_PLAN).expect("valid bridge fixture");
    let error = Supervisor::new()
        .execute(&plan, &SecretStore::new(), &CancellationToken::new())
        .expect_err("bridge execution should not start in phase 2");

    assert!(matches!(error, RuntimeError::BridgeUnavailable));
    assert_eq!(error.code(), "NH-RUNTIME-002");
}

fn execute_shell(
    script: &str,
    preserve_exit_code: bool,
    cancellation: Option<&CancellationToken>,
) -> nan_harness_runtime::ExecutionReport {
    let working_directory = tempfile::tempdir().expect("working directory should exist");
    let mut plan: LaunchPlan = serde_json::from_str(DIRECT_PLAN).expect("valid direct fixture");
    "/bin/sh".clone_into(&mut plan.harness.executable);
    plan.process.arguments = vec!["-c".to_owned(), script.to_owned()];
    plan.process.working_directory = working_directory.path().to_string_lossy().into_owned();
    plan.process.terminal = TerminalMode::Captured;
    plan.process.preserve_exit_code = preserve_exit_code;
    let reference = SecretRef::new("nan_api_key").expect("valid secret reference");
    let mut secrets = SecretStore::new();
    secrets.insert(
        reference,
        SecretValue::new("test-key").expect("valid secret value"),
    );
    let default_cancellation = CancellationToken::new();

    Supervisor::new()
        .execute(
            &plan,
            &secrets,
            cancellation.unwrap_or(&default_cancellation),
        )
        .expect("direct execution should complete")
}

fn assert_removed(path: Option<std::path::PathBuf>) {
    let path = path.expect("fixture includes a temporary artifact");
    assert!(!path.exists());
}
