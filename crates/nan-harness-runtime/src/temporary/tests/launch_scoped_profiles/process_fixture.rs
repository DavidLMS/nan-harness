use super::materialize_profile;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const CHILD: &str = "NAN_HARNESS_TEST_KILLED_SCOPED_FILE_OWNER";
const HOME: &str = "NAN_HARNESS_TEST_KILLED_SCOPED_FILE_HOME";
const READY: &str = "NAN_HARNESS_TEST_KILLED_SCOPED_FILE_READY";

pub(super) fn is_child() -> bool {
    std::env::var_os(CHILD).is_some()
}

pub(super) fn run_child() -> ! {
    let home = PathBuf::from(std::env::var_os(HOME).expect("child home should be provided"));
    let ready =
        PathBuf::from(std::env::var_os(READY).expect("child ready path should be provided"));
    let _workspace = materialize_profile(&home, "launch_01killedowner");
    fs::write(ready, "ready").expect("child readiness marker should be written");
    loop {
        thread::park();
    }
}

pub(super) struct KillOnDrop {
    child: Child,
}

impl KillOnDrop {
    pub(super) fn spawn(home: &Path, ready: &Path) -> Self {
        let child = Command::new(std::env::current_exe().expect("test executable should resolve"))
            .arg("killed_owner_artifacts_are_reclaimed_by_a_later_launch")
            .arg("--test-threads=1")
            .env(CHILD, "1")
            .env(HOME, home)
            .env(READY, ready)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("killed-owner fixture should start");
        Self { child }
    }

    pub(super) fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    pub(super) fn terminate(&mut self) {
        match self.child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) | Err(_) => {
                let _ = self.child.kill();
            }
        }
        let _ = self.child.wait();
    }
}

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        self.terminate();
    }
}

pub(super) fn wait_until_ready(child: &mut Child, ready: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if ready.exists() {
            return;
        }
        if let Some(status) = child
            .try_wait()
            .expect("fixture status should remain readable")
        {
            panic!("killed-owner fixture exited before readiness: {status}");
        }
        assert!(Instant::now() < deadline, "fixture readiness timed out");
        thread::sleep(Duration::from_millis(10));
    }
}
