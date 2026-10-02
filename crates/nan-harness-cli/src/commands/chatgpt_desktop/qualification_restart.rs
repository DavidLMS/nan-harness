//! An owned, one-time handoff requested by the disposable desktop itself.
use super::super::ChatGptDesktopError;
use super::SupervisedApp;
use std::io::{Error, ErrorKind, Read as _};
use std::path::{Path, PathBuf};
use tokio::process::{Child, Command};

const SWITCH: &str = "--codex-browser-background-networking-disabled";

pub(super) struct OwnedRestart<'a> {
    pub(super) child: Child,
    pub(super) command: &'a mut Command,
    marker: PathBuf,
    facts: PathBuf,
    _directory: tempfile::TempDir,
    restarted: bool,
    disabled: bool,
    executable: std::fs::Metadata,
    capture: Option<super::StderrCapture>,
    capture_stderr: bool,
    started: tokio::time::Instant,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Request {
    browser_background_networking_disabled: bool,
}

fn private_directory(path: &Path) -> bool {
    if !path.is_absolute() || path.is_symlink() || !path.is_dir() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        path.metadata()
            .is_ok_and(|metadata| metadata.permissions().mode().trailing_zeros() >= 6)
    }
    #[cfg(not(unix))]
    nan_harness_private_fs::restrict_path(path, nan_harness_private_fs::PrivatePathKind::Directory)
        .is_ok()
}

impl<'a> OwnedRestart<'a> {
    pub(super) fn prepare(
        command: &'a mut Command,
        capture_stderr: bool,
    ) -> Result<Option<Self>, ChatGptDesktopError> {
        if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        {
            return Ok(None);
        }
        let Some(facts) = std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS") else {
            return Ok(None);
        };
        let facts = PathBuf::from(facts);
        let Some(profile) = std::env::var_os("CODEX_ELECTRON_USER_DATA_PATH") else {
            return Ok(None);
        };
        let profile = PathBuf::from(profile);
        if !private_directory(&facts) || !private_directory(&profile) {
            return Ok(None);
        }
        let directory = tempfile::Builder::new()
            .prefix("codex-restart-")
            .tempdir_in(&facts)
            .map_err(ChatGptDesktopError::StartApp)?;
        nan_harness_private_fs::restrict_path(
            directory.path(),
            nan_harness_private_fs::PrivatePathKind::Directory,
        )
        .map_err(ChatGptDesktopError::StartApp)?;
        let marker = directory.path().join("request.json");
        command
            .env("CODEX_ELECTRON_DEV_RELAUNCH_MARKER_PATH", &marker)
            .env_remove("CODEX_DESKTOP_RELAUNCH_OPEN_EVENTS");
        let disabled = command
            .as_std()
            .get_args()
            .any(|argument| argument == SWITCH);
        let executable = std::fs::metadata(command.as_std().get_program())
            .map_err(ChatGptDesktopError::StartApp)?;
        let mut child = command.spawn().map_err(ChatGptDesktopError::StartApp)?;
        let capture = child.stderr.take().map(super::start_stderr_capture);
        receipt(&facts, Stage::Armed);
        Ok(Some(Self {
            child,
            command,
            marker,
            facts,
            _directory: directory,
            restarted: false,
            disabled,
            executable,
            capture,
            capture_stderr,
            started: tokio::time::Instant::now(),
        }))
    }
}

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum Stage {
    Armed,
    Restarted,
    NoRequest,
    InvalidRequest,
    ExecutableChanged,
    ChildExited,
    StartupTimeout,
    BridgeStopped,
    Cancelled,
}

impl Stage {
    const fn name(self) -> &'static str {
        match self {
            Self::Armed => "armed",
            Self::Restarted => "restarted",
            Self::NoRequest => "no-request",
            Self::InvalidRequest => "invalid-request",
            Self::ExecutableChanged => "executable-changed",
            Self::ChildExited => "child-exited",
            Self::StartupTimeout => "startup-timeout",
            Self::BridgeStopped => "bridge-stopped",
            Self::Cancelled => "cancelled",
        }
    }
}

// Receipts are advisory only: a failed write cannot affect admission or authorize a restart.
fn receipt(directory: &Path, stage: Stage) {
    let name = stage.name();
    let path = directory.join(format!(
        "codex-owned-relaunch-{}-{name}.json",
        std::process::id()
    ));
    if let Ok(mut file) = nan_harness_private_fs::open_private_new(&path) {
        let value = serde_json::json!({
            "schemaVersion": 1,
            "mechanism": "codex-owned-relaunch",
            "diagnosticsOnly": true,
            "stage": stage,
        });
        let _ = serde_json::to_writer(&mut file, &value);
    }
}

fn consume(
    marker: &Path,
    code: i32,
    restarted: bool,
    disabled: bool,
) -> std::io::Result<Option<bool>> {
    let metadata = match std::fs::symlink_metadata(marker) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if code != 0 || restarted || !metadata.file_type().is_file() || metadata.len() > 1024 {
        return Err(Error::from(ErrorKind::InvalidData));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o777 != 0o600 {
            return Err(Error::from(ErrorKind::PermissionDenied));
        }
    }
    let (file, _) = nan_harness_private_fs::open_private_read(marker)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let opened = file.metadata()?;
        if metadata.dev() != opened.dev() || metadata.ino() != opened.ino() || opened.nlink() != 1 {
            return Err(Error::from(ErrorKind::InvalidData));
        }
    }
    let mut bytes = Vec::new();
    file.take(1025).read_to_end(&mut bytes)?;
    let requested = parse(&bytes, disabled)?;
    std::fs::remove_file(marker)?;
    Ok(Some(requested))
}

fn same_executable(original: &std::fs::Metadata, current: &std::fs::Metadata) -> bool {
    if original.len() != current.len() || original.modified().ok() != current.modified().ok() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        original.dev() == current.dev() && original.ino() == current.ino()
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn parse(bytes: &[u8], disabled: bool) -> std::io::Result<bool> {
    if bytes.len() > 1024 {
        return Err(Error::from(ErrorKind::InvalidData));
    }
    let request: Request =
        serde_json::from_slice(bytes).map_err(|_| Error::from(ErrorKind::InvalidData))?;
    if request.browser_background_networking_disabled == disabled {
        return Err(Error::from(ErrorKind::InvalidData));
    }
    Ok(request.browser_background_networking_disabled)
}

impl SupervisedApp for OwnedRestart<'_> {
    fn stopping(&self, cause: super::StopCause) {
        receipt(
            &self.facts,
            match cause {
                super::StopCause::StartupTimeout => Stage::StartupTimeout,
                super::StopCause::BridgeStopped => Stage::BridgeStopped,
                super::StopCause::Cancelled => Stage::Cancelled,
            },
        );
    }
    fn restart_enabled(&self) -> bool {
        true
    }
    async fn wait(&mut self) -> Result<i32, ChatGptDesktopError> {
        let status = self
            .child
            .wait()
            .await
            .map_err(ChatGptDesktopError::WaitForApp)?;
        receipt(&self.facts, Stage::ChildExited);
        let early = self.started.elapsed() < std::time::Duration::from_millis(500);
        let stderr = if let Some(capture) = self.capture.as_mut() {
            super::finish_stderr_capture(capture).await
        } else {
            None
        };
        self.capture = None;
        if early && !self.marker.exists() {
            receipt(&self.facts, Stage::NoRequest);
            let error = super::classify_early_exit(status.success(), super::chatgpt_is_running()?);
            let failure = if matches!(error, ChatGptDesktopError::SingletonRace) {
                crate::native_diagnostic::Failure::NativeAlreadyRunning
            } else {
                crate::native_diagnostic::Failure::NativeAppExited
            };
            crate::native_diagnostic::emit_startup(
                failure,
                status,
                stderr.as_ref(),
                Path::new(self.command.as_std().get_program()),
            );
            return Err(error);
        }
        Ok(super::exit_code(status))
    }

    async fn stop(&mut self) -> Result<(), ChatGptDesktopError> {
        super::stop_chatgpt(&mut self.child).await
    }
    fn restart(&mut self, code: i32) -> Result<bool, ChatGptDesktopError> {
        let disabled = match consume(&self.marker, code, self.restarted, self.disabled) {
            Ok(Some(disabled)) => disabled,
            Ok(None) => {
                receipt(&self.facts, Stage::NoRequest);
                return Ok(false);
            }
            Err(error) => {
                receipt(&self.facts, Stage::InvalidRequest);
                return Err(ChatGptDesktopError::WaitForApp(error));
            }
        };
        let current = std::fs::metadata(self.command.as_std().get_program());
        if !current
            .as_ref()
            .is_ok_and(|current| same_executable(&self.executable, current))
        {
            receipt(&self.facts, Stage::ExecutableChanged);
            return Err(ChatGptDesktopError::StartApp(Error::from(
                ErrorKind::InvalidData,
            )));
        }
        self.restarted = true;
        let arguments: Vec<_> = self
            .command
            .as_std()
            .get_args()
            .filter(|argument| *argument != SWITCH)
            .map(std::ffi::OsStr::to_os_string)
            .collect();
        // Command has no clear-args operation: build a replacement with the same explicit launch environment.
        let mut next = Command::new(self.command.as_std().get_program());
        next.args(arguments)
            .kill_on_drop(true)
            .stdout(std::process::Stdio::null())
            .stderr(if self.capture_stderr {
                std::process::Stdio::piped()
            } else {
                std::process::Stdio::null()
            });
        if let Some(directory) = self.command.as_std().get_current_dir() {
            next.current_dir(directory);
        }
        if disabled {
            next.arg(SWITCH);
        }
        for (key, value) in self.command.as_std().get_envs() {
            if let Some(value) = value {
                next.env(key, value);
            } else {
                next.env_remove(key);
            }
        }
        self.child = next.spawn().map_err(ChatGptDesktopError::StartApp)?;
        self.capture = self.child.stderr.take().map(super::start_stderr_capture);
        self.started = tokio::time::Instant::now();
        self.disabled = disabled;
        receipt(&self.facts, Stage::Restarted);
        Ok(true)
    }
}

impl Drop for OwnedRestart<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.marker);
    }
}

#[cfg(test)]
mod tests {
    struct Handoff {
        restarts: usize,
        stops: usize,
        stop_cause: std::cell::Cell<Option<&'static str>>,
    }

    impl super::SupervisedApp for Handoff {
        fn stopping(&self, cause: super::super::StopCause) {
            self.stop_cause.set(Some(match cause {
                super::super::StopCause::StartupTimeout => "timeout",
                super::super::StopCause::BridgeStopped => "bridge",
                super::super::StopCause::Cancelled => "cancelled",
            }));
        }
        async fn wait(&mut self) -> Result<i32, super::ChatGptDesktopError> {
            if self.restarts == 0 {
                Ok(0)
            } else {
                std::future::pending().await
            }
        }
        async fn stop(&mut self) -> Result<(), super::ChatGptDesktopError> {
            self.stops += 1;
            Ok(())
        }
        fn restart_enabled(&self) -> bool {
            true
        }
        fn restart(&mut self, _: i32) -> Result<bool, super::ChatGptDesktopError> {
            self.restarts += 1;
            Ok(true)
        }
    }

    #[tokio::test(start_paused = true)]
    async fn handoff_keeps_original_deadline_and_bridge_failure_prevents_restart() {
        use crate::commands::chatgpt_desktop::startup::{StartupPolicy, StartupWatch};
        use std::time::Duration;
        let mut app = Handoff {
            restarts: 0,
            stops: 0,
            stop_cause: std::cell::Cell::new(None),
        };
        let (_activities, mut activity) = tokio::sync::broadcast::channel(1);
        let (_diagnostics, mut diagnostic) = tokio::sync::mpsc::unbounded_channel();
        let cancellation = nan_harness_runtime::CancellationToken::new();
        let mut watch =
            StartupWatch::new(StartupPolicy::resolve(Some(Duration::from_secs(10)), false));
        tokio::time::advance(Duration::from_secs(8)).await;
        let started = tokio::time::Instant::now();
        let result = super::super::supervise_startup(
            &mut app,
            std::future::pending(),
            &mut activity,
            &mut diagnostic,
            &mut watch,
            &cancellation,
            &mut Vec::new(),
        )
        .await;
        assert!(matches!(
            result,
            Err(super::ChatGptDesktopError::BridgeHandshakeTimeout)
        ));
        assert_eq!(app.restarts, 1);
        assert_eq!(app.stops, 1);
        assert_eq!(app.stop_cause.get(), Some("timeout"));
        assert_eq!(started.elapsed(), Duration::from_secs(2));
        let mut app = Handoff {
            restarts: 0,
            stops: 0,
            stop_cause: std::cell::Cell::new(None),
        };
        let result = super::super::supervise_startup(
            &mut app,
            std::future::ready(super::ChatGptDesktopError::BridgeExited),
            &mut activity,
            &mut diagnostic,
            &mut StartupWatch::new(StartupPolicy::resolve(None, false)),
            &cancellation,
            &mut Vec::new(),
        )
        .await;
        assert!(matches!(
            result,
            Err(super::ChatGptDesktopError::BridgeExited)
        ));
        assert_eq!(app.restarts, 0);
        assert_eq!(app.stop_cause.get(), Some("bridge"));
    }

    #[test]
    fn receipts_are_closed_private_and_never_overwrite_a_transition() {
        let directory = tempfile::tempdir().unwrap();
        super::receipt(directory.path(), super::Stage::Armed);
        let path = std::fs::read_dir(directory.path())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let bytes = std::fs::read(&path).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"schemaVersion":1,"mechanism":"codex-owned-relaunch","diagnosticsOnly":true,"stage":"armed"})
        );
        std::fs::write(&path, b"unchanged").unwrap();
        super::receipt(directory.path(), super::Stage::Armed);
        assert_eq!(std::fs::read(&path).unwrap(), b"unchanged");
        super::receipt(&directory.path().join("missing"), super::Stage::Restarted);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    fn marker(directory: &tempfile::TempDir) -> std::path::PathBuf {
        let path = directory.path().join("request.json");
        let mut file = nan_harness_private_fs::open_private_new(&path).unwrap();
        std::io::Write::write_all(
            &mut file,
            br#"{"browserBackgroundNetworkingDisabled":true}"#,
        )
        .unwrap();
        path
    }

    #[test]
    fn clean_exit_consumes_private_request_once() {
        let directory = tempfile::tempdir().unwrap();
        let path = marker(&directory);
        assert!(super::consume(&path, 1, false, false).is_err());
        assert!(path.exists());
        assert!(super::consume(&path, 0, true, false).is_err());
        assert_eq!(super::consume(&path, 0, false, false).unwrap(), Some(true));
        assert!(!path.exists());
        assert_eq!(super::consume(&path, 0, true, true).unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn writable_or_linked_requests_are_rejected() {
        use std::os::unix::fs::{PermissionsExt as _, symlink};
        let directory = tempfile::tempdir().unwrap();
        let path = marker(&directory);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(super::consume(&path, 0, false, false).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = directory.path().join("link");
        symlink(&path, &link).unwrap();
        assert!(super::consume(&link, 0, false, false).is_err());
        let hardlink = directory.path().join("hardlink");
        std::fs::hard_link(&path, &hardlink).unwrap();
        assert!(super::consume(&path, 0, false, false).is_err());
    }

    #[test]
    fn replacing_the_executable_is_not_an_owned_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("desktop");
        std::fs::write(&path, b"original").unwrap();
        let original = std::fs::metadata(&path).unwrap();
        assert!(super::same_executable(
            &original,
            &std::fs::metadata(&path).unwrap()
        ));
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"replacement").unwrap();
        assert!(!super::same_executable(
            &original,
            &std::fs::metadata(&path).unwrap()
        ));
    }

    #[test]
    fn only_a_changed_bounded_exact_request_is_admitted() {
        assert!(super::parse(br#"{"browserBackgroundNetworkingDisabled":true}"#, false).unwrap());
        for input in [
            br#"{"browserBackgroundNetworkingDisabled":false}"#.as_slice(),
            br#"{"browserBackgroundNetworkingDisabled":true,"desktopOpenEvents":"x"}"#,
            br#"{"browserBackgroundNetworkingDisabled":"true"}"#,
            b"null",
        ] {
            assert!(super::parse(input, false).is_err());
        }
        assert!(super::parse(&vec![b' '; 1025], false).is_err());
    }
}
