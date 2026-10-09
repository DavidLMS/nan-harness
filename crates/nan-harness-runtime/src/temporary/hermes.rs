use nan_harness_core::launch_plan::{
    ConfigurationOverlay, HERMES_HOME_PLACEHOLDER, OverlayFile, OverlayFilePolicy,
    TemporaryArtifactMode,
};
use std::{fs, io, path::Path};

/// Keep PM metadata local while borrowing the installed environment. Hermes uses
/// the canonical metadata directory to identify its owning home; linking that
/// directory would let it republish shared launchers with temporary Python paths.
pub(super) fn dependency_files(
    overlay: &ConfigurationOverlay,
    source: &Path,
) -> io::Result<Vec<OverlayFile>> {
    if overlay.source_path != HERMES_HOME_PLACEHOLDER {
        return Ok(Vec::new());
    }
    let entries = match fs::read_dir(source.join("installs")) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.len() != 16 || !name.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            continue;
        }
        if entry.path().join("facts.json").try_exists()? {
            files.push(OverlayFile {
                path: format!("installs/{name}/facts.json"),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template: String::new(),
                policy: OverlayFilePolicy::CopyBinary,
            });
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nan_harness_core::launch_plan::ArtifactLifecycle;

    #[test]
    fn hermes_overlay_keeps_metadata_private_and_dependencies_owned_by_source() {
        let source = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let state = "installs/0123456789abcdef";
        fs::create_dir_all(source.path().join(state).join("environments")).unwrap();
        fs::write(source.path().join(state).join("facts.json"), "{}").unwrap();
        let target = workspace.path().join("hermes");
        let overlay = ConfigurationOverlay {
            id: "hermes-home".into(),
            path_hint: "hermes".into(),
            source_path: HERMES_HOME_PLACEHOLDER.into(),
            lifecycle: ArtifactLifecycle::Launch,
            files: vec![],
        };
        super::super::overlays::materialize_overlay(
            &overlay,
            source.path(),
            &target,
            &|_, value| Ok(value.into()),
            source.path(),
        )
        .unwrap();
        assert_ne!(
            fs::canonicalize(target.join(state)).unwrap(),
            fs::canonicalize(source.path().join(state)).unwrap()
        );
        assert_eq!(
            fs::canonicalize(target.join(state).join("environments")).unwrap(),
            fs::canonicalize(source.path().join(state).join("environments")).unwrap()
        );
        fs::write(target.join(state).join("facts.json"), "changed").unwrap();
        assert_eq!(
            fs::read_to_string(source.path().join(state).join("facts.json")).unwrap(),
            "{}"
        );
        workspace.close().unwrap();
        assert!(source.path().join(state).join("environments").is_dir());
    }
}
