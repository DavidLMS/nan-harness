use super::{CaptureMode, ProcessEvent, ScanState, TerminalCommand, TerminalError};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PARENT_FIXTURE: &str = "terminal::windows_tests::parent_fixture";
const LEAF_FIXTURE: &str = "terminal::windows_tests::leaf_fixture";
const QUIET_PARENT_FIXTURE: &str = "terminal::windows_tests::quiet_parent_fixture";
const EMPTY_FIXTURE: &str = "terminal::windows_tests::empty_fixture";

#[test]
#[ignore = "subprocess fixture"]
fn leaf_fixture() {
    std::thread::sleep(Duration::from_secs(30));
}

#[test]
#[ignore = "subprocess fixture"]
fn empty_fixture() {}

#[test]
#[ignore = "subprocess fixture"]
fn quiet_parent_fixture() {
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args(["--ignored", "--exact", LEAF_FIXTURE])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("quiet owned descendant");
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

#[test]
#[ignore = "subprocess fixture"]
fn parent_fixture() {
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args(["--ignored", "--exact", LEAF_FIXTURE, "--nocapture"])
        .spawn()
        .expect("descendant should start with inherited output");
    // Reap while this fixture is alive; returning from its main test process deliberately
    // interrupts the waiter and leaves the descendant for the outer job owner to clean up.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    println!("TERMINAL_PARENT_OK");
    if std::env::var_os("NAN_TEST_PARENT_WAIT").is_some() {
        std::thread::sleep(Duration::from_secs(30));
    }
}

#[tokio::test]
async fn exited_parent_releases_descendant_output_without_hanging() {
    let workspace = tempfile::tempdir().expect("workspace");
    let started = Instant::now();
    let output = TerminalCommand::new(
        std::env::current_exe().expect("test executable"),
        workspace.path(),
    )
    .args(["--ignored", "--exact", PARENT_FIXTURE, "--nocapture"])
    .timeout(Duration::from_secs(10))
    .run()
    .await
    .expect("owned descendants must release captured output");
    assert!(output.status.success());
    assert!(output.stdout.contains("TERMINAL_PARENT_OK"));
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[tokio::test]
async fn timeout_closes_descendant_output_without_killing_unrelated_children() {
    let workspace = tempfile::tempdir().expect("workspace");
    let executable = std::env::current_exe().expect("test executable");
    let mut unrelated = Command::new(&executable)
        .args(["--ignored", "--exact", LEAF_FIXTURE])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("unrelated child");
    let started = Instant::now();
    let result = TerminalCommand::new(executable, workspace.path())
        .args(["--ignored", "--exact", PARENT_FIXTURE, "--nocapture"])
        .env("NAN_TEST_PARENT_WAIT", "1")
        .timeout(Duration::from_secs(2))
        .run()
        .await;
    let unrelated_running = unrelated.try_wait().expect("unrelated status").is_none();
    let _ = unrelated.kill();
    let _ = unrelated.wait();
    assert!(matches!(result, Err(TerminalError::Timeout { .. })));
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(unrelated_running);
}

#[tokio::test]
async fn diagnostic_detects_owned_descendant_after_root_exit_and_both_eofs() {
    let workspace = tempfile::tempdir().expect("workspace");
    let output = TerminalCommand::new(
        std::env::current_exe().expect("test executable"),
        workspace.path(),
    )
    .args(["--ignored", "--exact", QUIET_PARENT_FIXTURE])
    .observe_processes(true)
    .timeout(Duration::from_secs(10))
    .diagnose(CaptureMode::Pipe, None)
    .await;
    assert_eq!(output.event, ProcessEvent::Exited);
    assert!(output.status.expect("root status").success());
    assert!(output.stdout_eof && output.stderr_eof);
    assert_eq!(output.cleanup, None);
    assert_eq!(output.survivors_residual.state, ScanState::Available);
    assert!(output.survivors_residual.count >= 1);
    assert!(output.survivors_residual.names.is_empty());
}

#[tokio::test]
async fn retained_job_counts_descendant_after_root_exit_then_proves_cleanup() {
    let executable = std::env::current_exe().expect("test executable");
    let mut command = tokio::process::Command::new(&executable);
    command
        .args(["--ignored", "--exact", QUIET_PARENT_FIXTURE])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = super::spawn_owned(command).expect("owned parent");
    tokio::time::timeout(Duration::from_secs(10), async {
        // Before resuming from spawn, the root was already assigned to this Job. Whether it
        // is still running or has handed off to its child, at least one member must remain.
        assert!(child.active_process_count().expect("initial Job query") >= 1);
        assert!(
            super::child_wait(&mut child)
                .await
                .expect("root wait")
                .success()
        );
        assert!(child.active_process_count().expect("descendant Job query") >= 1);
        super::child_start_kill(&mut child).expect("owned tree termination");
        loop {
            if child.active_process_count().expect("cleanup Job query") == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("owned Job must drain inside the fixture budget");
}

#[tokio::test]
async fn retained_job_counts_a_live_root_without_descendants() {
    let mut command =
        tokio::process::Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["--ignored", "--exact", LEAF_FIXTURE])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = super::spawn_owned(command).expect("owned root");
    tokio::time::timeout(Duration::from_secs(10), async {
        assert_eq!(
            child.active_process_count().expect("live root Job query"),
            1
        );
        super::child_start_kill(&mut child).expect("owned root termination");
        super::child_wait(&mut child).await.expect("root wait");
        loop {
            if child.active_process_count().expect("cleanup Job query") == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("owned root must drain inside the fixture budget");
}

#[tokio::test]
async fn unrelated_live_child_does_not_prevent_an_available_empty_job() {
    let workspace = tempfile::tempdir().expect("workspace");
    let executable = std::env::current_exe().expect("test executable");
    let mut unrelated = Command::new(&executable)
        .args(["--ignored", "--exact", LEAF_FIXTURE])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("unrelated child");
    let output = TerminalCommand::new(executable, workspace.path())
        .args(["--ignored", "--exact", EMPTY_FIXTURE])
        .observe_processes(true)
        .timeout(Duration::from_secs(10))
        .diagnose(CaptureMode::Pipe, None)
        .await;
    let unrelated_running = unrelated.try_wait().expect("unrelated status").is_none();
    let _ = unrelated.kill();
    let _ = unrelated.wait();
    assert!(
        unrelated_running,
        "ownership must not include an unrelated child"
    );
    assert_eq!(output.event, ProcessEvent::Exited);
    assert!(output.status.expect("root status").success());
    assert!(output.stdout_eof && output.stderr_eof);
    assert_eq!(output.survivors_residual.state, ScanState::Available);
    assert_eq!(output.survivors_residual.count, 0);
    assert!(output.survivors_residual.names.is_empty());
}
