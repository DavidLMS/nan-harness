use super::TemporaryError;
use super::overlays::materialize_overlay;
use super::paths::{
    ensure_mode, invalid_artifact, render_user_home, resolve_overlay_source, user_home,
    validate_path_hint,
};
use super::platform::restrict_directory;
use nan_harness_core::launch_plan::{
    ConfigurationOverlay, LaunchScopedFile, TemporaryArtifact, TemporaryArtifactKind,
    TemporaryArtifactMode,
};
use nan_harness_private_fs::{create_private_dir_all, open_private_new, open_private_read};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::ErrorKind;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

pub(super) const SCOPED_FILES_LOCK_NAME: &str = ".nan-harness-scoped-files.lock";
const SCOPED_FILES_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const SCOPED_FILES_LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScopedFileLifecycleEvent {
    BeforeDirectoryLock,
    BeforeSessionLockCreate,
    SessionLockPublished,
    BeforeProfileCreate,
}

pub struct TemporaryWorkspace {
    root: TempDir,
    paths: BTreeMap<String, PathBuf>,
    user_home: PathBuf,
    _scoped_files: Vec<LaunchScopedFileGuard>,
}

impl TemporaryWorkspace {
    /// Creates a private workspace and materializes all declared artifacts.
    ///
    /// # Errors
    ///
    /// Returns [`TemporaryError`] when an artifact is unsafe or cannot be created privately.
    pub fn materialize(artifacts: &[TemporaryArtifact]) -> Result<Self, TemporaryError> {
        Self::materialize_with(artifacts, &[], &[], |_, content| Ok(content.to_owned()))
    }

    pub(crate) fn materialize_with(
        artifacts: &[TemporaryArtifact],
        overlays: &[ConfigurationOverlay],
        scoped_files: &[LaunchScopedFile],
        render: impl Fn(&str, &str) -> Result<String, TemporaryError>,
    ) -> Result<Self, TemporaryError> {
        Self::materialize_with_home_and_scoped(
            artifacts,
            overlays,
            scoped_files,
            &user_home()?,
            render,
        )
    }

    #[cfg(test)]
    pub(super) fn materialize_with_home(
        artifacts: &[TemporaryArtifact],
        overlays: &[ConfigurationOverlay],
        user_home: &Path,
        render: impl Fn(&str, &str) -> Result<String, TemporaryError>,
    ) -> Result<Self, TemporaryError> {
        Self::materialize_with_home_and_scoped(artifacts, overlays, &[], user_home, render)
    }

    pub(super) fn materialize_with_home_and_scoped(
        artifacts: &[TemporaryArtifact],
        overlays: &[ConfigurationOverlay],
        scoped_file_specs: &[LaunchScopedFile],
        user_home: &Path,
        render: impl Fn(&str, &str) -> Result<String, TemporaryError>,
    ) -> Result<Self, TemporaryError> {
        Self::materialize_with_home_and_scoped_observing(
            artifacts,
            overlays,
            scoped_file_specs,
            user_home,
            render,
            |_| {},
        )
    }

    pub(super) fn materialize_with_home_and_scoped_observing(
        artifacts: &[TemporaryArtifact],
        overlays: &[ConfigurationOverlay],
        scoped_file_specs: &[LaunchScopedFile],
        user_home: &Path,
        render: impl Fn(&str, &str) -> Result<String, TemporaryError>,
        observe: impl Fn(ScopedFileLifecycleEvent),
    ) -> Result<Self, TemporaryError> {
        let root = tempfile::Builder::new()
            .prefix("nan-harness-")
            .tempdir()
            .map_err(TemporaryError::CreateWorkspace)?;
        restrict_directory(root.path())?;
        let user_home = user_home.to_path_buf();
        let codex_home = std::env::var_os("CODEX_HOME");
        let mut paths = BTreeMap::new();
        let mut scoped_files = Vec::new();

        for overlay in overlays {
            validate_path_hint(&overlay.id, &overlay.path_hint)?;
            let path = root.path().join(&overlay.path_hint);
            let source =
                resolve_overlay_source(&overlay.source_path, &user_home, codex_home.as_deref());
            materialize_overlay(overlay, &source, &path, &render, &user_home)?;
            paths.insert(overlay.id.clone(), path);
        }

        for artifact in artifacts {
            validate_path_hint(&artifact.id, &artifact.path_hint)?;
            let path = root.path().join(&artifact.path_hint);
            match artifact.kind {
                TemporaryArtifactKind::File => {
                    let content = artifact
                        .content_template
                        .as_deref()
                        .ok_or_else(|| invalid_artifact(&artifact.id, "file content is missing"))?;
                    let rendered = render(&artifact.id, content)?;
                    ensure_mode(
                        &artifact.id,
                        artifact.mode,
                        TemporaryArtifactMode::OwnerFile,
                    )?;
                    let mut file =
                        open_private_new(&path).map_err(|source| TemporaryError::Materialize {
                            artifact_id: artifact.id.clone(),
                            source,
                        })?;
                    file.write_all(render_user_home(&rendered, &user_home).as_bytes())
                        .map_err(|source| TemporaryError::Materialize {
                            artifact_id: artifact.id.clone(),
                            source,
                        })?;
                }
                TemporaryArtifactKind::Directory => {
                    ensure_mode(
                        &artifact.id,
                        artifact.mode,
                        TemporaryArtifactMode::OwnerDirectory,
                    )?;
                    fs::create_dir(&path).map_err(|source| TemporaryError::Materialize {
                        artifact_id: artifact.id.clone(),
                        source,
                    })?;
                    restrict_directory(&path)?;
                }
            }
            paths.insert(artifact.id.clone(), path);
        }
        for scoped_file in scoped_file_specs {
            let directory =
                resolve_overlay_source(&scoped_file.directory, &user_home, codex_home.as_deref());
            let content = render(&scoped_file.id, &scoped_file.content_template)?;
            let guard = materialize_launch_scoped_file(
                scoped_file,
                &directory,
                &render_user_home(&content, &user_home),
                &observe,
            )?;
            paths.insert(scoped_file.id.clone(), guard.owned_path.clone());
            scoped_files.push(guard);
        }
        Ok(Self {
            root,
            paths,
            user_home,
            _scoped_files: scoped_files,
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        self.root.path()
    }

    #[must_use]
    pub fn path(&self, artifact_id: &str) -> Option<&Path> {
        self.paths.get(artifact_id).map(PathBuf::as_path)
    }

    #[must_use]
    pub(crate) fn user_home(&self) -> &Path {
        &self.user_home
    }
}

struct LaunchScopedFileGuard {
    directory: PathBuf,
    owned_path: PathBuf,
    owned_lock_path: PathBuf,
    lock_file: Option<File>,
}

impl Drop for LaunchScopedFileGuard {
    fn drop(&mut self) {
        let Ok(_directory_lock) = acquire_scoped_files_lock(&self.directory) else {
            return;
        };
        let _ = fs::remove_file(&self.owned_path);
        if let Some(lock_file) = self.lock_file.take() {
            let _ = File::unlock(&lock_file);
            drop(lock_file);
        }
        let _ = fs::remove_file(&self.owned_lock_path);
    }
}

struct ScopedFilesLock {
    file: File,
}

impl Drop for ScopedFilesLock {
    fn drop(&mut self) {
        let _ = File::unlock(&self.file);
    }
}

fn materialize_launch_scoped_file(
    spec: &LaunchScopedFile,
    directory: &Path,
    content: &str,
    observe: &impl Fn(ScopedFileLifecycleEvent),
) -> Result<LaunchScopedFileGuard, TemporaryError> {
    ensure_mode(&spec.id, spec.mode, TemporaryArtifactMode::OwnerFile)?;
    ensure_configuration_directory(directory, &spec.id)?;
    observe(ScopedFileLifecycleEvent::BeforeDirectoryLock);
    let _directory_lock =
        acquire_scoped_files_lock(directory).map_err(|source| TemporaryError::Materialize {
            artifact_id: spec.id.clone(),
            source,
        })?;
    cleanup_orphaned_scoped_files(directory, &spec.ownership_prefix);

    let path = directory.join(&spec.file_name);
    let lock_path = directory.join(format!("{}.lock", spec.file_name));
    publish_launch_scoped_file(directory, path, lock_path, content, observe).map_err(|source| {
        TemporaryError::Materialize {
            artifact_id: spec.id.clone(),
            source,
        }
    })
}

fn publish_launch_scoped_file(
    directory: &Path,
    path: PathBuf,
    lock_path: PathBuf,
    content: &str,
    observe: &impl Fn(ScopedFileLifecycleEvent),
) -> std::io::Result<LaunchScopedFileGuard> {
    observe(ScopedFileLifecycleEvent::BeforeSessionLockCreate);
    let lock_file = open_private_new(&lock_path)?;
    observe(ScopedFileLifecycleEvent::SessionLockPublished);
    if let Err(error) = File::lock(&lock_file) {
        drop(lock_file);
        let _ = fs::remove_file(&lock_path);
        return Err(error);
    }
    observe(ScopedFileLifecycleEvent::BeforeProfileCreate);
    let mut file = match open_private_new(&path) {
        Ok(file) => file,
        Err(error) => {
            remove_owned_session_lock(lock_file, &lock_path);
            return Err(error);
        }
    };
    if let Err(error) = file
        .write_all(content.as_bytes())
        .and_then(|()| file.sync_data())
    {
        drop(file);
        let _ = fs::remove_file(&path);
        remove_owned_session_lock(lock_file, &lock_path);
        return Err(error);
    }

    Ok(LaunchScopedFileGuard {
        directory: directory.to_path_buf(),
        owned_path: path,
        owned_lock_path: lock_path,
        lock_file: Some(lock_file),
    })
}

fn remove_owned_session_lock(lock_file: File, lock_path: &Path) {
    let _ = File::unlock(&lock_file);
    drop(lock_file);
    let _ = fs::remove_file(lock_path);
}

fn acquire_scoped_files_lock(directory: &Path) -> std::io::Result<ScopedFilesLock> {
    // This path stays published so every process coordinates through the same inode.
    let path = directory.join(SCOPED_FILES_LOCK_NAME);
    let file = match open_private_new(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            open_existing_scoped_files_lock(&path)?
        }
        Err(error) => return Err(error),
    };
    let started = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(ScopedFilesLock { file }),
            Err(TryLockError::WouldBlock) => {
                let remaining = SCOPED_FILES_LOCK_TIMEOUT.saturating_sub(started.elapsed());
                if remaining.is_zero() {
                    return Err(std::io::Error::new(
                        ErrorKind::TimedOut,
                        "timed out waiting for launch-scoped file coordination",
                    ));
                }
                thread::sleep(SCOPED_FILES_LOCK_RETRY_INTERVAL.min(remaining));
            }
            Err(TryLockError::Error(error)) => return Err(error),
        }
    }
}

fn open_existing_scoped_files_lock(path: &Path) -> std::io::Result<File> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(std::io::Error::new(
            ErrorKind::InvalidInput,
            "launch-scoped coordination lock is not a regular file",
        ));
    }
    open_private_read(path).map(|(file, _status)| file)
}

pub(super) fn ensure_configuration_directory(
    path: &Path,
    artifact_id: &str,
) -> Result<(), TemporaryError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(invalid_artifact(
            artifact_id,
            format!(
                "configuration directory '{}' is not a directory",
                path.display()
            ),
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            create_private_dir_all(path).map_err(|source| TemporaryError::Materialize {
                artifact_id: artifact_id.to_owned(),
                source,
            })
        }
        Err(source) => Err(TemporaryError::Materialize {
            artifact_id: artifact_id.to_owned(),
            source,
        }),
    }
}

fn cleanup_orphaned_scoped_files(directory: &Path, ownership_prefix: &str) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let names = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let file_type = entry.file_type().ok()?;
            (file_type.is_file()
                && name != SCOPED_FILES_LOCK_NAME
                && name.starts_with(ownership_prefix))
            .then_some(name)
        })
        .collect::<Vec<_>>();

    for name in names.iter().filter(|name| !has_lock_extension(name)) {
        let path = directory.join(name);
        let lock_path = directory.join(format!("{name}.lock"));
        reclaim_orphaned_profile(&path, &lock_path);
    }
    for name in names.iter().filter(|name| has_lock_extension(name)) {
        let Some(profile_name) = name.strip_suffix(".lock") else {
            continue;
        };
        if !matches!(
            fs::symlink_metadata(directory.join(profile_name)),
            Err(error) if error.kind() == ErrorKind::NotFound
        ) {
            continue;
        }
        reclaim_orphaned_lock(&directory.join(name));
    }
}

fn has_lock_extension(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("lock"))
}

fn reclaim_orphaned_profile(path: &Path, lock_path: &Path) {
    let lock_file = match open_regular_file(lock_path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            // Publication always creates its session lock first under directory coordination.
            let _ = fs::remove_file(path);
            return;
        }
        Err(_) => return,
    };
    if lock_file.try_lock().is_err() {
        return;
    }
    let profile_removed = match fs::remove_file(path) {
        Ok(()) => true,
        Err(error) => error.kind() == ErrorKind::NotFound,
    };
    let _ = File::unlock(&lock_file);
    drop(lock_file);
    if profile_removed {
        let _ = fs::remove_file(lock_path);
    }
}

fn reclaim_orphaned_lock(lock_path: &Path) {
    let Ok(lock_file) = open_regular_file(lock_path) else {
        return;
    };
    if lock_file.try_lock().is_err() {
        return;
    }
    let _ = File::unlock(&lock_file);
    drop(lock_file);
    let _ = fs::remove_file(lock_path);
}

fn open_regular_file(path: &Path) -> std::io::Result<File> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(std::io::Error::new(
            ErrorKind::InvalidInput,
            "launch-scoped lock is not a regular file",
        ));
    }
    let file = OpenOptions::new().read(true).open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            ErrorKind::InvalidInput,
            "launch-scoped lock handle is not a regular file",
        ));
    }
    Ok(file)
}
