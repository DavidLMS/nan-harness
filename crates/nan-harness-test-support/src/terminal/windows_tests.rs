use super::{TerminalCommand, TerminalError};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PARENT_FIXTURE: &str = "terminal::windows_tests::parent_fixture";
const LEAF_FIXTURE: &str = "terminal::windows_tests::leaf_fixture";

#[test]
#[ignore = "subprocess fixture"]
fn leaf_fixture() {
    std::thread::sleep(Duration::from_secs(30));
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
