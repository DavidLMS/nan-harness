//! Private, offline native helper. No captured pixels or recognized text are persisted.

#[cfg(target_os = "macos")]
mod identity;
mod image;
mod ocr;
mod process;
mod window;

#[cfg(target_os = "macos")]
pub(crate) use crate::diagnostics::ClaudeIdentityObservation;
pub(crate) use image::prepare_ocr_image;
pub(crate) use ocr::Page;
pub(crate) use process::FailureCategory;
pub(crate) use window::{DisplayRelation, ForegroundRelation, GuardFailure, Snapshot, Window};

#[cfg(any(test, windows))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FitFailureStage {
    Request,
    IdentityRead,
    IdentityMismatch,
    ForegroundRead,
    ForegroundMismatch,
    MonitorRead,
    WorkareaRead,
    WindowRead,
    WorkareaInvalid,
    IdentityChanged,
    ForegroundChanged,
    Resize,
    PostconditionIdentityRead,
    PostconditionIdentityMismatch,
    PostconditionForegroundRead,
    PostconditionForegroundMismatch,
    PostconditionWindowRead,
    PostconditionGeometry,
}

#[cfg(any(test, windows))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FitFailure {
    pub(crate) stage: FitFailureStage,
    pub(crate) foreground_relation: Option<FitForegroundRelation>,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum FitForegroundRelation {
    SameProcessDifferentWindow,
    DifferentProcess,
    IdentityUnavailable,
}

#[cfg(any(test, windows))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FitWindowError {
    Transport(FailureCategory),
    Diagnostic(FitFailure),
}

#[cfg(any(test, windows))]
impl FitFailure {
    pub(crate) fn parse(output: &str) -> Result<Self, ()> {
        let mut fields = output.lines().flat_map(str::split_whitespace);
        if fields.next() != Some("FIT_FAILURE") {
            return Err(());
        }
        let stage = match fields.next() {
            Some("request") => FitFailureStage::Request,
            Some("identity-read") => FitFailureStage::IdentityRead,
            Some("identity-mismatch") => FitFailureStage::IdentityMismatch,
            Some("foreground-read") => FitFailureStage::ForegroundRead,
            Some("foreground-mismatch") => FitFailureStage::ForegroundMismatch,
            Some("monitor-read") => FitFailureStage::MonitorRead,
            Some("workarea-read") => FitFailureStage::WorkareaRead,
            Some("window-read") => FitFailureStage::WindowRead,
            Some("workarea-invalid") => FitFailureStage::WorkareaInvalid,
            Some("identity-changed") => FitFailureStage::IdentityChanged,
            Some("foreground-changed") => FitFailureStage::ForegroundChanged,
            Some("resize") => FitFailureStage::Resize,
            Some("postcondition-identity-read") => FitFailureStage::PostconditionIdentityRead,
            Some("postcondition-identity-mismatch") => {
                FitFailureStage::PostconditionIdentityMismatch
            }
            Some("postcondition-foreground-read") => FitFailureStage::PostconditionForegroundRead,
            Some("postcondition-foreground-mismatch") => {
                FitFailureStage::PostconditionForegroundMismatch
            }
            Some("postcondition-window-read") => FitFailureStage::PostconditionWindowRead,
            Some("postcondition-geometry") => FitFailureStage::PostconditionGeometry,
            _ => return Err(()),
        };
        let foreground_relation = match fields.next() {
            None => None,
            Some("same-process-different-window") => {
                Some(FitForegroundRelation::SameProcessDifferentWindow)
            }
            Some("different-process") => Some(FitForegroundRelation::DifferentProcess),
            Some("identity-unavailable") => Some(FitForegroundRelation::IdentityUnavailable),
            Some(_) => return Err(()),
        };
        if fields.next().is_some()
            || foreground_relation.is_some()
                && !matches!(
                    stage,
                    FitFailureStage::ForegroundRead
                        | FitFailureStage::ForegroundMismatch
                        | FitFailureStage::ForegroundChanged
                )
            || matches!(stage, FitFailureStage::ForegroundRead)
                && foreground_relation
                    .is_some_and(|relation| relation != FitForegroundRelation::IdentityUnavailable)
        {
            return Err(());
        }
        Ok(Self {
            stage,
            foreground_relation,
        })
    }
}

use crate::report::Reason;
use nan_harness_private_fs::{create_private_dir_all, open_private_new};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use xa11y::Screenshot;

pub(crate) struct Native {
    directory: tempfile::TempDir,
    executable: PathBuf,
}

#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ClaudeStoragePresence {
    #[serde(flatten)]
    normal: NormalClaudeStorage,
    #[serde(flatten)]
    third_party: ThirdPartyClaudeStorage,
}

#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NormalClaudeStorage {
    claude_local_state: bool,
    claude_preferences: bool,
}

#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThirdPartyClaudeStorage {
    third_party_local_state: bool,
    third_party_preferences: bool,
}

#[cfg(any(windows, test))]
impl ClaudeStoragePresence {
    pub(crate) fn fresh(self) -> bool {
        !self.normal.claude_local_state
            && !self.normal.claude_preferences
            && !self.third_party.third_party_local_state
            && !self.third_party.third_party_preferences
    }

    fn parse(output: &str) -> Option<Self> {
        let fields: Vec<_> = output.strip_suffix('\n')?.split(' ').collect();
        let ["storage", normal, preferences, third, third_preferences] = fields.as_slice() else {
            return None;
        };
        let bit = |value: &str| match value {
            "0" => Some(false),
            "1" => Some(true),
            _ => None,
        };
        Some(Self {
            normal: NormalClaudeStorage {
                claude_local_state: bit(normal)?,
                claude_preferences: bit(preferences)?,
            },
            third_party: ThirdPartyClaudeStorage {
                third_party_local_state: bit(third)?,
                third_party_preferences: bit(third_preferences)?,
            },
        })
    }
}

fn parse_known_folders(bytes: &[u8]) -> Option<bool> {
    match bytes {
        b"true\n" => Some(true),
        b"false\n" => Some(false),
        _ => None,
    }
}

impl Native {
    /// Compare native Foundation folders with the original managed launch HOME.
    pub(crate) async fn claude_known_folders(&self, home: &Path) -> Option<bool> {
        use tokio::io::AsyncReadExt as _;
        let mut child = tokio::process::Command::new(&self.executable)
            .env_clear()
            .env("HOME", home)
            .arg("--claude-known-folders")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .ok()?;
        let stdout = child.stdout.take()?;
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut bytes = Vec::new();
            stdout.take(16).read_to_end(&mut bytes).await.ok()?;
            if !child.wait().await.ok()?.success() {
                return None;
            }
            parse_known_folders(&bytes)
        })
        .await
        .ok()
        .flatten()
    }

    #[cfg(windows)]
    pub(crate) fn executable(&self) -> &Path {
        &self.executable
    }

    pub(crate) fn new() -> Result<Self, Reason> {
        let directory = tempfile::Builder::new()
            .prefix("nanh-desktop-native-")
            .tempdir()
            .map_err(|_| Reason::IsolationUnavailable)?;
        create_private_dir_all(directory.path()).map_err(|_| Reason::IsolationUnavailable)?;
        let executable = directory.path().join(if cfg!(windows) {
            "helper.exe"
        } else {
            "helper"
        });
        open_private_new(&executable)
            .and_then(|mut file| file.write_all(include_bytes!(env!("NAN_DESKTOP_NATIVE_HELPER"))))
            .map_err(|_| Reason::IsolationUnavailable)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| Reason::IsolationUnavailable)?;
        }
        open_private_new(&directory.path().join("eng.traineddata"))
            .and_then(|mut file| file.write_all(include_bytes!(env!("NAN_DESKTOP_OCR_MODEL"))))
            .map_err(|_| Reason::IsolationUnavailable)?;
        Ok(Self {
            directory,
            executable,
        })
    }

    pub(crate) fn windows(&self) -> Result<Snapshot, Reason> {
        self.windows_with_category()
            .map_err(FailureCategory::reason)
    }

    pub(crate) fn windows_with_category(&self) -> Result<Snapshot, FailureCategory> {
        let output =
            process::run_with_category(&self.executable, std::ffi::OsStr::new("--windows"), None)?;
        Snapshot::parse(&output).map_err(|_| FailureCategory::Pipe)
    }

    pub(crate) fn windows_with_focus(&self, owned_pid: u32) -> Result<Snapshot, FailureCategory> {
        if !claude_focus_policy() {
            return self.windows_with_category();
        }
        if owned_pid == 0 {
            return Err(FailureCategory::InvalidInput);
        }
        let argument = format!("--windows-focus {owned_pid}");
        let output =
            process::run_with_category(&self.executable, std::ffi::OsStr::new(&argument), None)?;
        Snapshot::parse(&output).map_err(|_| FailureCategory::Pipe)
    }

    pub(crate) fn windows_for_absence(&self) -> Result<Vec<Window>, Reason> {
        // Absence needs complete window enumeration, not a focused application.
        // Return only windows so this inventory cannot certify an input/capture guard.
        let output = process::run(
            &self.executable,
            std::ffi::OsStr::new("--windows-absence"),
            None,
        )?;
        Ok(Snapshot::parse(&output)?.windows)
    }

    #[cfg(windows)]
    pub(crate) fn process_presence_until(
        &self,
        claude: bool,
        deadline: std::time::Instant,
    ) -> Result<zeroize::Zeroizing<String>, FailureCategory> {
        process::run_process_presence_until(&self.executable, claude, deadline)
    }

    #[cfg(any(windows, test))]
    pub(crate) fn claude_storage_until(
        &self,
        deadline: std::time::Instant,
    ) -> Option<ClaudeStoragePresence> {
        let output = process::run_claude_storage_until(&self.executable, deadline).ok()?;
        ClaudeStoragePresence::parse(&output)
    }

    #[cfg(any(windows, test))]
    pub(crate) fn windows_for_absence_until(
        &self,
        deadline: std::time::Instant,
    ) -> Result<Vec<Window>, Reason> {
        let output = process::run_absence_until(&self.executable, deadline)?;
        let windows = Snapshot::parse(&output)?.windows;
        if std::time::Instant::now() >= deadline {
            return Err(Reason::ActionUnsupported);
        }
        Ok(windows)
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn claude_identity_observation(
        &self,
        bundle: &Path,
    ) -> Result<
        (
            ClaudeIdentityObservation,
            crate::diagnostics::ClaudeReadiness,
            Option<crate::diagnostics::ClaudeMatchedWindowInventory>,
        ),
        FailureCategory,
    > {
        let input = bundle
            .to_str()
            .ok_or(FailureCategory::InvalidInput)?
            .as_bytes();
        let mut framed = Vec::with_capacity(input.len() + 1);
        framed.extend_from_slice(input);
        framed.push(b'\n');
        if identity::parse_bundle_input(&framed).is_err() {
            return Err(FailureCategory::InvalidInput);
        }
        let output = process::run_with_category_input(
            &self.executable,
            std::ffi::OsStr::new("--claude-observation"),
            input,
        )?;
        let observation =
            ClaudeIdentityObservation::parse(&output).map_err(|_| FailureCategory::Output)?;
        let readiness = ClaudeIdentityObservation::parse_readiness(&output).unwrap_or(
            crate::diagnostics::ClaudeReadiness {
                finished_launching: None,
                hidden: None,
                active: None,
            },
        );
        let inventory = ClaudeIdentityObservation::parse_inventory(&output)
            .map_err(|_| FailureCategory::Output)?;
        Ok((observation, readiness, inventory))
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn activate_owned_window(&self, window: &Window) -> Result<(), FailureCategory> {
        let argument = format!("--activate-window {} {}", window.id, window.pid);
        process::run_with_category(&self.executable, std::ffi::OsStr::new(&argument), None)
            .map(|_| ())
    }

    #[cfg(windows)]
    pub(crate) fn missing_window_state(&self, window: &Window) -> &'static str {
        let argument = format!("--window-state {} {}", window.id, window.pid);
        match process::run_with_category(&self.executable, std::ffi::OsStr::new(&argument), None) {
            Ok(output) => match output.trim() {
                "visible" => "visible",
                "gone" => "gone",
                "identity-changed" => "identity-changed",
                "minimized" => "minimized",
                "hidden" => "hidden",
                "cloaked" => "cloaked",
                "child-window" => "child-window",
                "process-name-unavailable" => "process-name-unavailable",
                "candidate-too-small" => "candidate-too-small",
                _ => "query-unavailable",
            },
            Err(_) => "query-unavailable",
        }
    }

    #[cfg(windows)]
    pub(crate) fn fit_owned_window(&self, window: &Window) -> Result<(), FitWindowError> {
        let argument = format!("--fit-window {} {}", window.id, window.pid);
        let output =
            process::run_with_category(&self.executable, std::ffi::OsStr::new(&argument), None)
                .map_err(FitWindowError::Transport)?;
        if output.trim().is_empty() {
            return Ok(());
        }
        Err(FitWindowError::Diagnostic(
            FitFailure::parse(&output)
                .map_err(|_| FitWindowError::Transport(FailureCategory::Output))?,
        ))
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn fit_mac_owned_until(
        &self,
        window: &Window,
        deadline: std::time::Instant,
    ) -> Result<(), Reason> {
        let argument = format!("--fit-window {} {}", window.id, window.pid);
        let output =
            process::run_fit_until(&self.executable, std::ffi::OsStr::new(&argument), deadline)?;
        if !output.trim().is_empty() || std::time::Instant::now() >= deadline {
            return Err(Reason::WindowChanged);
        }
        Ok(())
    }

    pub(crate) fn recognize(&self, screenshot: &Screenshot) -> Result<Page, Reason> {
        let output = process::run(
            &self.executable,
            self.directory.path().as_os_str(),
            Some(screenshot),
        )?;
        Page::parse(&output, screenshot.width, screenshot.height)
    }
}

pub(crate) fn claude_focus_policy() -> bool {
    let Some(directory) = std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS").map(PathBuf::from)
    else {
        return false;
    };
    let Ok(metadata) = std::fs::symlink_metadata(&directory) else {
        return false;
    };
    if !metadata.is_dir() || directory.canonicalize().ok().as_deref() != Some(directory.as_path()) {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o077 != 0 {
            return false;
        }
    }
    cfg!(target_os = "macos")
        && std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
        && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted")
        && std::env::var("RUNNER_OS").as_deref() == Ok("macOS")
        && std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() == Ok("startup-baseline")
        && std::env::var("NANH_CLAUDE_MAC_PROFILE_POLICY").as_deref() == Ok("native-known-folders")
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_storage_protocol_distinguishes_freshness_and_rejects_partial_output() {
        let fresh = ClaudeStoragePresence::parse("storage 0 0 0 0\n").unwrap();
        assert!(fresh.fresh());
        let existing = ClaudeStoragePresence::parse("storage 0 0 1 0\n").unwrap();
        assert!(!existing.fresh());
        assert!(existing.third_party.third_party_local_state);
        let value = serde_json::to_value(existing).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 4);
        assert_eq!(value["thirdPartyLocalState"], true);
        assert_eq!(
            serde_json::from_value::<ClaudeStoragePresence>(value.clone()).unwrap(),
            existing
        );
        let mut extra = value;
        extra["privatePath"] = serde_json::json!("rejected");
        assert!(serde_json::from_value::<ClaudeStoragePresence>(extra).is_err());
        for invalid in [
            "storage 0 0 0\n",
            "storage 0 0 0 2\n",
            "storage 0 0 0 0",
            "storage 0 0 0 0\nprivate",
            "storage  0 0 0 0\n",
        ] {
            assert!(ClaudeStoragePresence::parse(invalid).is_none());
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn native_storage_transport_observes_only_closed_output_and_failed_child() {
        use std::os::unix::fs::PermissionsExt as _;
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("synthetic-storage-helper");
        std::fs::write(&executable, "#!/bin/sh\n[ \"$1\" = --version ] && exit 0\n[ \"$1\" = --windows-claude-storage ] || exit 1\nprintf 'storage 0 0 1 0\\n'\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        nan_harness_test_support::executable_fixture::wait_until_ready(&executable).unwrap();
        let native = Native {
            directory,
            executable,
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        assert!(
            native
                .claude_storage_until(deadline)
                .unwrap()
                .third_party
                .third_party_local_state
        );
        assert!(
            native
                .claude_storage_until(std::time::Instant::now())
                .is_none()
        );
        std::fs::remove_file(&native.executable).unwrap();
        assert!(native.claude_storage_until(deadline).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn bounded_absence_reuses_prepared_synthetic_helper_and_parses_complete_inventory() {
        use std::os::unix::fs::PermissionsExt as _;
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("synthetic-helper");
        std::fs::write(&executable, "#!/bin/sh\n[ \"$1\" = --version ] && exit 0\n[ \"$1\" = --windows-absence ] || exit 1\nprintf 'FG 0 0\\nDISPLAY 0 0 800 600\\n'\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        nan_harness_test_support::executable_fixture::wait_until_ready(&executable).unwrap();
        let native = Native {
            directory,
            executable,
        };
        assert!(
            native
                .windows_for_absence_until(
                    std::time::Instant::now() + std::time::Duration::from_secs(1)
                )
                .unwrap()
                .is_empty()
        );
        assert!(
            native
                .windows_for_absence_until(std::time::Instant::now())
                .is_err()
        );
    }

    #[test]
    fn known_folder_protocol_rejects_partial_and_extra_output() {
        assert_eq!(parse_known_folders(b"true\n"), Some(true));
        assert_eq!(parse_known_folders(b"false\n"), Some(false));
        for bytes in [
            b"true".as_slice(),
            b"true\nfalse\n",
            b"",
            b" true\n",
            b"TRUE\n",
        ] {
            assert_eq!(parse_known_folders(bytes), None);
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn known_folder_transport_preserves_home_and_rejects_failed_child() {
        use std::os::unix::fs::PermissionsExt as _;
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("synthetic-helper");
        let native = Native {
            directory,
            executable,
        };
        let home = native.directory.path().join("original-home");
        let script = "#!/bin/sh\n[ \"$1\" = --claude-known-folders ] || exit 1\ncase \"$HOME\" in */original-home) ;; *) exit 1 ;; esac\nprintf 'true\\n'\n";
        std::fs::write(&native.executable, script).unwrap();
        std::fs::set_permissions(&native.executable, std::fs::Permissions::from_mode(0o700))
            .unwrap();
        assert_eq!(native.claude_known_folders(&home).await, Some(true));
        std::fs::write(&native.executable, "#!/bin/sh\nprintf 'true\\n'\nexit 1\n").unwrap();
        assert_eq!(native.claude_known_folders(&home).await, None);
    }

    use super::*;

    #[test]
    fn native_window_fitting_rejects_missing_identity_without_changing_windows() {
        let native = Native::new().unwrap();
        assert!(
            process::run(
                &native.executable,
                std::ffi::OsStr::new("--fit-window 0 0"),
                None
            )
            .is_err()
        );
    }

    #[test]
    fn fit_failure_protocol_is_closed_and_empty_success_is_distinct() {
        assert!(FitFailure::parse("").is_err());
        let stages = [
            ("request", FitFailureStage::Request),
            ("identity-read", FitFailureStage::IdentityRead),
            ("identity-mismatch", FitFailureStage::IdentityMismatch),
            ("foreground-read", FitFailureStage::ForegroundRead),
            ("foreground-mismatch", FitFailureStage::ForegroundMismatch),
            ("monitor-read", FitFailureStage::MonitorRead),
            ("workarea-read", FitFailureStage::WorkareaRead),
            ("window-read", FitFailureStage::WindowRead),
            ("workarea-invalid", FitFailureStage::WorkareaInvalid),
            ("identity-changed", FitFailureStage::IdentityChanged),
            ("foreground-changed", FitFailureStage::ForegroundChanged),
            ("resize", FitFailureStage::Resize),
            (
                "postcondition-identity-read",
                FitFailureStage::PostconditionIdentityRead,
            ),
            (
                "postcondition-identity-mismatch",
                FitFailureStage::PostconditionIdentityMismatch,
            ),
            (
                "postcondition-foreground-read",
                FitFailureStage::PostconditionForegroundRead,
            ),
            (
                "postcondition-foreground-mismatch",
                FitFailureStage::PostconditionForegroundMismatch,
            ),
            (
                "postcondition-window-read",
                FitFailureStage::PostconditionWindowRead,
            ),
            (
                "postcondition-geometry",
                FitFailureStage::PostconditionGeometry,
            ),
        ];
        for (name, expected) in stages {
            assert_eq!(
                FitFailure::parse(&format!("FIT_FAILURE {name}\n"))
                    .unwrap()
                    .stage,
                expected
            );
        }
        for invalid in [
            "FIT_FAILURE identity-mismatch 5\n",
            "FIT_FAILURE unknown\n",
            "FIT_FAILURE resize extra\n",
            "FIT_FAILURE request different-process\n",
            "FIT_FAILURE foreground-read different-process\n",
            "FIT_FAILURE foreground-read same-process-different-window\n",
            "FIT_FAILURE foreground-mismatch unknown\n",
            "FIT_FAILURE foreground-mismatch different-process extra\n",
        ] {
            assert!(FitFailure::parse(invalid).is_err());
        }
        assert_eq!(
            FitFailure::parse("FIT_FAILURE foreground-mismatch same-process-different-window\n")
                .unwrap()
                .foreground_relation,
            Some(FitForegroundRelation::SameProcessDifferentWindow)
        );
        assert_eq!(
            FitFailure::parse("FIT_FAILURE foreground-changed different-process\n")
                .unwrap()
                .foreground_relation,
            Some(FitForegroundRelation::DifferentProcess)
        );
        assert_eq!(
            FitFailure::parse("FIT_FAILURE foreground-read identity-unavailable\n")
                .unwrap()
                .foreground_relation,
            Some(FitForegroundRelation::IdentityUnavailable)
        );
    }

    #[test]
    fn native_window_activation_rejects_invalid_identity_without_activation() {
        let native = Native::new().unwrap();
        for request in [
            "--activate-window 0 1",
            "--activate-window 1 0",
            "--activate-window 18446744073709551616 1",
            "--activate-window 4294967296 1",
            "--activate-window -1 1",
            "--activate-window 1 -1",
            "--activate-window 1 4294967296",
            "--activate-window 1 1 trailing",
        ] {
            assert!(process::run(&native.executable, std::ffi::OsStr::new(request), None).is_err());
        }
    }

    #[test]
    fn bundled_helper_runs_without_an_external_ocr_installation() {
        let native = Native::new().unwrap();
        let output =
            process::run(&native.executable, std::ffi::OsStr::new("--version"), None).unwrap();
        assert_eq!(output.trim(), "nanh-desktop-native tesseract-5.5.2");
        assert_eq!(
            crate::report::digest(include_bytes!(env!("NAN_DESKTOP_OCR_MODEL"))),
            "7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2"
        );
    }

    #[test]
    fn helper_failure_categories_are_closed_and_privacy_safe() {
        let categories = [
            (FailureCategory::Spawn, Reason::ActionUnsupported),
            (FailureCategory::Pipe, Reason::ActionUnsupported),
            (FailureCategory::Output, Reason::ResponseMismatch),
            (FailureCategory::Timeout, Reason::ActionUnsupported),
            (FailureCategory::NonzeroExit, Reason::ActionUnsupported),
        ];
        for (category, reason) in categories {
            assert_eq!(category.reason(), reason);
            assert!(matches!(
                category,
                FailureCategory::Spawn
                    | FailureCategory::Pipe
                    | FailureCategory::Output
                    | FailureCategory::Timeout
                    | FailureCategory::NonzeroExit
            ));
        }
    }

    #[test]
    fn bundled_ocr_reads_synthetic_pixels_and_rejects_a_wrong_marker() {
        let glyphs = [
            [
                "11111", "00100", "00100", "00100", "00100", "00100", "00100",
            ],
            [
                "11111", "10000", "10000", "11110", "10000", "10000", "11111",
            ],
            [
                "01111", "10000", "10000", "01110", "00001", "00001", "11110",
            ],
            [
                "11111", "00100", "00100", "00100", "00100", "00100", "00100",
            ],
        ];
        let mut screenshot = Screenshot {
            width: 300,
            height: 100,
            pixels: vec![255; 300 * 100 * 4],
            scale: 1.0,
        };
        for (letter, glyph) in glyphs.iter().enumerate() {
            for (row, line) in glyph.iter().enumerate() {
                for (column, pixel) in line.bytes().enumerate() {
                    if pixel == b'1' {
                        for y in 0..6 {
                            for x in 0..6 {
                                let offset =
                                    ((20 + row * 6 + y) * 300 + 30 + letter * 36 + column * 6 + x)
                                        * 4;
                                screenshot.pixels[offset..offset + 3].fill(0);
                            }
                        }
                    }
                }
            }
        }
        let native = Native::new().unwrap();
        let page = native.recognize(&screenshot).unwrap();
        assert!(page.find_phrase("TEST").is_some());
        assert!(page.find_phrase("WRONG").is_none());
        let scaled = prepare_ocr_image(Screenshot {
            width: screenshot.width,
            height: screenshot.height,
            pixels: screenshot.pixels.clone(),
            scale: screenshot.scale,
        })
        .unwrap();
        let scaled_page = native.recognize(&scaled).unwrap();
        let original_bounds = page.find_phrase("TEST").unwrap();
        let scaled_bounds = scaled_page.find_phrase("TEST").unwrap();
        assert!((scaled_bounds.x / 2 - original_bounds.x).abs() <= 1);
        assert!((scaled_bounds.y / 2 - original_bounds.y).abs() <= 1);
        assert!(scaled_page.find_phrase("WRONG").is_none());
        screenshot.pixels.fill(255);
        assert!(native.recognize(&screenshot).unwrap().words.is_empty());
    }
}
