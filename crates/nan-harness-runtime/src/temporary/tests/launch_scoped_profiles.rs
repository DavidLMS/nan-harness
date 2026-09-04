use super::super::TemporaryWorkspace;
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, LaunchScopedFile, TemporaryArtifactMode, USER_HOME_PLACEHOLDER,
};
use std::fs;

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
