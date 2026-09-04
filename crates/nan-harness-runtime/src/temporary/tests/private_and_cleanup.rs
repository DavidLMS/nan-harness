use super::super::lifecycle::ensure_configuration_directory;
use super::super::paths::resolve_overlay_source;
use nan_harness_core::launch_plan::CODEX_HOME_PLACEHOLDER;
use std::fs;

#[cfg(unix)]
#[test]
fn missing_configuration_directories_are_created_private() {
    use std::os::unix::fs::PermissionsExt;

    let home = tempfile::tempdir().expect("temporary home should exist");
    let path = home.path().join("nested/configuration");

    ensure_configuration_directory(&path, "test-config")
        .expect("configuration directories should be created");

    for directory in [home.path().join("nested"), path] {
        let mode = fs::metadata(&directory)
            .expect("configuration directory metadata should exist")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);
    }
}

#[cfg(unix)]
#[test]
fn preexisting_configuration_directory_permissions_are_unchanged() {
    use std::os::unix::fs::PermissionsExt;

    let home = tempfile::tempdir().expect("temporary home should exist");
    let path = home.path().join("configuration");
    fs::create_dir(&path).expect("configuration directory should exist");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
        .expect("configuration directory should be permissive");

    ensure_configuration_directory(&path, "test-config")
        .expect("preexisting configuration directory should be accepted");

    assert_eq!(
        fs::metadata(&path)
            .expect("configuration directory metadata should exist")
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
}

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
