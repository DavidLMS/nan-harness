use super::super::ZedDesktopError;
use super::super::paths::{ZedPlatform, settings_path_for_platform};
use super::super::process::{SystemZedProcess, command_is_zed_main, resolve_explicit};
use std::fs;
use std::path::Path;

#[test]
fn platform_paths_follow_zed_conventions() {
    let home = Path::new("/Users/builder");
    let xdg = Path::new("/private/config");
    let app_data = Path::new("/private/windows-app-data");

    assert_eq!(
        settings_path_for_platform(ZedPlatform::Macos, home, None, None)
            .expect("macOS path should resolve"),
        Path::new("/Users/builder/.config/zed/settings.json")
    );
    assert_eq!(
        settings_path_for_platform(ZedPlatform::Linux, home, Some(xdg), None)
            .expect("XDG path should resolve"),
        Path::new("/private/config/zed/settings.json")
    );
    assert_eq!(
        settings_path_for_platform(ZedPlatform::Windows, home, None, Some(app_data))
            .expect("Windows path should resolve"),
        Path::new("/private/windows-app-data/Zed/settings.json")
    );
    assert!(matches!(
        settings_path_for_platform(ZedPlatform::Windows, home, None, None),
        Err(ZedDesktopError::MissingPlatformDirectory)
    ));
}

#[test]
fn process_detection_distinguishes_main_processes_from_helpers() {
    for command in [
        "/Applications/Zed.app/Contents/MacOS/zed",
        "/usr/local/bin/zed /workspace",
        "zeditor --foreground",
        "C:\\Zed\\zed.exe",
    ] {
        assert!(command_is_zed_main(command), "should detect {command}");
    }
    for command in [
        "/Applications/Zed.app/Contents/MacOS/cli --wait",
        "/Applications/Zed.app/Contents/MacOS/zed --type=gpu-process",
        "unrelated-zed-helper",
    ] {
        assert!(!command_is_zed_main(command), "should ignore {command}");
    }
}

#[cfg(unix)]
#[test]
fn explicit_discovery_supports_each_platform_shape() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = tempfile::tempdir().expect("temporary root should exist");
    let plain = root.path().join("zed");
    fs::write(&plain, b"#!/bin/sh\n").expect("executable should be written");
    let mut permissions = fs::metadata(&plain)
        .expect("metadata should exist")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&plain, permissions).expect("executable bit should be set");
    assert_eq!(
        resolve_explicit(ZedPlatform::Linux, &plain),
        Some(plain.clone())
    );
    assert_eq!(
        resolve_explicit(ZedPlatform::Windows, &plain),
        Some(plain.clone())
    );

    let app = root.path().join("Zed.app");
    let cli = app.join("Contents/MacOS/cli");
    fs::create_dir_all(cli.parent().expect("CLI parent should exist"))
        .expect("app directories should exist");
    fs::write(&cli, b"#!/bin/sh\n").expect("CLI should be written");
    let mut permissions = fs::metadata(&cli)
        .expect("metadata should exist")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&cli, permissions).expect("executable bit should be set");
    assert_eq!(resolve_explicit(ZedPlatform::Macos, &app), Some(cli));
}

#[cfg(unix)]
#[tokio::test]
async fn zed_child_receives_only_the_session_token_as_its_nan_key() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = tempfile::tempdir().expect("temporary root should exist");
    let executable = root.path().join("zed");
    let capture = root.path().join("capture.txt");
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).expect("workspace should exist");
    fs::write(
        &executable,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$NAN_API_KEY\" \"$@\" > '{}'\n",
            capture.display()
        ),
    )
    .expect("fake Zed should be written");
    let mut permissions = fs::metadata(&executable)
        .expect("metadata should exist")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&executable, permissions).expect("executable bit should be set");
    let process = SystemZedProcess::new(Some(executable)).expect("process should resolve");

    let mut child = process
        .spawn(
            &workspace,
            &["--new".to_owned()],
            "launch-scoped-session-token",
        )
        .expect("fake Zed should start");
    assert!(
        child
            .wait()
            .await
            .expect("fake Zed should finish")
            .success()
    );
    let captured = fs::read_to_string(capture).expect("capture should be readable");
    let lines = captured.lines().collect::<Vec<_>>();

    assert_eq!(lines[0], "launch-scoped-session-token");
    assert_eq!(lines[1..4], ["--foreground", "--wait", "--new"]);
    assert_eq!(lines[4], workspace.to_string_lossy());
    assert!(!captured.contains("provider-key-marker"));
}
