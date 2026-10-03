//! Owned default-native storage for a disposable hosted macOS acquisition trial.
use super::{Gui, ProbeSpec, Reason};
use nan_harness_core::DesktopHarnessKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum Stage {
    ProcessAbsence,
    FoundationQuery,
    NativeAlignment,
    RootsAbsent,
    RootsCreated,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum Failure {
    QueryFailed,
    ProcessPresent,
    AlignmentMismatch,
    ExistingRoot,
    CreationFailed,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Preflight {
    schema_version: u8,
    mechanism: &'static str,
    diagnostics_only: bool,
    stage: Stage,
    failure: Option<Failure>,
}
fn record(stage: Stage, failure: Option<Failure>) {
    let Some(directory) = std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS").map(PathBuf::from)
    else {
        return;
    };
    if identity(&directory).is_none() {
        return;
    }
    let facts = Preflight {
        schema_version: 1,
        mechanism: "claude-native-root-preflight",
        diagnostics_only: true,
        stage,
        failure,
    };
    let path = directory.join(format!(
        "claude-native-root-preflight-{}.json",
        std::process::id()
    ));
    if let Ok(file) = nan_harness_private_fs::open_private_new(&path) {
        let _ = serde_json::to_writer(file, &facts);
    }
}
fn reject(stage: Stage, failure: Failure) -> Reason {
    record(stage, Some(failure));
    Reason::IsolationUnavailable
}
fn process_exit(code: Option<i32>) -> Result<(), Failure> {
    match code {
        Some(1) => Ok(()),
        Some(0) => Err(Failure::ProcessPresent),
        _ => Err(Failure::QueryFailed),
    }
}

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

pub(crate) struct NativeRoots {
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
fn require_process_absence() -> Result<(), Failure> {
    let mut child = std::process::Command::new("/usr/bin/pgrep")
        .args(["-f", "Claude.app/Contents/MacOS/Claude"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Failure::QueryFailed)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return process_exit(status.code());
            }
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Failure::QueryFailed);
            }
        }
    }
}

impl NativeRoots {
    #[cfg(any(test, target_os = "macos"))]
    pub(crate) fn verifies_created_roots(&self) -> bool {
        self.roots.len() == 2
            && self
                .roots
                .iter()
                .all(|root| identity(&root.path) == Some(root.identity))
    }
    pub(super) async fn prepare(spec: &ProbeSpec) -> Result<Option<Self>, Reason> {
        if !enabled(spec)? {
            return Ok(None);
        }
        require_process_absence().map_err(|failure| reject(Stage::ProcessAbsence, failure))?;
        Gui::ensure_absent(DesktopHarnessKind::Claude).map_err(|failure| {
            reject(
                Stage::ProcessAbsence,
                if failure.reason == Reason::AlreadyRunning {
                    Failure::ProcessPresent
                } else {
                    Failure::QueryFailed
                },
            )
        })?;
        let home = PathBuf::from(
            std::env::var_os("HOME")
                .ok_or_else(|| reject(Stage::FoundationQuery, Failure::QueryFailed))?,
        );
        let support = home.join("Library/Application Support");
        let native = crate::native::Native::new()
            .map_err(|_| reject(Stage::FoundationQuery, Failure::QueryFailed))?;
        let valid = native.claude_known_folders(&home).await;
        if valid.is_none() {
            return Err(reject(Stage::FoundationQuery, Failure::QueryFailed));
        }
        if valid != Some(true) || support.canonicalize().ok().as_deref() != Some(&support) {
            return Err(reject(Stage::NativeAlignment, Failure::AlignmentMismatch));
        }
        for name in ["Claude", "Claude-3p"] {
            match std::fs::symlink_metadata(support.join(name)) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_) => return Err(reject(Stage::RootsAbsent, Failure::ExistingRoot)),
                Err(_) => return Err(reject(Stage::RootsAbsent, Failure::QueryFailed)),
            }
        }
        let roots = Self::create(&support)
            .inspect_err(|_| record(Stage::RootsCreated, Some(Failure::CreationFailed)))?;
        record(Stage::RootsCreated, None);
        Ok(Some(roots))
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
    fn distinguishes_process_presence_from_query_failure_and_keeps_receipts_closed() {
        assert_eq!(process_exit(Some(1)), Ok(()));
        assert_eq!(process_exit(Some(0)), Err(Failure::ProcessPresent));
        assert_eq!(process_exit(Some(2)), Err(Failure::QueryFailed));
        assert_eq!(process_exit(None), Err(Failure::QueryFailed));
        let value = serde_json::to_value(Preflight {
            schema_version: 1,
            mechanism: "claude-native-root-preflight",
            diagnostics_only: true,
            stage: Stage::FoundationQuery,
            failure: Some(Failure::QueryFailed),
        })
        .unwrap();
        assert_eq!(value["stage"], "foundation-query");
        assert_eq!(value["failure"], "query-failed");
        assert_eq!(value.as_object().unwrap().len(), 5);
        assert!(value.get("path").is_none());
        assert!(value.get("pid").is_none());
    }
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
    fn rejects_missing_or_nonprivate_owned_roots() {
        use std::os::unix::fs::PermissionsExt as _;
        let temp = tempfile::tempdir().unwrap();
        let support = temp.path().canonicalize().unwrap();
        let mut roots = NativeRoots::create(&support).unwrap();
        assert!(roots.verifies_created_roots());
        let path = roots.roots[0].path.clone();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!roots.verifies_created_roots());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(roots.verifies_created_roots());
        std::fs::remove_dir(&path).unwrap();
        assert!(!roots.verifies_created_roots());
        roots.roots.clear();
    }
    #[test]
    fn detects_replaced_owned_root() {
        let temp = tempfile::tempdir().unwrap();
        let mut roots = NativeRoots::create(&temp.path().canonicalize().unwrap()).unwrap();
        assert!(roots.verifies_created_roots());
        let path = roots.roots[0].path.clone();
        std::fs::rename(&path, temp.path().join("original")).unwrap();
        nan_harness_private_fs::create_private_dir(&path).unwrap();
        assert_ne!(identity(&path), Some(roots.roots[0].identity));
        assert!(!roots.verifies_created_roots());
        roots.roots.clear();
    }
}
