//! Deterministic checks that one budget covers sending the step script,
//! closing the script pipe and waiting for the remote step to finish.
//!
//! The fixtures are synthetic `/bin/sh` children with synthetic payloads: no
//! SSH, VM, provider or user process takes part, and nothing here inspects the
//! environment. They are Unix-only because they depend on `/bin/sh` and on
//! pipe semantics, so the same code path on Windows is simply unverified by
//! this module; nothing here qualifies any platform.

use super::{RemoteScriptAttemptError, classify_exit_status, send_script_and_wait};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;
use tokio::process::{Child, Command};

/// Far longer than any budget below, so a blocked fixture can only end because
/// the code under test ended it.
const BLOCKED_CHILD: &str = "exec sleep 30";
/// The same sleeper without a reader for the script pipe, which turns an
/// oversized send into a broken pipe instead of a block.
const UNREADABLE_PIPE_CHILD: &str = "exec sleep 30 0<&-";
/// Consumes the script and exits only at end of file.
const CONSUMING_CHILD: &str = "exec cat >/dev/null";

/// Generous for a blocked fixture yet far below its sleep.
const BLOCKED_BUDGET: Duration = Duration::from_millis(500);
/// Generous for a fixture that has real work to finish under instrumentation.
const COMPLETING_BUDGET: Duration = Duration::from_secs(5);
/// The outer bound for one case, including its own fixture cleanup.
const TEST_DEADLINE: Duration = Duration::from_secs(20);

/// Larger than a usual pipe buffer, so a child that never reads cannot absorb
/// the whole send.
const OVERSIZED_SCRIPT_BYTES: usize = 4 * 1024 * 1024;

fn oversized_script() -> String {
    "# synthetic payload\n".repeat(OVERSIZED_SCRIPT_BYTES / 20)
}

fn small_script() -> String {
    "# synthetic payload\n".to_owned()
}

fn spawn_fixture(shell_command: &str) -> Child {
    Command::new("/bin/sh")
        .args(["-c", shell_command])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("the synthetic fixture should start")
}

/// What one case observed, recorded before the fixture is cleaned up so that
/// a regression can never leave a sleeper behind an assertion failure.
struct Attempt {
    outcome: Result<ExitStatus, RemoteScriptAttemptError>,
    reaped: bool,
}

/// Runs the bounded attempt against a fixture, records whether the code under
/// test collected it, then makes sure the fixture is gone either way.
async fn attempt(shell_command: &str, script: &str, budget: Duration) -> Attempt {
    let mut child = spawn_fixture(shell_command);
    let stdin = child
        .stdin
        .take()
        .expect("the synthetic fixture should expose a script pipe");
    let outcome = tokio::time::timeout(
        TEST_DEADLINE,
        send_script_and_wait(&mut child, stdin, script, budget),
    )
    .await
    .expect("the bounded attempt should finish well within the test deadline");
    let reaped = matches!(child.try_wait(), Ok(Some(_)));
    let _ = child.start_kill();
    let _ = tokio::time::timeout(TEST_DEADLINE, child.wait()).await;
    Attempt { outcome, reaped }
}

fn failure(attempt: &Attempt) -> &RemoteScriptAttemptError {
    attempt
        .outcome
        .as_ref()
        .expect_err("this fixture should not let the attempt succeed")
}

async fn exit_status(shell_command: &str) -> ExitStatus {
    tokio::time::timeout(
        TEST_DEADLINE,
        Command::new("/bin/sh")
            .args(["-c", shell_command])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .status(),
    )
    .await
    .expect("the synthetic status fixture should finish within the test deadline")
    .expect("the synthetic status fixture should run")
}

#[tokio::test]
async fn a_send_blocked_by_a_child_that_never_reads_times_out() {
    let attempt = attempt(BLOCKED_CHILD, &oversized_script(), BLOCKED_BUDGET).await;

    let failure = failure(&attempt);
    assert_eq!(failure.detail, "remote step timed out");
    assert!(!failure.retryable, "a timed-out step must not be retried");
    assert!(
        attempt.reaped,
        "the attempt should terminate and collect its own child"
    );
}

#[tokio::test]
async fn a_wait_blocked_by_a_child_that_never_exits_times_out() {
    let attempt = attempt(BLOCKED_CHILD, &small_script(), BLOCKED_BUDGET).await;

    let failure = failure(&attempt);
    assert_eq!(failure.detail, "remote step timed out");
    assert!(!failure.retryable, "a timed-out step must not be retried");
    assert!(
        attempt.reaped,
        "the attempt should terminate and collect its own child"
    );
}

#[tokio::test]
async fn a_send_without_a_reader_is_a_retryable_failure() {
    let attempt = attempt(UNREADABLE_PIPE_CHILD, &oversized_script(), BLOCKED_BUDGET).await;

    let failure = failure(&attempt);
    assert!(
        failure
            .detail
            .starts_with("could not send the remote script: "),
        "an interrupted send should report itself, got {}",
        failure.detail
    );
    assert!(failure.retryable, "a failed send should be retried");
    assert!(
        attempt.reaped,
        "the attempt should terminate and collect its own child"
    );
}

#[tokio::test]
async fn a_consuming_child_succeeds_once_the_script_pipe_reaches_end_of_file() {
    let attempt = attempt(CONSUMING_CHILD, &small_script(), COMPLETING_BUDGET).await;

    let status = match &attempt.outcome {
        Ok(status) => status,
        Err(error) => panic!(
            "a child that consumes the script should exit successfully, got {}",
            error.detail
        ),
    };
    assert!(
        status.success(),
        "the fixture exits only at end of file, so the script pipe was closed"
    );
    assert!(attempt.reaped, "waiting for the child should collect it");
}

#[tokio::test]
async fn a_successful_status_completes_the_step() {
    assert!(classify_exit_status(exit_status("exit 0").await).is_ok());
}

#[tokio::test]
async fn the_ssh_transport_status_is_retryable() {
    let status = exit_status("exit 255").await;

    let error = classify_exit_status(status).expect_err("status 255 should fail the attempt");
    assert_eq!(error.detail, "SSH transport exited with status 255");
    assert!(
        error.retryable,
        "an SSH transport failure should be retried"
    );
}

#[tokio::test]
async fn another_nonzero_status_is_fatal() {
    let status = exit_status("exit 3").await;

    let error = classify_exit_status(status).expect_err("status 3 should fail the attempt");
    assert_eq!(error.detail, "remote step exited with status 3");
    assert!(
        !error.retryable,
        "a failing remote step must not be retried"
    );
}

#[tokio::test]
async fn a_signalled_status_is_fatal() {
    let status = exit_status("kill -TERM $$").await;

    assert_eq!(status.code(), None, "the fixture should die from a signal");
    let error = classify_exit_status(status).expect_err("a signal should fail the attempt");
    assert_eq!(error.detail, "remote step exited with a signal");
    assert!(!error.retryable, "a killed remote step must not be retried");
}
