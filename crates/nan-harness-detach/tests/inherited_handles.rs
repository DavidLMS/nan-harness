//! Regression for the handle inheritance of a detached helper.
//!
//! A launcher that pipes its own standard output must reach end of file for its reader when it
//! exits, even while a helper it started keeps running. On Windows the helper receives a copy of the
//! launcher's standard handles unless the launcher withholds them, which is what
//! [`nan_harness_detach::without_inherited_standard_handles`] does. The first test observes that
//! Windows behaviour directly, so the guard cannot be removed silently; the second test observes the
//! guarantee.
use std::io::{BufRead as _, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

/// Selects how the inner launcher starts its helper.
const MODE: &str = "NAN_HARNESS_DETACH_TEST_MODE";
/// Printed by the inner launcher on its piped standard output.
const HELPER_MARKER: &str = "HOLDER-PID ";
/// The exact inner test name the launcher run selects.
const LAUNCHER_TEST: &str = "detached_helper_launcher";
/// Bound for observing the launcher's end of file.
const EOF_BUDGET: Duration = Duration::from_secs(6);
/// How long the helper outlives its launcher.
const HELPER_LIFETIME: Duration = Duration::from_secs(30);

/// Starts one helper through whichever start style `MODE` selected and reports its process id.
///
/// This is the launcher the outer tests observe: it owns a piped standard output and exits while the
/// helper keeps running.
#[test]
#[ignore = "the outer tests run this launcher with a piped standard output"]
fn detached_helper_launcher() {
    let mode = std::env::var(MODE).expect("the outer test selects the helper start style");
    let mut command = helper_command();
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = if mode == "detached" {
        nan_harness_detach::without_inherited_standard_handles(|| command.spawn())
    } else {
        command.spawn()
    }
    .expect("the helper should start");
    println!("{HELPER_MARKER}{}", child.id());
    // The helper is meant to outlive this launcher: waiting for it would defeat the observation, and
    // the handle belongs to a process that is about to exit.
    std::mem::forget(child);
}

/// Keeps the launcher's pipe open through an inherited handle copy unless the launcher withholds it.
#[test]
fn a_detached_helper_does_not_hold_a_launcher_pipe() {
    let launcher = Launcher::start("detached");
    launcher
        .wait_for_eof(EOF_BUDGET)
        .expect("a detached helper kept the launcher's standard output open");
    assert!(
        launcher.helper_is_running(),
        "the helper must outlive the launcher for this observation to mean anything"
    );
}

/// Documents the Windows behaviour the guard exists for: an inherited copy keeps the pipe open.
#[cfg(windows)]
#[test]
fn an_inherited_handle_copy_holds_a_launcher_pipe() {
    let launcher = Launcher::start("plain");
    let observed = launcher.wait_for_eof(EOF_BUDGET);
    assert!(
        matches!(observed, Err(RecvTimeoutError::Timeout)),
        "the helper was expected to inherit a copy of the launcher's standard handles"
    );
    assert!(
        launcher.helper_is_running(),
        "the inherited copy belongs to the helper, so the helper must still be running"
    );
    // Closing the copy is what releases the pipe, which confirms which process held it.
    terminate(launcher.helper);
    launcher
        .wait_for_eof(EOF_BUDGET)
        .expect("terminating the holder should release the launcher's standard output");
}

/// One launcher run with a piped standard output.
struct Launcher {
    child: Child,
    helper: u32,
    eof: Receiver<()>,
    reader: Option<thread::JoinHandle<()>>,
}

impl Launcher {
    /// Starts the launcher, waits for its helper report, and keeps reading its standard output.
    fn start(mode: &str) -> Self {
        let executable = std::env::current_exe().expect("the test executable should be known");
        let mut command = Command::new(executable);
        command
            .args([
                "--exact",
                LAUNCHER_TEST,
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(MODE, mode)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().expect("the launcher should start");
        let stdout = child
            .stdout
            .take()
            .expect("the launcher standard output should be piped");
        let (helper_sender, helper_receiver) = mpsc::channel();
        let (eof_sender, eof) = mpsc::channel();
        let reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                // The test harness writes its own progress on the same line, so the marker is
                // searched for rather than anchored.
                if let Some(pid) = line
                    .split_once(HELPER_MARKER)
                    .and_then(|(_, value)| value.trim().parse::<u32>().ok())
                {
                    let _ = helper_sender.send(pid);
                }
            }
            let _ = eof_sender.send(());
        });
        let Ok(helper) = helper_receiver.recv_timeout(EOF_BUDGET) else {
            let status = child
                .wait()
                .expect("the launcher status should be readable");
            panic!("the launcher did not report a helper and exited with {status}");
        };
        Self {
            child,
            helper,
            eof,
            reader: Some(reader),
        }
    }

    /// Waits for end of file on the launcher's standard output.
    fn wait_for_eof(&self, budget: Duration) -> Result<(), RecvTimeoutError> {
        self.eof.recv_timeout(budget)
    }

    /// Reports whether the helper is still running.
    fn helper_is_running(&self) -> bool {
        if cfg!(windows) {
            Command::new("tasklist")
                .args([
                    "/FI",
                    &format!("PID eq {}", self.helper),
                    "/NH",
                    "/FO",
                    "CSV",
                ])
                .output()
                .is_ok_and(|output| {
                    String::from_utf8_lossy(&output.stdout).contains(&self.helper.to_string())
                })
        } else {
            Command::new("kill")
                .args(["-0", &self.helper.to_string()])
                .status()
                .is_ok_and(|status| status.success())
        }
    }
}

impl Drop for Launcher {
    fn drop(&mut self) {
        terminate(self.helper);
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

/// Builds the long-lived helper that outlives the launcher.
fn helper_command() -> Command {
    let seconds = HELPER_LIFETIME.as_secs().to_string();
    if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args(["/d", "/c", "ping", "-n", &seconds, "127.0.0.1"]);
        command
    } else {
        let mut command = Command::new("sleep");
        command.arg(seconds);
        command
    }
}

/// Ends the helper process so the pipe copy it holds, if any, is closed.
fn terminate(helper: u32) {
    let pid = helper.to_string();
    let (program, arguments) = if cfg!(windows) {
        ("taskkill", vec!["/PID", pid.as_str(), "/T", "/F"])
    } else {
        ("kill", vec!["-9", pid.as_str()])
    };
    let _ = Command::new(program)
        .args(arguments)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}
