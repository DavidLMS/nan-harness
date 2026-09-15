//! Windows regression: a detached helper must release the standard handles it inherited.
//!
//! The shared request coordinator is the helper that made `nanh codex ... | consumer` wait for
//! input after the launcher exited: Windows copies every inheritable handle into the detached
//! child, so the daemon kept the launcher's stdout and stderr open for the whole fifteen-minute
//! idle lifetime. This test runs the real entry point with a piped stdout and requires end of file
//! while the daemon is still serving.
#![cfg(windows)]

use std::io::Read as _;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Any surviving-writer defect reports end of file only when the daemon exits, so a short bound
/// separates a released pipe from a retained one without slowing the suite.
const HOLD: Duration = Duration::from_secs(3);
const CONFIG_DIRECTORY_ENVIRONMENT: &str = "NAN_HARNESS_CONFIG_DIR";

#[test]
fn coordinator_daemon_releases_the_launcher_standard_handles() {
    let directory = tempfile::tempdir().expect("isolated configuration directory");
    let mut child = Command::new(env!("CARGO_BIN_EXE_nan-harness"))
        .arg("__coordinator")
        .env(CONFIG_DIRECTORY_ENVIRONMENT, directory.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the coordinator daemon should start");
    let started = Instant::now();
    let mut stdout = child.stdout.take().expect("daemon stdout pipe");
    let mut buffer = [0_u8; 64];
    let read = stdout.read(&mut buffer);

    let still_serving = child.try_wait().map(|status| status.is_none());
    let failure = match (read, still_serving) {
        (Ok(0), Ok(true)) if started.elapsed() < HOLD => None,
        (Ok(0), Ok(true)) => Some(format!(
            "the daemon kept the launcher's stdout open for {:?}",
            started.elapsed()
        )),
        (Ok(0), Ok(false)) => Some("the daemon exited before releasing the pipe".to_owned()),
        (Ok(0), Err(error)) => Some(format!("could not query the daemon: {error}")),
        (Ok(_), _) => Some("the daemon wrote to the launcher's stdout".to_owned()),
        (Err(error), _) => Some(format!("reading the daemon stdout failed: {error}")),
    };
    if let Some(message) = failure {
        let stderr = collect_diagnostics(&mut child);
        panic!("{message}; daemon stderr: {stderr}");
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Kills a failed fixture and returns its bounded stderr for the failure message.
fn collect_diagnostics(child: &mut Child) -> String {
    let _ = child.kill();
    let mut text = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut text);
    }
    let _ = child.wait();
    text.trim().to_owned()
}
