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
            check_descendants_and_cancellation(&receipt).await;
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
    let deadline = Instant::now() + Duration::from_secs(30);
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

async fn check_descendants_and_cancellation(receipt: &std::ffi::OsStr) {
    let descendant_receipt = std::path::PathBuf::from(receipt).with_extension("descendant");
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "process::windows_job_tests::owned_tree_fixture"])
        .env("NAN_HARNESS_TEST_JOB_TREE", "root")
        .env("NAN_HARNESS_TEST_JOB_RECEIPT", &descendant_receipt)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = spawn_managed(command).unwrap();
    assert!(child.wait().await.unwrap().success());
    assert!(descendant_receipt.is_file());
    assert!(
        tokio::time::timeout(Duration::from_millis(50), child.wait_for_descendants())
            .await
            .is_err(),
        "a live descendant must prevent an empty-Job receipt"
    );
    std::fs::write(receipt, "descendant-observed").unwrap();
    child.kill().await.unwrap();
    child.wait_for_descendants().await.unwrap();

    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "process::windows_job_tests::owned_tree_fixture"])
        .env("NAN_HARNESS_TEST_JOB_TREE", "descendant")
        .env("NAN_HARNESS_TEST_JOB_RECEIPT", &descendant_receipt)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = spawn_managed(command).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), child.wait())
            .await
            .is_err(),
        "waiting for a running root must remain cancellable"
    );
    std::fs::write(receipt, "wait-cancelled").unwrap();
    child.kill().await.unwrap();
    child.wait_for_descendants().await.unwrap();
}

#[tokio::test]
async fn owned_tree_fixture() {
    let Ok(mode) = std::env::var("NAN_HARNESS_TEST_JOB_TREE") else {
        return;
    };
    let receipt = std::env::var_os("NAN_HARNESS_TEST_JOB_RECEIPT").unwrap();
    if mode == "descendant" {
        std::fs::write(receipt, "started").unwrap();
        tokio::time::sleep(Duration::from_mins(1)).await;
        return;
    }
    // The parent deliberately exits first; the outer owned Job must retain
    // and reap this descendant after its original process handle is dropped.
    let _descendant = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "process::windows_job_tests::owned_tree_fixture"])
        .env("NAN_HARNESS_TEST_JOB_TREE", "descendant")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !std::path::Path::new(&receipt).is_file() {
        assert!(Instant::now() < deadline, "descendant did not start");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
