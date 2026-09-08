#[cfg(unix)]
use super::spawn_with_retry;
use super::{ProbeError, run_bounded};
#[cfg(unix)]
use std::io;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

fn fixture() -> PathBuf {
    static FIXTURE: OnceLock<tempfile::TempDir> = OnceLock::new();
    FIXTURE
        .get_or_init(|| {
            let directory = tempfile::tempdir().expect("fixture directory");
            let status = Command::new("rustc")
                .args(["--edition=2024", "tests/fixtures/probe.rs", "-o"])
                .arg(
                    directory
                        .path()
                        .join(format!("probe{}", std::env::consts::EXE_SUFFIX)),
                )
                .status()
                .expect("compile native fixture");
            assert!(status.success());
            // Complete the platform's first executable load before short-budget concurrent probes.
            assert!(
                Command::new(
                    directory
                        .path()
                        .join(format!("probe{}", std::env::consts::EXE_SUFFIX))
                )
                .arg("success")
                .output()
                .unwrap()
                .status
                .success()
            );
            directory
        })
        .path()
        .join(format!("probe{}", std::env::consts::EXE_SUFFIX))
}

fn probe(arguments: &[&str], timeout: Duration, limit: usize) -> Result<Output, ProbeError> {
    let executable = fixture();
    let arguments: Vec<String> = arguments.iter().map(ToString::to_string).collect();
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let _ = sender.send(run_bounded(&executable, &arguments, timeout, limit));
    });
    let result = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("outer probe deadline");
    worker.join().expect("probe thread");
    result
}

#[test]
fn native_probe_preserves_streams_exit_status_and_closed_stdin() {
    let timeout = Duration::from_secs(2);
    let success = probe(&["success"], timeout, 1024).unwrap();
    assert!(success.status.success());
    assert_eq!(success.stdout, b"codex 0.153.4\n");
    assert!(success.stderr.is_empty());
    let stderr = probe(&["stderr"], timeout, 1024).unwrap();
    assert!(stderr.stdout.is_empty());
    assert_eq!(stderr.stderr, b"codex 0.153.4\n");
    assert_eq!(
        probe(&["nonzero"], timeout, 1024).unwrap().status.code(),
        Some(17)
    );
    assert_eq!(
        probe(&["stdin"], timeout, 1024).unwrap().stdout,
        b"closed\n"
    );
}

#[test]
fn native_probe_bounds_stdout_stderr_and_combined_output() {
    for mode in ["stdout-flood", "stderr-flood", "combined"] {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("pid");
        let result = probe(
            &[mode, marker.to_str().unwrap()],
            Duration::from_secs(2),
            super::OUTPUT_LIMIT,
        );
        assert!(
            matches!(result, Err(ProbeError::OutputLimit)),
            "{mode}: {result:?}"
        );
        assert_stopped(&marker);
    }
    assert!(
        probe(
            &["success"],
            Duration::from_secs(2),
            b"codex 0.153.4\n".len()
        )
        .is_ok()
    );
    assert!(matches!(
        probe(
            &["success"],
            Duration::from_secs(2),
            b"codex 0.153.4\n".len() - 1
        ),
        Err(ProbeError::OutputLimit)
    ));
}

#[test]
fn native_probe_times_out_and_reaps_sleeping_child() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("pid");
    assert!(matches!(
        probe(
            &["sleep", marker.to_str().unwrap()],
            Duration::from_secs(1),
            1024
        ),
        Err(ProbeError::Timeout)
    ));
    assert_stopped(&marker);
}

#[test]
fn native_probe_terminates_descendant_retaining_pipes() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("pid");
    assert!(matches!(
        probe(
            &["retain", marker.to_str().unwrap()],
            Duration::from_secs(1),
            1024
        ),
        Err(ProbeError::Timeout)
    ));
    assert_stopped(&marker);
    assert_stopped(&directory.path().join("pid.parent"));
}

#[test]
fn native_probe_cleanup_preserves_unrelated_process() {
    let mut unrelated = Command::new(fixture()).arg("sleep").spawn().unwrap();
    let result = probe(&["sleep"], Duration::from_secs(1), 1024);
    let still_running = unrelated.try_wait().unwrap().is_none();
    unrelated.kill().unwrap();
    unrelated.wait().unwrap();
    assert!(matches!(result, Err(ProbeError::Timeout)));
    assert!(
        still_running,
        "probe cleanup must only signal its own process group or job"
    );
}

fn assert_stopped(marker: &std::path::Path) {
    let pid: u32 = std::fs::read_to_string(marker)
        .expect("child started")
        .parse()
        .unwrap();
    #[cfg(unix)]
    {
        use nix::{sys::signal::kill, unistd::Pid};
        let pid = Pid::from_raw(i32::try_from(pid).unwrap());
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if kill(pid, None) == Err(nix::errno::Errno::ESRCH) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "probe child {pid} survived cleanup"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[cfg(windows)]
    {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let output = Command::new("tasklist")
                .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
                .output()
                .unwrap();
            assert!(output.status.success());
            if !String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\"")) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "probe child {pid} survived cleanup"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn probe_cleanup_preserves_errors_when_group_absence_is_not_proven() {
    let mut child = super::child::ProbeChild::spawn(&fixture(), &["sleep"]).unwrap();
    let live_error = child.confirm_termination(Err(io::ErrorKind::PermissionDenied.into()));
    let terminated = child.terminate();
    tokio::time::timeout(Duration::from_secs(2), child.reap())
        .await
        .unwrap()
        .unwrap();
    child.confirm_termination(terminated).unwrap();
    assert_eq!(
        live_error.unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert!(
        child
            .confirm_termination(Err(io::ErrorKind::PermissionDenied.into()))
            .is_ok()
    );
    assert_eq!(
        child
            .confirm_termination(Err(io::ErrorKind::Other.into()))
            .unwrap_err()
            .kind(),
        io::ErrorKind::Other
    );
}

#[cfg(unix)]
#[tokio::test]
async fn probe_busy_retries_share_deadline_and_stop_after_three_attempts() {
    let mut attempts = 0;
    let result = spawn_with_retry(Instant::now() + Duration::from_secs(1), || {
        attempts += 1;
        if attempts < 3 {
            Err(io::Error::from_raw_os_error(nix::libc::ETXTBSY))
        } else {
            Ok(42)
        }
    })
    .await;
    assert_eq!(result.unwrap(), 42);
    assert_eq!(attempts, 3);
    attempts = 0;
    let result: Result<(), _> = spawn_with_retry(Instant::now() + Duration::from_secs(1), || {
        attempts += 1;
        Err(io::Error::from_raw_os_error(nix::libc::ETXTBSY))
    })
    .await;
    assert!(matches!(result, Err(ProbeError::Io(_))));
    assert_eq!(attempts, 3);
    attempts = 0;
    let result: Result<(), _> = spawn_with_retry(Instant::now() + Duration::from_millis(1), || {
        attempts += 1;
        Err(io::Error::from_raw_os_error(nix::libc::ETXTBSY))
    })
    .await;
    assert!(matches!(result, Err(ProbeError::Timeout)));
    assert_eq!(attempts, 1);
    attempts = 0;
    let result: Result<(), _> = spawn_with_retry(Instant::now() + Duration::from_secs(1), || {
        attempts += 1;
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "synthetic"))
    })
    .await;
    assert!(matches!(result, Err(ProbeError::Io(_))));
    assert_eq!(attempts, 1);
}
