#[cfg(unix)]
use super::{PenDesktopError, SystemPenProcess, process_matches};
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use tempfile::tempdir;

#[cfg(unix)]
fn immediate_exit_script(directory: &Path, name: &str, code: i32) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let script = directory.join(name);
    fs::write(&script, format!("#!/bin/sh\nexit {code}\n"))
        .expect("synthetic process script should be written");
    let mut permissions = fs::metadata(&script)
        .expect("synthetic process script should be readable")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions)
        .expect("synthetic process script should be executable");
    script
}

#[cfg(unix)]
fn script_path(path: &Path) -> &str {
    path.to_str()
        .expect("temporary script path should be UTF-8")
}

#[cfg(unix)]
#[test]
fn process_matches_maps_success_and_not_running_statuses() {
    let directory = tempdir().expect("temporary process directory should be created");
    let running = immediate_exit_script(directory.path(), "running", 0);
    let not_running = immediate_exit_script(directory.path(), "not-running", 1);

    assert!(matches!(
        process_matches(script_path(&running), &[]),
        Ok(true)
    ));
    assert!(matches!(
        process_matches(script_path(&not_running), &[]),
        Ok(false)
    ));
}

#[cfg(unix)]
#[test]
fn process_matches_rejects_unexpected_and_missing_statuses() {
    let directory = tempdir().expect("temporary process directory should be created");
    let unexpected = immediate_exit_script(directory.path(), "unexpected", 2);
    let missing = directory.path().join("missing");

    assert!(matches!(
        process_matches(script_path(&unexpected), &[]),
        Err(PenDesktopError::ProcessCheckFailed(Some(2)))
    ));
    assert!(matches!(
        process_matches(script_path(&missing), &[]),
        Err(PenDesktopError::ProcessCheck(_))
    ));
}

#[cfg(target_os = "macos")]
#[test]
fn explicit_executable_is_available_without_launching_pen() {
    let directory = tempdir().expect("temporary Pen directory should be created");
    let executable = directory.path().join("Pen");
    fs::write(&executable, b"synthetic executable")
        .expect("synthetic executable should be written");

    let process = SystemPenProcess::new(Some(executable)).expect("macOS should be supported");

    assert!(process.ensure_available().is_ok());
}

#[cfg(target_os = "macos")]
#[test]
fn explicit_app_fixture_reports_its_bundle_version() {
    let directory = tempdir().expect("temporary Pen directory should be created");
    let app = directory.path().join("Pen.app");
    let executable = app.join("Contents/MacOS/Pen");
    let info = app.join("Contents/Info.plist");
    fs::create_dir_all(
        executable
            .parent()
            .expect("synthetic executable should have a parent"),
    )
    .expect("synthetic app layout should be created");
    fs::write(&executable, b"synthetic executable")
        .expect("synthetic executable should be written");
    fs::write(
        &info,
        br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>CFBundleShortVersionString</key><string>1.2.3</string></dict></plist>
"#,
    )
    .expect("synthetic Info.plist should be written");

    let process = SystemPenProcess::new(Some(app)).expect("macOS should be supported");

    assert_eq!(
        process
            .installed_version()
            .map(|version| version.to_string()),
        Some("1.2.3".into())
    );
}
