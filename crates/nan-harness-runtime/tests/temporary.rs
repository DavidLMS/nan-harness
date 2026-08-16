use nan_harness_core::launch_plan::{
    ArtifactLifecycle, TemporaryArtifact, TemporaryArtifactKind, TemporaryArtifactMode,
};
use nan_harness_runtime::temporary::TemporaryWorkspace;
use std::fs;

#[test]
fn temporary_artifacts_are_private_and_removed_on_drop() {
    let artifacts = [
        TemporaryArtifact {
            id: "config-file".to_owned(),
            kind: TemporaryArtifactKind::File,
            path_hint: "config.json".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: Some("{}".to_owned()),
            lifecycle: ArtifactLifecycle::Launch,
        },
        TemporaryArtifact {
            id: "cache-dir".to_owned(),
            kind: TemporaryArtifactKind::Directory,
            path_hint: "cache".to_owned(),
            mode: TemporaryArtifactMode::OwnerDirectory,
            content_template: None,
            lifecycle: ArtifactLifecycle::Launch,
        },
    ];
    let workspace = TemporaryWorkspace::materialize(&artifacts).expect("artifacts should exist");
    let root = workspace.root().to_path_buf();
    let file = workspace
        .path("config-file")
        .expect("file path should exist");
    let directory = workspace
        .path("cache-dir")
        .expect("directory path should exist");

    assert_eq!(
        fs::read_to_string(file).expect("file should be readable"),
        "{}"
    );
    assert!(directory.is_dir());
    assert_private_modes(&root, file, directory);

    drop(workspace);
    assert!(!root.exists());
}

#[cfg(unix)]
fn assert_private_modes(
    root: &std::path::Path,
    file: &std::path::Path,
    directory: &std::path::Path,
) {
    use std::os::unix::fs::PermissionsExt;

    let mode = |path: &std::path::Path| {
        fs::metadata(path)
            .expect("metadata should exist")
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode(root), 0o700);
    assert_eq!(mode(file), 0o600);
    assert_eq!(mode(directory), 0o700);
}

#[cfg(not(unix))]
fn assert_private_modes(
    _root: &std::path::Path,
    _file: &std::path::Path,
    _directory: &std::path::Path,
) {
}
