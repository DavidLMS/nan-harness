use super::TemporaryError;
use super::platform::windows_user_home;
use nan_harness_core::launch_plan::{
    CODEX_HOME_PLACEHOLDER, TemporaryArtifactMode, USER_HOME_PLACEHOLDER,
};
use nan_harness_i18n::DiagnosticText;
use nan_harness_i18n::messages as detail_messages;
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
            DiagnosticText::new(
                detail_messages::detail_pathhint_must_be_one_relative_path_component,
            ),
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
            DiagnosticText::new(
                detail_messages::detail_artifact_kind_and_permission_mode_do_not_match,
            ),
        ))
    }
}

pub(super) fn invalid_artifact(
    artifact_id: &str,
    reason: impl Into<DiagnosticText>,
) -> TemporaryError {
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

pub(super) fn render_overlay_paths(
    value: &str,
    overlay_id: &str,
    overlay_path: &Path,
    user_home: &Path,
    json_strings: bool,
) -> String {
    let encode = |path: &Path| {
        let path = path.to_string_lossy().into_owned();
        if json_strings {
            // Placeholders occur inside JSON strings; preserve Windows separators and
            // quotes/control characters in paths without changing their decoded value.
            let encoded = serde_json::Value::String(path).to_string();
            encoded[1..encoded.len() - 1].to_owned()
        } else {
            path
        }
    };
    value
        .replace(USER_HOME_PLACEHOLDER, &encode(user_home))
        .replace(&format!("{{artifact:{overlay_id}}}"), &encode(overlay_path))
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
    use super::{render_overlay_paths, render_user_home, validate_path_hint};
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

    #[test]
    fn json_overlay_paths_round_trip_on_every_platform() {
        let template = r#"{"plugins":{"load":{"paths":["{artifact:openclaw-config}/plugins/nan-harness-search"]}},"home":"{runtime:user_home}"}"#;
        for path in [
            r"C:\Users\ADMINI~1\AppData\Local\Temp\2\nan-harness-test\openclaw",
            r"\\server\share\new\test",
            "/tmp/nan-harness-test/openclaw",
            "/var/folders/example/T/nan-harness-test/openclaw",
            "/tmp/quoted\"directory/line\nfeed",
        ] {
            let rendered = render_overlay_paths(
                template,
                "openclaw-config",
                Path::new(path),
                Path::new(path),
                true,
            );
            let config: serde_json::Value =
                serde_json::from_str(&rendered).expect("paths must remain valid JSON");
            assert_eq!(
                config["plugins"]["load"]["paths"][0],
                format!("{path}/plugins/nan-harness-search")
            );
            assert_eq!(config["home"], path);
        }
    }

    #[test]
    fn non_json_overlay_paths_remain_literal() {
        let path = r"C:\Users\Administrator\Temp\2";
        assert_eq!(
            render_overlay_paths(
                "{artifact:config}/plugins:{runtime:user_home}",
                "config",
                Path::new(path),
                Path::new(path),
                false,
            ),
            format!("{path}/plugins:{path}"),
        );
    }
}
