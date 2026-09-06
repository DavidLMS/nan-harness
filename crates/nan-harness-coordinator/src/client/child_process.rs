use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Names the file a child scenario writes once its assertions have all run.
const COMPLETION_MARKER_ENVIRONMENT: &str = "NAN_HARNESS_TEST_SCENARIO_MARKER";
const COMPLETION_POLL: Duration = Duration::from_millis(10);

/// Runs one `#[ignore]`d scenario of this test binary as an externally bounded child.
///
/// Two client behaviours cannot be observed inside the test process: the salt
/// publication wait sleeps synchronously until its own deadline, so no in-process
/// timer can interrupt it, and the managed-process gate reads environment state that
/// is global to the process. A child gives both a hard external boundary and an
/// environment that no other test can observe.
pub(super) struct ChildScenario {
    command: Command,
    marker: PathBuf,
}

impl ChildScenario {
    /// Prepares the child for `test_path`, the scenario's exact `module::test` path.
    pub(super) fn new(test_path: &str, marker: &Path) -> Self {
        let executable = std::env::current_exe().expect("the test executable should be known");
        let mut command = Command::new(executable);
        command
            .args(["--exact", test_path, "--ignored", "--test-threads=1"])
            .stdin(Stdio::null())
            .env(COMPLETION_MARKER_ENVIRONMENT, marker);
        Self {
            command,
            marker: marker.to_owned(),
        }
    }

    pub(super) fn with_env(&mut self, key: &str, value: impl AsRef<OsStr>) -> &mut Self {
        self.command.env(key, value);
        self
    }

    pub(super) fn without_env(&mut self, key: &str) -> &mut Self {
        self.command.env_remove(key);
        self
    }

    /// Runs the scenario, failing when it does not complete successfully within `budget`.
    pub(super) fn run(&mut self, budget: Duration) {
        let child = self
            .command
            .spawn()
            .expect("the child scenario should start");
        let mut child = ReapedChild(child);
        let deadline = Instant::now() + budget;
        let status = loop {
            if let Some(status) = child
                .0
                .try_wait()
                .expect("the child status should be readable")
            {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "the child scenario did not finish within {budget:?}"
            );
            std::thread::sleep(COMPLETION_POLL);
        };
        assert!(status.success(), "the child scenario failed with {status}");
        assert!(
            self.marker.exists(),
            "the child scenario did not run its assertions"
        );
    }
}

/// Records that a child scenario reached the end of its assertions.
pub(super) fn scenario_completed() {
    let marker = std::env::var_os(COMPLETION_MARKER_ENVIRONMENT)
        .expect("a child scenario only runs through the bounded harness");
    std::fs::write(PathBuf::from(marker), b"completed")
        .expect("the completion marker should be written");
}

/// Kills and reaps the child even when a parent assertion panics first.
struct ReapedChild(Child);

impl Drop for ReapedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
