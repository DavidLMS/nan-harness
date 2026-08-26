use nan_harness_core::launch_plan::{
    CODEX_HOME_PLACEHOLDER, ConfigurationOverlay, LaunchScopedFile, OverlayFilePolicy,
    TemporaryArtifact, TemporaryArtifactKind, TemporaryArtifactMode, USER_HOME_PLACEHOLDER,
};
use nan_harness_private_fs::{PrivatePathKind, open_private_new, restrict_path};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::ErrorKind;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use tempfile::TempDir;
use thiserror::Error;

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
    fn materialize_with_home(
        artifacts: &[TemporaryArtifact],
        overlays: &[ConfigurationOverlay],
        user_home: &Path,
        render: impl Fn(&str, &str) -> Result<String, TemporaryError>,
    ) -> Result<Self, TemporaryError> {
        Self::materialize_with_home_and_scoped(artifacts, overlays, &[], user_home, render)
    }

    fn materialize_with_home_and_scoped(
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

fn ensure_configuration_directory(path: &Path, artifact_id: &str) -> Result<(), TemporaryError> {
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
            fs::create_dir_all(path).map_err(|source| TemporaryError::Materialize {
                artifact_id: artifact_id.to_owned(),
                source,
            })?;
            restrict_directory(path)
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

#[derive(Debug, Error)]
pub enum TemporaryError {
    #[error("could not create a private temporary workspace: {0}")]
    CreateWorkspace(std::io::Error),
    #[error("could not resolve the current user's home directory")]
    MissingUserHome,
    #[error("temporary artifact '{artifact_id}' is invalid: {reason}")]
    InvalidArtifact { artifact_id: String, reason: String },
    #[error("could not materialize temporary artifact '{artifact_id}': {source}")]
    Materialize {
        artifact_id: String,
        #[source]
        source: std::io::Error,
    },
    #[error("could not mirror configuration overlay '{overlay_id}': {source}")]
    MirrorOverlay {
        overlay_id: String,
        #[source]
        source: std::io::Error,
    },
    #[error("could not set private permissions on '{}': {source}", path.display())]
    Permissions {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

fn materialize_overlay(
    overlay: &ConfigurationOverlay,
    source: &Path,
    target: &Path,
    render: &impl Fn(&str, &str) -> Result<String, TemporaryError>,
    user_home: &Path,
) -> Result<(), TemporaryError> {
    let replacements = overlay
        .files
        .iter()
        .filter(|file| {
            file.policy != OverlayFilePolicy::Preserve || !path_exists(&source.join(&file.path))
        })
        .map(|file| PathBuf::from(&file.path))
        .collect::<BTreeSet<_>>();
    mirror_directory(source, target, Path::new(""), &replacements, &overlay.id)?;

    for file in &overlay.files {
        let path = target.join(&file.path);
        if file.policy == OverlayFilePolicy::Preserve && path_exists(&path) {
            continue;
        }
        let source_path = source.join(&file.path);
        if file.policy == OverlayFilePolicy::CopyBinary {
            if !path_exists(&source_path) {
                continue;
            }
            ensure_mode(&overlay.id, file.mode, TemporaryArtifactMode::OwnerFile)?;
            create_private_parents(target, path.parent(), &overlay.id)?;
            let mut source_file =
                File::open(&source_path).map_err(|source| TemporaryError::Materialize {
                    artifact_id: overlay.id.clone(),
                    source,
                })?;
            let mut target_file =
                open_private_new(&path).map_err(|source| TemporaryError::Materialize {
                    artifact_id: overlay.id.clone(),
                    source,
                })?;
            std::io::copy(&mut source_file, &mut target_file).map_err(|source| {
                TemporaryError::Materialize {
                    artifact_id: overlay.id.clone(),
                    source,
                }
            })?;
            continue;
        }
        ensure_mode(&overlay.id, file.mode, TemporaryArtifactMode::OwnerFile)?;
        create_private_parents(target, path.parent(), &overlay.id)?;
        let content = overlay_file_content(overlay, file, &source_path, &path, render, user_home)?;
        let mut target_file =
            open_private_new(&path).map_err(|source| TemporaryError::Materialize {
                artifact_id: overlay.id.clone(),
                source,
            })?;
        target_file
            .write_all(content.as_bytes())
            .map_err(|source| TemporaryError::Materialize {
                artifact_id: overlay.id.clone(),
                source,
            })?;
    }
    Ok(())
}

fn overlay_file_content(
    overlay: &ConfigurationOverlay,
    file: &nan_harness_core::launch_plan::OverlayFile,
    source_path: &Path,
    target_path: &Path,
    render: &impl Fn(&str, &str) -> Result<String, TemporaryError>,
    user_home: &Path,
) -> Result<String, TemporaryError> {
    if file.policy == OverlayFilePolicy::Copy && path_exists(source_path) {
        return fs::read_to_string(source_path)
            .map_err(|source| overlay_error(&overlay.id, source));
    }
    let rendered = render(&overlay.id, &file.content_template)?;
    let rendered = render_user_home(&rendered, user_home);
    match file.policy {
        OverlayFilePolicy::MergeJson => {
            let mut base = if path_exists(source_path) {
                let content = fs::read_to_string(source_path)
                    .map_err(|source| overlay_error(&overlay.id, source))?;
                parse_json_object(&overlay.id, "source", &content)?
            } else {
                serde_json::Map::new()
            };
            let patch = parse_json_object(&overlay.id, "patch", &rendered)?;
            merge_json_objects(&mut base, patch);
            serde_json::to_string_pretty(&serde_json::Value::Object(base)).map_err(|error| {
                invalid_artifact(
                    &overlay.id,
                    format!("could not serialize merged JSON overlay: {error}"),
                )
            })
        }
        OverlayFilePolicy::MergeToml => {
            let mut base = if path_exists(source_path) {
                let content = fs::read_to_string(source_path)
                    .map_err(|source| overlay_error(&overlay.id, source))?;
                parse_toml_table(&overlay.id, "source", &content)?
            } else {
                toml::Table::new()
            };
            let patch = parse_toml_table(&overlay.id, "patch", &rendered)?;
            merge_toml_tables(&mut base, patch);
            relocate_hook_state_keys(&mut base, source_path, target_path);
            toml::to_string(&toml::Value::Table(base)).map_err(|error| {
                invalid_artifact(
                    &overlay.id,
                    format!("could not serialize merged TOML overlay: {error}"),
                )
            })
        }
        OverlayFilePolicy::Replace
        | OverlayFilePolicy::Preserve
        | OverlayFilePolicy::Copy
        | OverlayFilePolicy::CopyBinary => Ok(rendered),
    }
}

fn relocate_hook_state_keys(config: &mut toml::Table, source_path: &Path, target_path: &Path) {
    let Some(state) = config
        .get_mut("hooks")
        .and_then(toml::Value::as_table_mut)
        .and_then(|hooks| hooks.get_mut("state"))
        .and_then(toml::Value::as_table_mut)
    else {
        return;
    };
    let Some(source_root) = source_path.parent() else {
        return;
    };
    let Some(target_root) = target_path.parent() else {
        return;
    };
    let source_prefix = format!("{}:", source_root.join("hooks.json").display());
    let mut target_prefixes =
        BTreeSet::from([format!("{}:", target_root.join("hooks.json").display())]);
    if let Ok(canonical_target_root) = fs::canonicalize(target_root) {
        target_prefixes.insert(format!(
            "{}:",
            canonical_target_root.join("hooks.json").display()
        ));
    }
    let keys = state.keys().cloned().collect::<Vec<_>>();
    for key in keys {
        let Some(suffix) = key.strip_prefix(&source_prefix) else {
            continue;
        };
        if let Some(value) = state.get(&key).cloned() {
            for target_prefix in &target_prefixes {
                state.insert(format!("{target_prefix}{suffix}"), value.clone());
            }
        }
    }
}

fn parse_json_object(
    overlay_id: &str,
    label: &str,
    content: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, TemporaryError> {
    let value: serde_json::Value = serde_json::from_str(content).map_err(|error| {
        invalid_artifact(
            overlay_id,
            format!("{label} JSON overlay is invalid: {error}"),
        )
    })?;
    value.as_object().cloned().ok_or_else(|| {
        invalid_artifact(
            overlay_id,
            format!("{label} JSON overlay must be an object"),
        )
    })
}

fn merge_json_objects(
    target: &mut serde_json::Map<String, serde_json::Value>,
    patch: serde_json::Map<String, serde_json::Value>,
) {
    for (key, patch_value) in patch {
        match (target.get_mut(&key), patch_value) {
            (
                Some(serde_json::Value::Object(target_object)),
                serde_json::Value::Object(patch_object),
            ) => merge_json_objects(target_object, patch_object),
            (_, patch_value) => {
                target.insert(key, patch_value);
            }
        }
    }
}

fn parse_toml_table(
    overlay_id: &str,
    label: &str,
    content: &str,
) -> Result<toml::Table, TemporaryError> {
    toml::from_str(content).map_err(|error| {
        invalid_artifact(
            overlay_id,
            format!("{label} TOML overlay is invalid: {error}"),
        )
    })
}

fn merge_toml_tables(target: &mut toml::Table, patch: toml::Table) {
    for (key, patch_value) in patch {
        match (target.get_mut(&key), patch_value) {
            (Some(toml::Value::Table(target_table)), toml::Value::Table(patch_table)) => {
                merge_toml_tables(target_table, patch_table);
            }
            (_, patch_value) => {
                target.insert(key, patch_value);
            }
        }
    }
}

fn mirror_directory(
    source: &Path,
    target: &Path,
    relative: &Path,
    replacements: &BTreeSet<PathBuf>,
    overlay_id: &str,
) -> Result<(), TemporaryError> {
    fs::create_dir(target).map_err(|source| overlay_error(overlay_id, source))?;
    restrict_directory(target)?;

    let metadata = match fs::metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(overlay_error(overlay_id, error)),
    };
    if !metadata.is_dir() {
        return Err(invalid_artifact(
            overlay_id,
            format!("overlay source '{}' is not a directory", source.display()),
        ));
    }

    let entries = fs::read_dir(source).map_err(|source| overlay_error(overlay_id, source))?;
    for entry in entries {
        let entry = entry.map_err(|source| overlay_error(overlay_id, source))?;
        let name = entry.file_name();
        let child_relative = relative.join(&name);
        let replaces_child = replacements.contains(&child_relative);
        let replaces_descendant = replacements.iter().any(|replacement| {
            replacement != &child_relative && replacement.starts_with(&child_relative)
        });
        if replaces_child {
            continue;
        }

        let child_target = target.join(&name);
        if replaces_descendant {
            mirror_directory(
                &entry.path(),
                &child_target,
                &child_relative,
                replacements,
                overlay_id,
            )?;
        } else {
            link_entry(&entry.path(), &child_target)
                .map_err(|source| overlay_error(overlay_id, source))?;
        }
    }
    Ok(())
}

fn create_private_parents(
    overlay_root: &Path,
    parent: Option<&Path>,
    overlay_id: &str,
) -> Result<(), TemporaryError> {
    let Some(parent) = parent else {
        return Ok(());
    };
    let relative = parent
        .strip_prefix(overlay_root)
        .map_err(|_| invalid_artifact(overlay_id, "overlay file escaped its temporary root"))?;
    let mut current = overlay_root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(invalid_artifact(
                overlay_id,
                "overlay file path contains an unsafe component",
            ));
        };
        current.push(name);
        match fs::create_dir(&current) {
            Ok(()) => restrict_directory(&current)?,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(source) => return Err(overlay_error(overlay_id, source)),
        }
    }
    Ok(())
}

fn validate_path_hint(resource_id: &str, path_hint: &str) -> Result<(), TemporaryError> {
    let mut components = Path::new(path_hint).components();
    if matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none() {
        Ok(())
    } else {
        Err(invalid_artifact(
            resource_id,
            "pathHint must be one relative path component",
        ))
    }
}

fn ensure_mode(
    artifact_id: &str,
    actual: TemporaryArtifactMode,
    expected: TemporaryArtifactMode,
) -> Result<(), TemporaryError> {
    if actual == expected {
        Ok(())
    } else {
        Err(invalid_artifact(
            artifact_id,
            "artifact kind and permission mode do not match",
        ))
    }
}

fn invalid_artifact(artifact_id: &str, reason: impl Into<String>) -> TemporaryError {
    TemporaryError::InvalidArtifact {
        artifact_id: artifact_id.to_owned(),
        reason: reason.into(),
    }
}

fn overlay_error(overlay_id: &str, source: std::io::Error) -> TemporaryError {
    TemporaryError::MirrorOverlay {
        overlay_id: overlay_id.to_owned(),
        source,
    }
}

fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn render_user_home(value: &str, user_home: &Path) -> String {
    value.replace(USER_HOME_PLACEHOLDER, &user_home.to_string_lossy())
}

fn resolve_overlay_source(value: &str, user_home: &Path, codex_home: Option<&OsStr>) -> PathBuf {
    if value == CODEX_HOME_PLACEHOLDER {
        return codex_home
            .filter(|value| !value.is_empty())
            .map_or_else(|| user_home.join(".codex"), PathBuf::from);
    }
    PathBuf::from(render_user_home(value, user_home))
}

fn user_home() -> Result<PathBuf, TemporaryError> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(windows_user_home)
        .filter(|path| path.is_absolute())
        .ok_or(TemporaryError::MissingUserHome)
}

#[cfg(windows)]
fn windows_user_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(not(windows))]
fn windows_user_home() -> Option<PathBuf> {
    None
}

#[cfg(unix)]
fn link_entry(source: &Path, target: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, target)
}

#[cfg(windows)]
fn link_entry(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::fs::{symlink_dir, symlink_file};

    if fs::metadata(source)?.is_dir() {
        symlink_dir(source, target)
    } else {
        symlink_file(source, target)
    }
}

#[cfg(not(any(unix, windows)))]
fn link_entry(_source: &Path, _target: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        ErrorKind::Unsupported,
        "configuration overlays require symbolic link support",
    ))
}

fn restrict_directory(path: &Path) -> Result<(), TemporaryError> {
    restrict_path(path, PrivatePathKind::Directory).map_err(|source| TemporaryError::Permissions {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::{TemporaryWorkspace, resolve_overlay_source};
    use nan_harness_core::launch_plan::{
        ArtifactLifecycle, CODEX_HOME_PLACEHOLDER, ConfigurationOverlay, LaunchScopedFile,
        OverlayFile, OverlayFilePolicy, TemporaryArtifactMode, USER_HOME_PLACEHOLDER,
    };
    use std::fs;

    #[test]
    fn codex_overlay_source_prefers_the_configured_home() {
        let user_home = tempfile::tempdir().expect("temporary user home should exist");
        let codex_home = tempfile::tempdir().expect("temporary Codex home should exist");

        assert_eq!(
            resolve_overlay_source(
                CODEX_HOME_PLACEHOLDER,
                user_home.path(),
                Some(codex_home.path().as_os_str()),
            ),
            codex_home.path()
        );
        assert_eq!(
            resolve_overlay_source(CODEX_HOME_PLACEHOLDER, user_home.path(), None),
            user_home.path().join(".codex")
        );
    }

    #[test]
    fn launch_scoped_profiles_are_private_and_removed_on_drop() {
        let home = tempfile::tempdir().expect("temporary home should exist");
        let codex_home = home.path().join(".codex");
        fs::create_dir_all(&codex_home).expect("Codex home should exist");
        fs::write(codex_home.join("config.toml"), "notify = [\"true\"]\n")
            .expect("base config should exist");
        let files = [codex_profile("launch_01scopedfile")];

        let workspace = TemporaryWorkspace::materialize_with_home_and_scoped(
            &[],
            &[],
            &files,
            home.path(),
            |_, content| Ok(content.to_owned()),
        )
        .expect("profile should materialize");
        let profile = workspace
            .path("codex-profile")
            .expect("profile path should exist")
            .to_path_buf();
        let lock = profile.with_file_name(format!(
            "{}.lock",
            profile
                .file_name()
                .expect("profile name should exist")
                .to_string_lossy()
        ));

        assert_eq!(
            fs::read_to_string(&profile).expect("profile should be readable"),
            "model = \"qwen3.6\"\n"
        );
        assert!(lock.exists());
        assert_eq!(
            fs::read_to_string(codex_home.join("config.toml"))
                .expect("base config should remain readable"),
            "notify = [\"true\"]\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&profile)
                    .expect("profile metadata should exist")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        #[cfg(windows)]
        nan_harness_test_support::windows_acl::assert_private_file(&profile)
            .expect("launch-scoped profile should have a private protected DACL");

        drop(workspace);
        assert!(!profile.exists());
        assert!(!lock.exists());
    }

    #[test]
    fn launch_scoped_profile_cleanup_preserves_active_launches() {
        let home = tempfile::tempdir().expect("temporary home should exist");
        let codex_home = home.path().join(".codex");
        fs::create_dir_all(&codex_home).expect("Codex home should exist");
        let stale = codex_home.join("nan-harness-launch_01staleprofile.config.toml");
        let stale_lock = codex_home.join("nan-harness-launch_01staleprofile.config.toml.lock");
        fs::write(&stale, "stale").expect("stale profile should exist");
        fs::write(&stale_lock, "").expect("stale lock should exist");

        let first_files = [codex_profile("launch_01firstactive")];
        let first = TemporaryWorkspace::materialize_with_home_and_scoped(
            &[],
            &[],
            &first_files,
            home.path(),
            |_, content| Ok(content.to_owned()),
        )
        .expect("first profile should materialize");
        let first_profile = first
            .path("codex-profile")
            .expect("first profile should exist")
            .to_path_buf();
        assert!(!stale.exists());
        assert!(!stale_lock.exists());

        let second_files = [codex_profile("launch_01secondactive")];
        let second = TemporaryWorkspace::materialize_with_home_and_scoped(
            &[],
            &[],
            &second_files,
            home.path(),
            |_, content| Ok(content.to_owned()),
        )
        .expect("second profile should materialize");
        assert!(first_profile.exists());

        drop(second);
        assert!(first_profile.exists());
        drop(first);
        assert!(!first_profile.exists());
    }

    fn codex_profile(launch_id: &str) -> LaunchScopedFile {
        LaunchScopedFile {
            id: "codex-profile".to_owned(),
            directory: format!("{USER_HOME_PLACEHOLDER}/.codex"),
            file_name: format!("nan-harness-{launch_id}.config.toml"),
            ownership_prefix: "nan-harness-launch_".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: "model = \"qwen3.6\"\n".to_owned(),
            lifecycle: ArtifactLifecycle::Launch,
        }
    }

    #[test]
    fn overlays_replace_routing_files_and_link_the_remaining_user_state() {
        let home = tempfile::tempdir().expect("temporary home should exist");
        let source = home.path().join(".cline");
        fs::create_dir_all(source.join("data/settings")).expect("settings should exist");
        fs::create_dir_all(source.join("data/sessions")).expect("sessions should exist");
        fs::write(source.join("data/settings/providers.json"), "USER_PROVIDER")
            .expect("provider fixture should exist");
        fs::write(source.join("data/sessions/session.json"), "USER_SESSION")
            .expect("session fixture should exist");
        fs::write(source.join("hooks.json"), "USER_HOOKS").expect("hook fixture should exist");
        let overlays = [ConfigurationOverlay {
            id: "cline-config".to_owned(),
            path_hint: "cline".to_owned(),
            source_path: format!("{USER_HOME_PLACEHOLDER}/.cline"),
            files: vec![OverlayFile {
                path: "data/settings/providers.json".to_owned(),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template: "NAN_PROVIDER".to_owned(),
                policy: OverlayFilePolicy::Replace,
            }],
            lifecycle: ArtifactLifecycle::Launch,
        }];

        let workspace =
            TemporaryWorkspace::materialize_with_home(&[], &overlays, home.path(), |_, content| {
                Ok(content.to_owned())
            })
            .expect("overlay should materialize");
        let overlay = workspace
            .path("cline-config")
            .expect("overlay should exist");

        assert_eq!(
            fs::read_to_string(overlay.join("data/settings/providers.json"))
                .expect("provider overlay should be readable"),
            "NAN_PROVIDER"
        );
        assert_eq!(
            fs::read_to_string(overlay.join("data/sessions/session.json"))
                .expect("linked session should be readable"),
            "USER_SESSION"
        );
        assert_eq!(
            fs::read_to_string(overlay.join("hooks.json")).expect("linked hook should be readable"),
            "USER_HOOKS"
        );
        #[cfg(unix)]
        {
            assert!(
                fs::symlink_metadata(overlay.join("data/sessions"))
                    .expect("session link should have metadata")
                    .file_type()
                    .is_symlink()
            );
            assert!(
                fs::symlink_metadata(overlay.join("hooks.json"))
                    .expect("hook link should have metadata")
                    .file_type()
                    .is_symlink()
            );
        }
    }

    #[test]
    fn preserve_policy_creates_only_missing_fallback_files() {
        let home = tempfile::tempdir().expect("temporary home should exist");
        let source = home.path().join(".openclaw");
        fs::create_dir_all(&source).expect("OpenClaw source should exist");
        fs::write(source.join("openclaw.json"), "USER_CONFIG")
            .expect("OpenClaw fixture should exist");
        let overlays = [ConfigurationOverlay {
            id: "openclaw-config".to_owned(),
            path_hint: "openclaw".to_owned(),
            source_path: format!("{USER_HOME_PLACEHOLDER}/.openclaw"),
            files: vec![
                OverlayFile {
                    path: "openclaw.json".to_owned(),
                    mode: TemporaryArtifactMode::OwnerFile,
                    content_template: "{}".to_owned(),
                    policy: OverlayFilePolicy::Preserve,
                },
                OverlayFile {
                    path: "nan-harness.json".to_owned(),
                    mode: TemporaryArtifactMode::OwnerFile,
                    content_template: "NAN_CONFIG".to_owned(),
                    policy: OverlayFilePolicy::Replace,
                },
            ],
            lifecycle: ArtifactLifecycle::Launch,
        }];

        let workspace =
            TemporaryWorkspace::materialize_with_home(&[], &overlays, home.path(), |_, content| {
                Ok(content.to_owned())
            })
            .expect("overlay should materialize");
        let overlay = workspace
            .path("openclaw-config")
            .expect("overlay should exist");

        assert_eq!(
            fs::read_to_string(overlay.join("openclaw.json"))
                .expect("original config should remain readable"),
            "USER_CONFIG"
        );
        assert_eq!(
            fs::read_to_string(overlay.join("nan-harness.json"))
                .expect("nan-harness config should be readable"),
            "NAN_CONFIG"
        );
    }

    #[test]
    fn home_overlay_merges_routing_and_copies_mutable_state() {
        let home = tempfile::tempdir().expect("temporary home should exist");
        let storage = home.path().join(".agent-mock/global-storage");
        fs::create_dir_all(storage.join("tasks/session-1")).expect("agent state should exist");
        fs::write(
            storage.join("global-state.json"),
            r#"{"theme":"dark","nested":{"preserved":true}}"#,
        )
        .expect("agent state fixture should exist");
        fs::write(storage.join("secrets.json"), r#"{"userSecret":"keep"}"#)
            .expect("agent secrets fixture should exist");
        fs::write(storage.join("tasks/session-1/history.json"), "USER_SESSION")
            .expect("agent session fixture should exist");
        let overlays = [ConfigurationOverlay {
            id: "agent-home".to_owned(),
            path_hint: "agent-home".to_owned(),
            source_path: USER_HOME_PLACEHOLDER.to_owned(),
            files: vec![
                OverlayFile {
                    path: ".agent-mock/global-storage/global-state.json".to_owned(),
                    mode: TemporaryArtifactMode::OwnerFile,
                    content_template:
                        r#"{"openAiNativeBaseUrl":"http://127.0.0.1:1234/v1","nested":{"routing":true}}"#
                            .to_owned(),
                    policy: OverlayFilePolicy::MergeJson,
                },
                OverlayFile {
                    path: ".agent-mock/global-storage/secrets.json".to_owned(),
                    mode: TemporaryArtifactMode::OwnerFile,
                    content_template: "{}".to_owned(),
                    policy: OverlayFilePolicy::Copy,
                },
            ],
            lifecycle: ArtifactLifecycle::Launch,
        }];

        let workspace =
            TemporaryWorkspace::materialize_with_home(&[], &overlays, home.path(), |_, content| {
                Ok(content.to_owned())
            })
            .expect("agent home overlay should materialize");
        let overlay = workspace.path("agent-home").expect("overlay should exist");
        let state: serde_json::Value = serde_json::from_slice(
            &fs::read(overlay.join(".agent-mock/global-storage/global-state.json"))
                .expect("merged state should be readable"),
        )
        .expect("merged state should be JSON");

        assert_eq!(state["theme"], "dark");
        assert_eq!(state["nested"]["preserved"], true);
        assert_eq!(state["nested"]["routing"], true);
        assert_eq!(state["openAiNativeBaseUrl"], "http://127.0.0.1:1234/v1");
        let overlay_secrets = overlay.join(".agent-mock/global-storage/secrets.json");
        assert_eq!(
            fs::read_to_string(&overlay_secrets).expect("copied secrets should be readable"),
            r#"{"userSecret":"keep"}"#
        );
        fs::write(&overlay_secrets, r#"{"bridgeToken":"temporary"}"#)
            .expect("temporary secrets should be writable");
        assert_eq!(
            fs::read_to_string(storage.join("secrets.json"))
                .expect("source secrets should remain readable"),
            r#"{"userSecret":"keep"}"#
        );
        assert_eq!(
            fs::read_to_string(
                overlay.join(".agent-mock/global-storage/tasks/session-1/history.json"),
            )
            .expect("linked agent session should be readable"),
            "USER_SESSION"
        );
    }

    #[test]
    fn toml_overlay_merges_model_and_shares_codex_session_state() {
        let home = tempfile::tempdir().expect("temporary home should exist");
        let source = home.path().join(".codex");
        fs::create_dir_all(&source).expect("Codex source should exist");
        fs::write(
            source.join("config.toml"),
            "model = \"qwen3.6\"\nmodel_provider = \"openai\"\n\n[profiles.default]\neffort = \"high\"\n",
        )
        .expect("Codex config fixture should exist");
        fs::write(source.join("state_5.sqlite"), [0, 1, 2, 3])
            .expect("Codex state fixture should exist");
        let overlays = [ConfigurationOverlay {
            id: "codex-home".to_owned(),
            path_hint: "codex-home".to_owned(),
            source_path: format!("{USER_HOME_PLACEHOLDER}/.codex"),
            files: vec![OverlayFile {
                path: "config.toml".to_owned(),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template: "model = \"deepseek-v4-flash\"\n".to_owned(),
                policy: OverlayFilePolicy::MergeToml,
            }],
            lifecycle: ArtifactLifecycle::Launch,
        }];

        let workspace =
            TemporaryWorkspace::materialize_with_home(&[], &overlays, home.path(), |_, content| {
                Ok(content.to_owned())
            })
            .expect("Codex overlay should materialize");
        let overlay = workspace.path("codex-home").expect("overlay should exist");
        let merged: toml::Table = toml::from_str(
            &fs::read_to_string(overlay.join("config.toml"))
                .expect("merged Codex config should be readable"),
        )
        .expect("merged Codex config should be TOML");

        assert_eq!(merged["model"].as_str(), Some("deepseek-v4-flash"));
        assert_eq!(merged["model_provider"].as_str(), Some("openai"));
        assert_eq!(
            merged["profiles"]["default"]["effort"].as_str(),
            Some("high")
        );
        assert!(
            fs::read_to_string(source.join("config.toml"))
                .expect("source Codex config should remain readable")
                .contains("model = \"qwen3.6\"")
        );
        let mirrored_state = overlay.join("state_5.sqlite");
        fs::write(&mirrored_state, [4, 5, 6, 7]).expect("mirrored state should be writable");
        assert_eq!(
            fs::read(source.join("state_5.sqlite")).expect("source state should be readable"),
            [4, 5, 6, 7]
        );
        #[cfg(unix)]
        assert!(
            fs::symlink_metadata(mirrored_state)
                .expect("mirrored state should have metadata")
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn toml_overlay_preserves_unmanaged_kimi_settings() {
        let home = tempfile::tempdir().expect("temporary home should exist");
        let source = home.path().join(".kimi-code");
        fs::create_dir_all(&source).expect("Kimi Code source should exist");
        fs::write(
            source.join("config.toml"),
            "default_model = \"user/model\"\n\n[agents.review]\nprompt = \"Review carefully\"\n",
        )
        .expect("Kimi Code config fixture should exist");
        let overlays = [ConfigurationOverlay {
            id: "kimi-code-home".to_owned(),
            path_hint: "kimi-code".to_owned(),
            source_path: format!("{USER_HOME_PLACEHOLDER}/.kimi-code"),
            files: vec![OverlayFile {
                path: "config.toml".to_owned(),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template: "[models.\"nan/qwen3.6\"]\nmodel = \"qwen3.6\"\n".to_owned(),
                policy: OverlayFilePolicy::MergeToml,
            }],
            lifecycle: ArtifactLifecycle::Launch,
        }];

        let workspace =
            TemporaryWorkspace::materialize_with_home(&[], &overlays, home.path(), |_, content| {
                Ok(content.to_owned())
            })
            .expect("Kimi Code overlay should materialize");
        let merged: toml::Table = toml::from_str(
            &fs::read_to_string(
                workspace
                    .path("kimi-code-home")
                    .expect("overlay should exist")
                    .join("config.toml"),
            )
            .expect("merged Kimi Code config should be readable"),
        )
        .expect("merged Kimi Code config should be TOML");

        assert_eq!(merged["default_model"].as_str(), Some("user/model"));
        assert_eq!(
            merged["agents"]["review"]["prompt"].as_str(),
            Some("Review carefully")
        );
        assert_eq!(
            merged["models"]["nan/qwen3.6"]["model"].as_str(),
            Some("qwen3.6")
        );
    }

    #[test]
    fn toml_overlay_relocates_codex_hook_state_to_the_mirrored_home() {
        let home = tempfile::tempdir().expect("temporary home should exist");
        let source = home.path().join(".codex");
        fs::create_dir_all(&source).expect("Codex source should exist");
        fs::write(source.join("hooks.json"), "{\"hooks\":{}}").expect("Codex hooks should exist");
        fs::write(
            source.join("config.toml"),
            format!(
                "[hooks.state.\"{}:pre_tool_use:0:0\"]\ntrusted_hash = \"sha256:test\"\n",
                source.join("hooks.json").display()
            ),
        )
        .expect("Codex config should exist");
        let overlays = [ConfigurationOverlay {
            id: "codex-home".to_owned(),
            path_hint: "codex-home".to_owned(),
            source_path: format!("{USER_HOME_PLACEHOLDER}/.codex"),
            files: vec![OverlayFile {
                path: "config.toml".to_owned(),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template: "model = \"deepseek-v4-flash\"\n".to_owned(),
                policy: OverlayFilePolicy::MergeToml,
            }],
            lifecycle: ArtifactLifecycle::Launch,
        }];

        let workspace =
            TemporaryWorkspace::materialize_with_home(&[], &overlays, home.path(), |_, content| {
                Ok(content.to_owned())
            })
            .expect("Codex overlay should materialize");
        let overlay = workspace.path("codex-home").expect("overlay should exist");
        let merged: toml::Table = toml::from_str(
            &fs::read_to_string(overlay.join("config.toml"))
                .expect("merged Codex config should be readable"),
        )
        .expect("merged Codex config should be TOML");

        let state = merged["hooks"]["state"]
            .as_table()
            .expect("hook state should be a table");
        assert!(state.contains_key(&format!(
            "{}:pre_tool_use:0:0",
            overlay.join("hooks.json").display()
        )));
        let canonical_overlay = fs::canonicalize(overlay).expect("overlay should canonicalize");
        assert!(state.contains_key(&format!(
            "{}:pre_tool_use:0:0",
            canonical_overlay.join("hooks.json").display()
        )));
        assert!(state.contains_key(&format!(
            "{}:pre_tool_use:0:0",
            source.join("hooks.json").display()
        )));
    }

    #[test]
    fn binary_copy_overlay_isolated_from_user_state() {
        let home = tempfile::tempdir().expect("temporary home should exist");
        let source = home.path().join(".codex");
        fs::create_dir_all(&source).expect("Codex source should exist");
        fs::write(source.join("state_5.sqlite"), [0, 1, 2, 3])
            .expect("Codex state fixture should exist");
        let overlays = [ConfigurationOverlay {
            id: "codex-home".to_owned(),
            path_hint: "codex-home".to_owned(),
            source_path: format!("{USER_HOME_PLACEHOLDER}/.codex"),
            files: vec![OverlayFile {
                path: "state_5.sqlite".to_owned(),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template: String::new(),
                policy: OverlayFilePolicy::CopyBinary,
            }],
            lifecycle: ArtifactLifecycle::Launch,
        }];

        let workspace =
            TemporaryWorkspace::materialize_with_home(&[], &overlays, home.path(), |_, content| {
                Ok(content.to_owned())
            })
            .expect("Codex state overlay should materialize");
        let copied = workspace
            .path("codex-home")
            .expect("overlay should exist")
            .join("state_5.sqlite");
        assert_eq!(
            fs::read(&copied).expect("copied state should be readable"),
            [0, 1, 2, 3]
        );
        fs::write(&copied, [4, 5, 6, 7]).expect("copied state should be writable");
        assert_eq!(
            fs::read(source.join("state_5.sqlite")).expect("source state should be readable"),
            [0, 1, 2, 3]
        );
        #[cfg(unix)]
        assert!(
            !fs::symlink_metadata(copied)
                .expect("copied state should have metadata")
                .file_type()
                .is_symlink()
        );
    }
}
