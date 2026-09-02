use super::TemporaryError;
use super::platform::windows_user_home;
use nan_harness_core::launch_plan::{
    CODEX_HOME_PLACEHOLDER, TemporaryArtifactMode, USER_HOME_PLACEHOLDER,
};
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub(super) fn validate_path_hint(resource_id: &str, path_hint: &str) -> Result<(), TemporaryError> {
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

pub(super) fn ensure_mode(
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

pub(super) fn invalid_artifact(artifact_id: &str, reason: impl Into<String>) -> TemporaryError {
    TemporaryError::InvalidArtifact {
        artifact_id: artifact_id.to_owned(),
        reason: reason.into(),
    }
}

pub(super) fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

pub(super) fn render_user_home(value: &str, user_home: &Path) -> String {
    value.replace(USER_HOME_PLACEHOLDER, &user_home.to_string_lossy())
}

pub(super) fn resolve_overlay_source(
    value: &str,
    user_home: &Path,
    codex_home: Option<&OsStr>,
) -> PathBuf {
    if value == CODEX_HOME_PLACEHOLDER {
        return codex_home
            .filter(|value| !value.is_empty())
            .map_or_else(|| user_home.join(".codex"), PathBuf::from);
    }
    PathBuf::from(render_user_home(value, user_home))
}

pub(super) fn user_home() -> Result<PathBuf, TemporaryError> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(windows_user_home)
        .filter(|path| path.is_absolute())
        .ok_or(TemporaryError::MissingUserHome)
}

#[cfg(test)]
mod tests {
    use super::{render_user_home, validate_path_hint};
    use crate::temporary::TemporaryError;
    use nan_harness_core::launch_plan::USER_HOME_PLACEHOLDER;
    use std::path::Path;

    #[test]
    fn path_hints_accept_exactly_one_relative_component() {
        assert!(validate_path_hint("config", "config").is_ok());

        for path_hint in ["", ".", "..", "nested/config", "/config"] {
            assert!(matches!(
                validate_path_hint("config", path_hint),
                Err(TemporaryError::InvalidArtifact { .. })
            ));
        }
    }

    #[test]
    fn user_home_rendering_replaces_every_placeholder() {
        assert_eq!(
            render_user_home(
                &format!("{USER_HOME_PLACEHOLDER}/one:{USER_HOME_PLACEHOLDER}/two"),
                Path::new("/private/home"),
            ),
            "/private/home/one:/private/home/two"
        );
    }
}
