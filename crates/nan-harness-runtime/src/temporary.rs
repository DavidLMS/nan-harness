use nan_harness_core::launch_plan::{
    ConfigurationOverlay, OverlayFilePolicy, TemporaryArtifact, TemporaryArtifactKind,
    TemporaryArtifactMode, USER_HOME_PLACEHOLDER,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use tempfile::TempDir;
use thiserror::Error;

pub struct TemporaryWorkspace {
    root: TempDir,
    paths: BTreeMap<String, PathBuf>,
    user_home: PathBuf,
}

impl TemporaryWorkspace {
    /// Creates a private workspace and materializes all declared artifacts.
    ///
    /// # Errors
    ///
    /// Returns [`TemporaryError`] when an artifact is unsafe or cannot be created privately.
    pub fn materialize(artifacts: &[TemporaryArtifact]) -> Result<Self, TemporaryError> {
        Self::materialize_with(artifacts, &[], |_, content| Ok(content.to_owned()))
    }

    pub(crate) fn materialize_with(
        artifacts: &[TemporaryArtifact],
        overlays: &[ConfigurationOverlay],
        render: impl Fn(&str, &str) -> Result<String, TemporaryError>,
    ) -> Result<Self, TemporaryError> {
        Self::materialize_with_home(artifacts, overlays, &user_home()?, render)
    }

    fn materialize_with_home(
        artifacts: &[TemporaryArtifact],
        overlays: &[ConfigurationOverlay],
        user_home: &Path,
        render: impl Fn(&str, &str) -> Result<String, TemporaryError>,
    ) -> Result<Self, TemporaryError> {
        let root = tempfile::Builder::new()
            .prefix("nan-harness-")
            .tempdir()
            .map_err(TemporaryError::CreateWorkspace)?;
        set_mode(root.path(), 0o700)?;
        let user_home = user_home.to_path_buf();
        let mut paths = BTreeMap::new();

        for overlay in overlays {
            validate_path_hint(&overlay.id, &overlay.path_hint)?;
            let path = root.path().join(&overlay.path_hint);
            let source = PathBuf::from(render_user_home(&overlay.source_path, &user_home));
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
                    fs::write(&path, render_user_home(&rendered, &user_home)).map_err(
                        |source| TemporaryError::Materialize {
                            artifact_id: artifact.id.clone(),
                            source,
                        },
                    )?;
                    ensure_mode(
                        &artifact.id,
                        artifact.mode,
                        TemporaryArtifactMode::OwnerFile,
                    )?;
                    set_mode(&path, 0o600)?;
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
                    set_mode(&path, 0o700)?;
                }
            }
            paths.insert(artifact.id.clone(), path);
        }
        Ok(Self {
            root,
            paths,
            user_home,
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
            file.policy == OverlayFilePolicy::Replace || !path_exists(&source.join(&file.path))
        })
        .map(|file| PathBuf::from(&file.path))
        .collect::<BTreeSet<_>>();
    mirror_directory(source, target, Path::new(""), &replacements, &overlay.id)?;

    for file in &overlay.files {
        let path = target.join(&file.path);
        if file.policy == OverlayFilePolicy::Preserve && path_exists(&path) {
            continue;
        }
        ensure_mode(&overlay.id, file.mode, TemporaryArtifactMode::OwnerFile)?;
        create_private_parents(target, path.parent(), &overlay.id)?;
        let rendered = render(&overlay.id, &file.content_template)?;
        fs::write(&path, render_user_home(&rendered, user_home)).map_err(|source| {
            TemporaryError::Materialize {
                artifact_id: overlay.id.clone(),
                source,
            }
        })?;
        set_mode(&path, 0o600)?;
    }
    Ok(())
}

fn mirror_directory(
    source: &Path,
    target: &Path,
    relative: &Path,
    replacements: &BTreeSet<PathBuf>,
    overlay_id: &str,
) -> Result<(), TemporaryError> {
    fs::create_dir(target).map_err(|source| overlay_error(overlay_id, source))?;
    set_mode(target, 0o700)?;

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
            Ok(()) => set_mode(&current, 0o700)?,
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

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<(), TemporaryError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|source| {
        TemporaryError::Permissions {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<(), TemporaryError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::TemporaryWorkspace;
    use nan_harness_core::launch_plan::{
        ArtifactLifecycle, ConfigurationOverlay, OverlayFile, OverlayFilePolicy,
        TemporaryArtifactMode, USER_HOME_PLACEHOLDER,
    };
    use std::fs;

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
                .expect("NaN config should be readable"),
            "NAN_CONFIG"
        );
    }
}
