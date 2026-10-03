//! Observed wrapper operations do not establish absence of job members.
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum TerminateResult {
    NotAttempted,
    Issued,
    Failed,
}

pub(super) struct StopObservation {
    wrapper_present: bool,
    launcher_handle_available: bool,
    terminate_result: TerminateResult,
    job_closed: bool,
}

impl StopObservation {
    pub(super) fn new() -> Self {
        Self {
            wrapper_present: false,
            launcher_handle_available: false,
            terminate_result: TerminateResult::NotAttempted,
            job_closed: false,
        }
    }

    pub(super) fn attempted(&mut self, wrapper: bool, handle: bool, succeeded: bool) {
        self.wrapper_present = wrapper;
        self.launcher_handle_available = handle;
        self.terminate_result = if !wrapper {
            TerminateResult::NotAttempted
        } else if succeeded {
            TerminateResult::Issued
        } else {
            TerminateResult::Failed
        };
    }

    pub(super) fn closed(&mut self) {
        self.job_closed = true;
    }

    fn value(&self) -> serde_json::Value {
        serde_json::json!({"schemaVersion":1,"mechanism":"windows-owned-stop","diagnosticsOnly":true,
            "wrapperPresent":self.wrapper_present,"launcherHandleAvailable":self.launcher_handle_available,
            "terminateResult":self.terminate_result,"jobClosed":self.job_closed})
    }

    #[cfg(windows)]
    pub(super) fn record(&self) {
        use std::io::Write as _;
        use std::os::windows::fs::MetadataExt as _;
        use std::path::PathBuf;
        if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
            || std::env::var("RUNNER_OS").as_deref() != Ok("Windows")
            || std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() != Ok("startup-baseline")
            || std::env::var("NANH_DESKTOP_RENDERER_APP").as_deref() != Ok("claude-desktop")
            || std::env::var("NANH_CLAUDE_WINDOWS_PROFILE_POLICY").as_deref() != Ok("private-env")
        {
            return;
        }
        let Some(directory) =
            std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS").map(PathBuf::from)
        else {
            return;
        };
        let Ok(metadata) = std::fs::symlink_metadata(&directory) else {
            return;
        };
        if !metadata.is_dir() || metadata.is_symlink() || !directory.is_absolute() {
            return;
        }
        // Windows canonicalization adds a verbatim prefix. Reject reparse
        // components first, then use the canonical directory for the write.
        let mut ancestor = PathBuf::new();
        for component in directory.components() {
            ancestor.push(component.as_os_str());
            if !ancestor.is_absolute() {
                continue;
            }
            let Ok(metadata) = std::fs::symlink_metadata(&ancestor) else {
                return;
            };
            if metadata.file_attributes() & 0x400 != 0 {
                return;
            }
        }
        let Ok(directory) = directory.canonicalize() else {
            return;
        };
        if nan_harness_private_fs::restrict_path(
            &directory,
            nan_harness_private_fs::PrivatePathKind::Directory,
        )
        .is_err()
        {
            return;
        }
        let Ok(bytes) = serde_json::to_vec(&self.value()) else {
            return;
        };
        let mut nonce = [0_u8; 8];
        if getrandom::fill(&mut nonce).is_err() {
            return;
        }
        let path = directory.join(format!(
            "windows-owned-stop-{}.json",
            u64::from_le_bytes(nonce)
        ));
        if let Ok(mut file) = nan_harness_private_fs::open_private_new(&path) {
            let _ = file.write_all(&bytes).and_then(|()| file.sync_all());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_wrapper_never_claims_termination_or_member_absence() {
        let mut observation = StopObservation::new();
        observation.attempted(false, false, true);
        observation.closed();
        let value = observation.value();
        assert_eq!(value["terminateResult"], "not-attempted");
        assert_eq!(value["jobClosed"], true);
        assert_eq!(value.as_object().unwrap().len(), 7);
        assert!(value.get("membersAbsent").is_none());
    }
    #[test]
    fn issued_and_failed_are_exact_operation_outcomes() {
        let mut observation = StopObservation::new();
        observation.attempted(true, true, false);
        assert_eq!(observation.terminate_result, TerminateResult::Failed);
        assert!(!observation.job_closed);
        observation.attempted(true, false, true);
        assert_eq!(observation.terminate_result, TerminateResult::Issued);
        assert!(!observation.launcher_handle_available);
    }
}
