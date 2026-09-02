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
use nan_harness_private_fs::{create_private_dir_all, open_private_new};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::ErrorKind;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

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
            )?;
            paths.insert(scoped_file.id.clone(), guard.path.clone());
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
    path: PathBuf,
    lock_path: PathBuf,
    lock_file: Option<File>,
}

impl Drop for LaunchScopedFileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        if let Some(lock_file) = self.lock_file.take() {
            let _ = File::unlock(&lock_file);
            drop(lock_file);
        }
        let _ = fs::remove_file(&self.lock_path);
    }
}

fn materialize_launch_scoped_file(
    spec: &LaunchScopedFile,
    directory: &Path,
    content: &str,
) -> Result<LaunchScopedFileGuard, TemporaryError> {
    ensure_mode(&spec.id, spec.mode, TemporaryArtifactMode::OwnerFile)?;
    ensure_configuration_directory(directory, &spec.id)?;
    cleanup_orphaned_scoped_files(directory, &spec.ownership_prefix);

    let path = directory.join(&spec.file_name);
    let lock_path = directory.join(format!("{}.lock", spec.file_name));
    let lock_file = open_private_new(&lock_path).map_err(|source| TemporaryError::Materialize {
        artifact_id: spec.id.clone(),
        source,
    })?;
    let guard = LaunchScopedFileGuard {
        path: path.clone(),
        lock_path,
        lock_file: Some(lock_file),
    };
    let lock_file = guard
        .lock_file
        .as_ref()
        .ok_or_else(|| TemporaryError::Materialize {
            artifact_id: spec.id.clone(),
            source: std::io::Error::other("launch-scoped lock file is missing"),
        })?;
    File::lock(lock_file).map_err(|source| TemporaryError::Materialize {
        artifact_id: spec.id.clone(),
        source,
    })?;
    let mut file = open_private_new(&path).map_err(|source| TemporaryError::Materialize {
        artifact_id: spec.id.clone(),
        source,
    })?;
    file.write_all(content.as_bytes())
        .map_err(|source| TemporaryError::Materialize {
            artifact_id: spec.id.clone(),
            source,
        })?;
    file.sync_data()
        .map_err(|source| TemporaryError::Materialize {
            artifact_id: spec.id.clone(),
            source,
        })?;
    Ok(guard)
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
            (file_type.is_file() && name.starts_with(ownership_prefix)).then_some(name)
        })
        .collect::<Vec<_>>();

    for name in names.iter().filter(|name| !has_lock_extension(name)) {
        let path = directory.join(name);
        let lock_path = directory.join(format!("{name}.lock"));
        if scoped_lock_is_active(&lock_path) {
            continue;
        }
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(lock_path);
    }
    for name in names.iter().filter(|name| has_lock_extension(name)) {
        let Some(profile_name) = name.strip_suffix(".lock") else {
            continue;
        };
        if directory.join(profile_name).exists() {
            continue;
        }
        let lock_path = directory.join(name);
        if !scoped_lock_is_active(&lock_path) {
            let _ = fs::remove_file(lock_path);
        }
    }
}

fn has_lock_extension(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("lock"))
}

fn scoped_lock_is_active(path: &Path) -> bool {
    let Ok(file) = OpenOptions::new().read(true).write(true).open(path) else {
        return false;
    };
    match file.try_lock() {
        Ok(()) => {
            let _ = File::unlock(&file);
            false
        }
        Err(TryLockError::WouldBlock | TryLockError::Error(_)) => true,
    }
}
