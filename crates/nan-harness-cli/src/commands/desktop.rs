#[cfg(feature = "desktop-qualification")]
mod claude_profile;
#[cfg(feature = "desktop-qualification")]
mod qualification;

pub(crate) fn spawn_observed_desktop(
    command: &mut std::process::Command,
    app: &'static str,
) -> std::io::Result<()> {
    #[cfg(feature = "desktop-qualification")]
    {
        qualification::spawn(command, app)
    }
    #[cfg(not(feature = "desktop-qualification"))]
    {
        let _ = app;
        command
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
    }
}

use nan_harness_private_fs::{
    PrivatePathKind, open_private_new, open_private_read_write, restrict_path,
};
use std::fs::{self, File, TryLockError};
use std::io::Write;
use std::path::Path;
use thiserror::Error;

#[derive(Debug)]
pub(crate) struct DesktopSessionLock {
    file: File,
}

impl DesktopSessionLock {
    pub(crate) fn acquire(directory: &Path) -> Result<Self, DesktopStateError> {
        create_private_directory(directory)?;
        let lock_path = directory.join("session.lock");
        reject_symlink(&lock_path)?;
        let file = open_private_read_write(&lock_path).map_err(DesktopStateError::Io)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { file }),
            Err(TryLockError::WouldBlock) => Err(DesktopStateError::AlreadyLocked),
            Err(TryLockError::Error(error)) => Err(DesktopStateError::Io(error)),
        }
    }
}

impl Drop for DesktopSessionLock {
    fn drop(&mut self) {
        let _ = File::unlock(&self.file);
    }
}

pub(crate) fn create_private_directory(path: &Path) -> Result<(), DesktopStateError> {
    reject_symlink(path)?;
    nan_harness_private_fs::create_private_dir_all(path).map_err(DesktopStateError::Io)?;
    restrict_path(path, PrivatePathKind::Directory).map_err(DesktopStateError::Io)
}

pub(crate) fn write_private_atomic(path: &Path, contents: &[u8]) -> Result<(), DesktopStateError> {
    let parent = path.parent().ok_or(DesktopStateError::InvalidPath)?;
    create_private_directory(parent)?;
    reject_symlink(path)?;
    let mut temporary = tempfile::Builder::new()
        .make_in(parent, open_private_new)
        .map_err(DesktopStateError::Io)?;
    temporary
        .write_all(contents)
        .and_then(|()| temporary.flush())
        .map_err(DesktopStateError::Io)?;
    temporary
        .persist(path)
        .map_err(|error| DesktopStateError::Io(error.error))?;
    restrict_path(path, PrivatePathKind::File).map_err(DesktopStateError::Io)
}

pub(crate) fn create_private_new(path: &Path) -> Result<File, DesktopStateError> {
    let parent = path.parent().ok_or(DesktopStateError::InvalidPath)?;
    create_private_directory(parent)?;
    reject_symlink(path)?;
    open_private_new(path).map_err(DesktopStateError::Io)
}

pub(crate) fn remove_file_if_present(path: &Path) -> Result<(), DesktopStateError> {
    reject_symlink(path)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DesktopStateError::Io(error)),
    }
}

pub(crate) fn reject_symlink(path: &Path) -> Result<(), DesktopStateError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(DesktopStateError::Symlink),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DesktopStateError::Io(error)),
    }
}

#[derive(Debug, Error)]
pub(crate) enum DesktopStateError {
    #[error("another managed desktop session is already active")]
    AlreadyLocked,
    #[error("the managed desktop state contains an unsafe symbolic link")]
    Symlink,
    #[error("the managed desktop state path is invalid")]
    InvalidPath,
    #[error("managed desktop state operation failed: {0}")]
    Io(std::io::Error),
}

impl DesktopStateError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::AlreadyLocked => "NH-DESKTOP-001",
            Self::Symlink | Self::InvalidPath => "NH-DESKTOP-002",
            Self::Io(_) => "NH-DESKTOP-003",
        }
    }
}

// Terminal localization is separate from canonical Display used by machine contracts.
impl nan_harness_i18n::TerminalMessage for DesktopStateError {
    fn terminal_message(&self, locale: nan_harness_i18n::Locale) -> String {
        use nan_harness_i18n::messages as m;
        if locale == nan_harness_i18n::Locale::En {
            return self.to_string();
        }
        match self {
            Self::AlreadyLocked => m::error_desktop_state_already_locked(locale),
            Self::Symlink => m::error_desktop_state_symlink(locale),
            Self::InvalidPath => m::error_desktop_state_invalid_path(locale),
            Self::Io(field_0) => m::error_desktop_state_io(locale, &(field_0)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DesktopSessionLock, DesktopStateError, write_private_atomic};

    #[test]
    fn desktop_session_lock_is_exclusive() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let state = directory.path().join("desktop");
        let first = DesktopSessionLock::acquire(&state).expect("first lock should succeed");
        assert!(matches!(
            DesktopSessionLock::acquire(&state),
            Err(DesktopStateError::AlreadyLocked)
        ));
        drop(first);
        DesktopSessionLock::acquire(&state).expect("released lock should be reusable");
    }

    #[test]
    fn atomic_state_write_creates_and_replaces_private_contents() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("desktop/session.json");
        for payload in [b"original".as_slice(), b"replacement".as_slice()] {
            write_private_atomic(&path, payload).expect("private atomic write");
            assert_eq!(std::fs::read(&path).expect("published contents"), payload);
            #[cfg(windows)]
            nan_harness_test_support::windows_acl::assert_private_file(&path)
                .expect("published DACL");
        }
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[test]
    fn failed_atomic_state_write_preserves_destination_and_cleans_staging() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("session.json");
        std::fs::create_dir(&path).expect("obstruction");
        std::fs::write(path.join("sentinel"), b"user-owned").expect("sentinel");
        assert!(write_private_atomic(&path, b"replacement").is_err());
        assert_eq!(std::fs::read(path.join("sentinel")).unwrap(), b"user-owned");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn desktop_state_uses_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let state = directory.path().join("desktop");
        let file = state.join("session.json");
        super::create_private_directory(&state).expect("directory should be private");
        write_private_atomic(&file, b"{}\n").expect("file should be private");

        assert_eq!(
            std::fs::metadata(&state)
                .expect("directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(file)
                .expect("file metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

pub(crate) fn newer_version_warning(harness: &str, detected: &str, last_tested: &str) -> String {
    format!(
        "warning: {harness} is newer than the last tested version (detected: {detected}; last tested: {last_tested}); continuing with this untested version"
    )
}

#[cfg(test)]
mod warning_tests {
    #[test]
    fn warning_distinguishes_installed_and_tested_versions_without_requesting_override() {
        let warning = super::newer_version_warning(
            "ChatGPT Desktop",
            "app 2.0.0, bundled Codex 3.0.0",
            "app 1.0.0, bundled Codex 2.0.0",
        );
        assert!(warning.starts_with("warning: ChatGPT Desktop"));
        assert!(warning.contains("detected: app 2.0.0, bundled Codex 3.0.0"));
        assert!(warning.contains("last tested: app 1.0.0, bundled Codex 2.0.0"));
        assert!(warning.contains("continuing"));
        assert!(!warning.contains("--allow-untested"));
    }
}

/// Fixed loopback renderer instrumentation, absent from normal CLI builds.
pub(crate) fn qualification_renderer_arguments(
    kind: nan_harness_core::DesktopHarnessKind,
) -> Vec<String> {
    #[cfg(feature = "desktop-qualification")]
    {
        let hosted = std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
            && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted");
        let mut arguments = renderer_arguments(
            kind,
            hosted,
            std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS")
                .map(std::path::PathBuf::from)
                .as_deref(),
            std::env::var("NANH_DESKTOP_QUALIFICATION_CDP_PORT")
                .ok()
                .as_deref(),
        );
        if std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() == Ok("startup-baseline") {
            arguments.clear();
            arguments.extend(startup_accessibility_arguments(
                kind,
                std::env::consts::OS,
                std::env::var("RUNNER_OS").ok().as_deref(),
                qualification_capture_enabled(),
                std::env::var("NANH_CLAUDE_MAC_PROFILE_POLICY")
                    .ok()
                    .as_deref(),
            ));
        }
        if qualification_capture_enabled()
            && cfg!(target_os = "linux")
            && std::env::var("NANH_DESKTOP_QUALIFICATION_NAMESPACE_POLICY").as_deref()
                == Ok("scoped-apparmor-userns")
        {
            arguments.push("--disable-setuid-sandbox".into());
        }
        arguments
    }
    #[cfg(not(feature = "desktop-qualification"))]
    {
        let _ = kind;
        Vec::new()
    }
}

#[cfg(feature = "desktop-qualification")]
fn startup_accessibility_arguments(
    kind: nan_harness_core::DesktopHarnessKind,
    platform: &str,
    runner_os: Option<&str>,
    capture_enabled: bool,
    mac_policy: Option<&str>,
) -> Vec<String> {
    // Chromium's public flag exposes native semantics without enabling CDP.
    let supported = platform == "linux"
        || (platform == "macos"
            && runner_os == Some("macOS")
            && mac_policy == Some("native-known-folders"));
    if kind == nan_harness_core::DesktopHarnessKind::Claude && capture_enabled && supported {
        vec!["--force-renderer-accessibility".into()]
    } else {
        Vec::new()
    }
}

#[cfg(feature = "desktop-qualification")]
fn qualification_capture_enabled() -> bool {
    let hosted = std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
        && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted");
    hosted
        && std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS")
            .map(std::path::PathBuf::from)
            .is_some_and(|path| path.is_absolute() && path.is_dir() && !path.is_symlink())
}

#[cfg(feature = "desktop-qualification")]
fn renderer_arguments(
    kind: nan_harness_core::DesktopHarnessKind,
    hosted: bool,
    directory: Option<&Path>,
    port: Option<&str>,
) -> Vec<String> {
    if hosted
        && directory.is_some_and(|path| path.is_absolute() && path.is_dir() && !path.is_symlink())
        && let Some(port) = port.and_then(|value| value.parse::<u16>().ok())
        && port > 1024
    {
        let mut arguments = vec![
            format!("--remote-debugging-port={port}"),
            "--remote-debugging-address=127.0.0.1".into(),
        ];
        if kind == nan_harness_core::DesktopHarnessKind::Pen && cfg!(target_os = "linux") {
            // SwANGLE supplies a software GLES driver on GPU-less hosted runners.
            arguments.extend(["--use-gl=angle".into(), "--use-angle=swiftshader".into()]);
        }
        return arguments;
    }
    Vec::new()
}

#[cfg(all(test, feature = "desktop-qualification"))]
mod renderer_tests {
    use super::{renderer_arguments, startup_accessibility_arguments};
    use nan_harness_core::DesktopHarnessKind;
    #[test]
    fn startup_accessibility_preserves_linux_and_scopes_the_mac_trial() {
        let claude = DesktopHarnessKind::Claude;
        let flag = vec!["--force-renderer-accessibility".to_owned()];
        assert_eq!(
            startup_accessibility_arguments(claude, "linux", None, true, None),
            flag
        );
        assert_eq!(
            startup_accessibility_arguments(
                claude,
                "macos",
                Some("macOS"),
                true,
                Some("native-known-folders")
            ),
            flag
        );
        for (kind, platform, runner, capture, policy) in [
            (claude, "macos", None, true, Some("native-known-folders")),
            (
                claude,
                "macos",
                Some("macOS"),
                false,
                Some("native-known-folders"),
            ),
            (
                claude,
                "macos",
                Some("macOS"),
                true,
                Some("electron-user-data-dir"),
            ),
            (
                claude,
                "windows",
                Some("Windows"),
                true,
                Some("native-known-folders"),
            ),
            (
                DesktopHarnessKind::Pen,
                "macos",
                Some("macOS"),
                true,
                Some("native-known-folders"),
            ),
            (claude, "linux", None, false, None),
        ] {
            assert!(
                startup_accessibility_arguments(kind, platform, runner, capture, policy).is_empty()
            );
        }
    }
    #[test]
    fn instrumentation_requires_owned_hosted_context_and_numeric_loopback_port() {
        let directory = tempfile::tempdir().unwrap();
        let root = Some(directory.path());
        assert!(renderer_arguments(DesktopHarnessKind::Pen, false, root, Some("43210")).is_empty());
        assert!(renderer_arguments(DesktopHarnessKind::Pen, true, None, Some("43210")).is_empty());
        for port in ["0", "1024", "65536", "43210 --no-sandbox", "*:43210"] {
            assert!(renderer_arguments(DesktopHarnessKind::Pen, true, root, Some(port)).is_empty());
        }
        assert_eq!(
            renderer_arguments(DesktopHarnessKind::Claude, true, root, Some("43210")),
            [
                "--remote-debugging-port=43210",
                "--remote-debugging-address=127.0.0.1"
            ]
        );
    }
}

/// A qualification-only native Electron profile override; ordinary launches are unchanged.
#[cfg(feature = "desktop-qualification")]
pub(crate) fn claude_profile_arguments(executable: Option<&Path>) -> std::io::Result<Vec<String>> {
    claude_profile::arguments(executable)
}
