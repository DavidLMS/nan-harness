//! Owned default-native storage for a disposable hosted macOS acquisition trial.
use super::{Gui, ProbeSpec, Reason};
use nan_harness_core::DesktopHarnessKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncReadExt as _;

pub(super) fn enabled(spec: &ProbeSpec) -> Result<bool, Reason> {
    if std::env::var("NANH_CLAUDE_MAC_PROFILE_POLICY").as_deref() != Ok("native-known-folders") {
        return Ok(false);
    }
    if !cfg!(target_os = "macos")
        || spec.kind != DesktopHarnessKind::Claude
        || spec.session != crate::cli::SessionMode::GithubHosted
        || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        || std::env::var("RUNNER_OS").as_deref() != Ok("macOS")
        || std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() != Ok("startup-baseline")
    {
        return Err(Reason::IsolationUnavailable);
    }
    Ok(true)
}

pub(super) struct NativeRoots {
    roots: Vec<OwnedRoot>,
}
struct OwnedRoot {
    path: PathBuf,
    identity: (u64, u64),
}
fn raw_identity(path: &Path) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let metadata = std::fs::symlink_metadata(path).ok()?;
        if !metadata.is_dir() || path.canonicalize().ok().as_deref() != Some(path) {
            return None;
        }
        Some((metadata.dev(), metadata.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}
fn identity(path: &Path) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        let metadata = std::fs::symlink_metadata(path).ok()?;
        if !metadata.is_dir()
            || metadata.permissions().mode() & 0o077 != 0
            || path.canonicalize().ok().as_deref() != Some(path)
        {
            return None;
        }
        Some((metadata.dev(), metadata.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}
fn require_process_absence() -> Result<(), Reason> {
    let mut child = std::process::Command::new("/usr/bin/pgrep")
        .args(["-f", "Claude.app/Contents/MacOS/Claude"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Reason::IsolationUnavailable)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.code() == Some(1) {
                    Ok(())
                } else {
                    Err(Reason::IsolationUnavailable)
                };
            }
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Reason::IsolationUnavailable);
            }
        }
    }
}

impl NativeRoots {
    pub(super) async fn prepare(spec: &ProbeSpec) -> Result<Option<Self>, Reason> {
        if !enabled(spec)? {
            return Ok(None);
        }
        require_process_absence()?;
        Gui::ensure_absent(DesktopHarnessKind::Claude).map_err(|_| Reason::IsolationUnavailable)?;
        let home = PathBuf::from(std::env::var_os("HOME").ok_or(Reason::IsolationUnavailable)?);
        let support = home.join("Library/Application Support");
        let script = "import Foundation\nlet home = FileManager.default.homeDirectoryForCurrentUser.path\nlet support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first?.path\nlet expected = ProcessInfo.processInfo.environment[\"HOME\"]\nprint(home == expected && support == expected.map { $0 + \"/Library/Application Support\" } ? \"true\" : \"false\")";
        let mut child = tokio::process::Command::new("/usr/bin/swift")
            .args(["-e", script])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| Reason::IsolationUnavailable)?;
        let stdout = child.stdout.take().ok_or(Reason::IsolationUnavailable)?;
        let valid = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut bytes = Vec::new();
            stdout.take(16).read_to_end(&mut bytes).await.ok()?;
            Some(child.wait().await.ok()?.success() && bytes == b"true\n")
        })
        .await
        .ok()
        .flatten()
            == Some(true);
        if !valid || support.canonicalize().ok().as_deref() != Some(&support) {
            return Err(Reason::IsolationUnavailable);
        }
        Ok(Some(Self::create(&support)?))
    }
    fn create(support: &Path) -> Result<Self, Reason> {
        let paths = [support.join("Claude"), support.join("Claude-3p")];
        if paths.iter().any(|p| !matches!(std::fs::symlink_metadata(p), Err(e) if e.kind() == std::io::ErrorKind::NotFound)) {
            return Err(Reason::IsolationUnavailable);
        }
        let mut owner = Self { roots: Vec::new() };
        for path in paths {
            if let Err(reason) = owner.create_one(&path) {
                owner.rollback_empty()?;
                return Err(reason);
            }
        }
        Ok(owner)
    }
    fn create_one(&mut self, path: &Path) -> Result<(), Reason> {
        std::fs::create_dir(path).map_err(|_| Reason::IsolationUnavailable)?;
        let created = raw_identity(path).ok_or(Reason::CleanupFailed)?;
        self.roots.push(OwnedRoot {
            path: path.to_path_buf(),
            identity: created,
        });
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
            let directory = std::fs::File::open(path).map_err(|_| Reason::IsolationUnavailable)?;
            let metadata = directory
                .metadata()
                .map_err(|_| Reason::IsolationUnavailable)?;
            if (metadata.dev(), metadata.ino()) != created {
                return Err(Reason::IsolationUnavailable);
            }
            directory
                .set_permissions(std::fs::Permissions::from_mode(0o700))
                .map_err(|_| Reason::IsolationUnavailable)?;
        }
        if identity(path) != Some(created) {
            return Err(Reason::IsolationUnavailable);
        }
        Ok(())
    }
    fn rollback_empty(&mut self) -> Result<(), Reason> {
        for root in &self.roots {
            if raw_identity(&root.path) != Some(root.identity) {
                return Err(Reason::CleanupFailed);
            }
            std::fs::remove_dir(&root.path).map_err(|_| Reason::CleanupFailed)?;
        }
        self.roots.clear();
        Ok(())
    }
    pub(super) fn cleanup(&mut self) -> Result<(), Reason> {
        require_process_absence().map_err(|_| Reason::CleanupFailed)?;
        Gui::ensure_absent(DesktopHarnessKind::Claude).map_err(|_| Reason::CleanupFailed)?;
        for root in &self.roots {
            if identity(&root.path) != Some(root.identity)
                || root.path.join("claude_desktop_config.json").exists()
                || root.path.join("configLibrary/_meta.json").exists()
                || root
                    .path
                    .join("configLibrary/6e616e68-6172-4e65-8000-000000000001.json")
                    .exists()
            {
                return Err(Reason::CleanupFailed);
            }
        }
        for root in &self.roots {
            std::fs::remove_dir_all(&root.path).map_err(|_| Reason::CleanupFailed)?;
        }
        self.roots.clear();
        Ok(())
    }
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn refuses_existing_roots_without_adopting_them() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("Claude-3p")).unwrap();
        assert!(NativeRoots::create(temp.path()).is_err());
        assert!(!temp.path().join("Claude").exists());
    }
    #[test]
    fn rolls_back_only_empty_created_roots_and_reports_retained_state() {
        let temp = tempfile::tempdir().unwrap();
        let support = temp.path().canonicalize().unwrap();
        let mut roots = NativeRoots::create(&support).unwrap();
        let retained = roots.roots[0].path.join("unexpected-state");
        std::fs::write(&retained, b"synthetic").unwrap();
        assert_eq!(roots.rollback_empty(), Err(Reason::CleanupFailed));
        assert!(retained.exists());
        std::fs::remove_file(retained).unwrap();
        roots.rollback_empty().unwrap();
        assert!(!support.join("Claude").exists());
        assert!(!support.join("Claude-3p").exists());
    }
    #[test]
    fn detects_replaced_owned_root() {
        let temp = tempfile::tempdir().unwrap();
        let mut roots = NativeRoots::create(&temp.path().canonicalize().unwrap()).unwrap();
        let path = roots.roots[0].path.clone();
        std::fs::rename(&path, temp.path().join("original")).unwrap();
        nan_harness_private_fs::create_private_dir(&path).unwrap();
        assert_ne!(identity(&path), Some(roots.roots[0].identity));
        roots.roots.clear();
    }
}
