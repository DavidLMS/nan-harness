use super::spawn_managed;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PROBE: &str = "NAN_HARNESS_TEST_EXITED_JOB";
const TEST: &str =
    "process::windows_job_tests::exited_job_reaping_stays_bounded_after_status_polling";

#[test]
fn exited_job_reaping_stays_bounded_after_status_polling() {
    if let Some(receipt) = std::env::var_os(PROBE) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let mut command = tokio::process::Command::new("cmd.exe");
            command.args(["/d", "/c", "exit", "0"]);
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let mut child = spawn_managed(command).unwrap();
            assert!(child.wait().await.unwrap().success());
            for _ in 0..32 {
                assert!(child.wait().await.unwrap().success());
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            std::fs::write(&receipt, "root-observed").unwrap();
            child.kill().await.unwrap();
            std::fs::write(&receipt, "reaped").unwrap();
        });
        return;
    }

    // A broken job wait can leave an uncancellable blocking task. Bound the
    // entire test process, including Tokio shutdown, with an owned child handle.
    let directory = tempfile::tempdir().unwrap();
    let receipt = directory.path().join("receipt");
    let mut probe = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture"])
        .env(PROBE, &receipt)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = probe.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            probe.kill().unwrap();
            probe.wait().unwrap();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let phase = std::fs::read_to_string(receipt).unwrap_or_default();
    assert!(
        status.is_some_and(|status| status.success()),
        "job probe failed at {phase}"
    );
    assert_eq!(phase, "reaped");
}
