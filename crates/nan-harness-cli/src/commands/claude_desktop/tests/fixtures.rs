use super::super::paths::DesktopPaths;
use super::super::{ClaudeDesktopError, DesktopProcess, read_json_object};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub(super) fn paths() -> (tempfile::TempDir, DesktopPaths) {
    let root = tempfile::tempdir().expect("temp root");
    let paths = DesktopPaths::new(
        &root.path().join("Claude"),
        &root.path().join("Claude-3p"),
        &root.path().join("state"),
    );
    (root, paths)
}

pub(super) struct FakeProcess {
    pub(super) profile: PathBuf,
    pub(super) available: AtomicBool,
    pub(super) running: AtomicBool,
    pub(super) terminated: AtomicBool,
    pub(super) force_terminated: AtomicBool,
    pub(super) terminated_while_gateway_active: AtomicBool,
    pub(super) fail_checks: AtomicBool,
    pub(super) transient_check_failures: AtomicUsize,
    pub(super) fail_terminate: AtomicBool,
    pub(super) fail_force_terminate: AtomicBool,
}

impl FakeProcess {
    pub(super) fn running(profile: PathBuf) -> Self {
        Self {
            profile,
            available: AtomicBool::new(true),
            running: AtomicBool::new(true),
            terminated: AtomicBool::new(false),
            force_terminated: AtomicBool::new(false),
            terminated_while_gateway_active: AtomicBool::new(false),
            fail_checks: AtomicBool::new(false),
            transient_check_failures: AtomicUsize::new(0),
            fail_terminate: AtomicBool::new(false),
            fail_force_terminate: AtomicBool::new(false),
        }
    }
}

impl DesktopProcess for FakeProcess {
    fn ensure_available(&self) -> Result<(), ClaudeDesktopError> {
        if self.available.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(ClaudeDesktopError::AppNotFound { platform: "test" })
        }
    }

    fn is_running(&self) -> Result<bool, ClaudeDesktopError> {
        let transient_failure = self
            .transient_check_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok();
        if self.fail_checks.load(Ordering::SeqCst) || transient_failure {
            return Err(ClaudeDesktopError::ProcessCheck(std::io::Error::other(
                "synthetic process check failure",
            )));
        }
        Ok(self.running.load(Ordering::SeqCst))
    }

    fn launch(&self) -> Result<(), ClaudeDesktopError> {
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn terminate(&self) -> Result<(), ClaudeDesktopError> {
        if self.fail_terminate.load(Ordering::SeqCst) {
            return Err(ClaudeDesktopError::Terminate(std::io::Error::other(
                "synthetic termination failure",
            )));
        }
        let gateway_active = read_json_object(&self.profile).is_ok_and(|profile| {
            profile.get("inferenceProvider").and_then(Value::as_str) == Some("gateway")
        });
        self.terminated_while_gateway_active
            .store(gateway_active, Ordering::SeqCst);
        self.terminated.store(true, Ordering::SeqCst);
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn force_terminate(&self) -> Result<(), ClaudeDesktopError> {
        self.force_terminated.store(true, Ordering::SeqCst);
        if self.fail_force_terminate.load(Ordering::SeqCst) {
            return Err(ClaudeDesktopError::Terminate(std::io::Error::other(
                "synthetic forced termination failure",
            )));
        }
        self.terminated.store(true, Ordering::SeqCst);
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }
}
