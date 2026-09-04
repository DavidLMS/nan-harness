use super::super::super::TemporaryWorkspace;
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, ConfigurationOverlay, OverlayFile, OverlayFilePolicy, TemporaryArtifactMode,
    USER_HOME_PLACEHOLDER,
};
use std::fs;

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
