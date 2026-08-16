use nan_harness_core::launch_plan::{
    TemporaryArtifact, TemporaryArtifactKind, TemporaryArtifactMode,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use tempfile::TempDir;
use thiserror::Error;

pub struct TemporaryWorkspace {
    root: TempDir,
    paths: BTreeMap<String, PathBuf>,
}

impl TemporaryWorkspace {
    /// Creates a private workspace and materializes all declared artifacts.
    ///
    /// # Errors
    ///
    /// Returns [`TemporaryError`] when an artifact is unsafe or cannot be created privately.
    pub fn materialize(artifacts: &[TemporaryArtifact]) -> Result<Self, TemporaryError> {
        Self::materialize_with(artifacts, |_, content| Ok(content.to_owned()))
    }

    pub(crate) fn materialize_with(
        artifacts: &[TemporaryArtifact],
        render: impl Fn(&TemporaryArtifact, &str) -> Result<String, TemporaryError>,
    ) -> Result<Self, TemporaryError> {
        let root = tempfile::Builder::new()
            .prefix("nan-harness-")
            .tempdir()
            .map_err(TemporaryError::CreateWorkspace)?;
        set_mode(root.path(), 0o700)?;

        let mut paths = BTreeMap::new();
        for artifact in artifacts {
            validate_path_hint(artifact)?;
            let path = root.path().join(&artifact.path_hint);
            match artifact.kind {
                TemporaryArtifactKind::File => {
                    let content = artifact.content_template.as_deref().ok_or_else(|| {
                        TemporaryError::InvalidArtifact {
                            artifact_id: artifact.id.clone(),
                            reason: "file content is missing".to_owned(),
                        }
                    })?;
                    let rendered = render(artifact, content)?;
                    fs::write(&path, rendered).map_err(|source| TemporaryError::Materialize {
                        artifact_id: artifact.id.clone(),
                        source,
                    })?;
                    ensure_mode(artifact, TemporaryArtifactMode::OwnerFile)?;
                    set_mode(&path, 0o600)?;
                }
                TemporaryArtifactKind::Directory => {
                    ensure_mode(artifact, TemporaryArtifactMode::OwnerDirectory)?;
                    fs::create_dir(&path).map_err(|source| TemporaryError::Materialize {
                        artifact_id: artifact.id.clone(),
                        source,
                    })?;
                    set_mode(&path, 0o700)?;
                }
            }
            paths.insert(artifact.id.clone(), path);
        }
        Ok(Self { root, paths })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        self.root.path()
    }

    #[must_use]
    pub fn path(&self, artifact_id: &str) -> Option<&Path> {
        self.paths.get(artifact_id).map(PathBuf::as_path)
    }
}

#[derive(Debug, Error)]
pub enum TemporaryError {
    #[error("could not create a private temporary workspace: {0}")]
    CreateWorkspace(std::io::Error),
    #[error("temporary artifact '{artifact_id}' is invalid: {reason}")]
    InvalidArtifact { artifact_id: String, reason: String },
    #[error("could not materialize temporary artifact '{artifact_id}': {source}")]
    Materialize {
        artifact_id: String,
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

fn validate_path_hint(artifact: &TemporaryArtifact) -> Result<(), TemporaryError> {
    let mut components = Path::new(&artifact.path_hint).components();
    if matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none() {
        Ok(())
    } else {
        Err(TemporaryError::InvalidArtifact {
            artifact_id: artifact.id.clone(),
            reason: "pathHint must be one relative path component".to_owned(),
        })
    }
}

fn ensure_mode(
    artifact: &TemporaryArtifact,
    expected: TemporaryArtifactMode,
) -> Result<(), TemporaryError> {
    if artifact.mode == expected {
        Ok(())
    } else {
        Err(TemporaryError::InvalidArtifact {
            artifact_id: artifact.id.clone(),
            reason: "artifact kind and permission mode do not match".to_owned(),
        })
    }
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
