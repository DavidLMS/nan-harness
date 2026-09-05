#[cfg(unix)]
use std::env;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

use crate::fixtures::serve_release;
use crate::platform::{binary_file_name, conflicting_nanh_path};
#[cfg(unix)]
use crate::support::assert_curl_option;
use crate::support::run_installer;

#[cfg(unix)]
#[test]
fn release_installer_bounds_download_failures_and_reports_them() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let install_directory = directory.path().join("bin");
    let state_directory = directory.path().join("state");
    let tool_directory = directory.path().join("tools");
    let curl_arguments = directory.path().join("curl-arguments.txt");
    fs::create_dir_all(&home).expect("isolated home should exist");
    fs::create_dir_all(&tool_directory).expect("tool directory should exist");

    let fake_curl = tool_directory.join("curl");
    fs::write(
        &fake_curl,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$NAN_TEST_CURL_ARGUMENTS\"\nexit 28\n",
    )
    .expect("fake curl should be writable");
    let mut permissions = fs::metadata(&fake_curl)
        .expect("fake curl metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_curl, permissions).expect("fake curl should be executable");

    let inherited_path = env::var_os("PATH").unwrap_or_default();
    let path = env::join_paths(
        std::iter::once(tool_directory.clone()).chain(env::split_paths(&inherited_path)),
    )
    .expect("test PATH should be valid");
    let mut command = crate::support::installer_process(
        directory.path(),
        &home,
        &install_directory,
        &state_directory,
        "https://example.invalid",
    );
    command
        .env("PATH", path)
        .env("NAN_TEST_CURL_ARGUMENTS", &curl_arguments);

    let output = command.output().expect("release installer should start");
    assert!(
        !output.status.success(),
        "installer should fail when the release cannot be downloaded"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("could not download nan-harness-"),
        "unexpected stderr: {stderr}"
    );
    let curl_arguments = fs::read_to_string(curl_arguments)
        .expect("fake curl should record the arguments it received");
    assert_curl_option(&curl_arguments, "--connect-timeout", "10");
    assert_curl_option(&curl_arguments, "--max-time", "120");
    assert_curl_option(&curl_arguments, "--retry-max-time", "10");
    assert!(!install_directory.exists());
}

#[test]
fn release_installer_rejects_an_unrelated_nanh_before_replacing_the_binary() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let install_directory = directory.path().join("bin");
    let state_directory = directory.path().join("state");
    fs::create_dir_all(&home).expect("isolated home should exist");
    fs::create_dir_all(&install_directory).expect("install directory should exist");
    let alias = conflicting_nanh_path(&install_directory);
    fs::write(&alias, b"unrelated nanh command")
        .expect("unrelated nanh command should be writable");
    let binary = install_directory.join(binary_file_name());
    fs::write(&binary, b"existing canonical binary")
        .expect("existing canonical binary should be writable");

    let (base_url, server) = serve_release();
    let output = run_installer(
        directory.path(),
        &home,
        &install_directory,
        &state_directory,
        &base_url,
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("exists and is not the nan-harness command alias")
    );
    server
        .join()
        .expect("release server should finish")
        .expect("release server should deliver every file");
    assert_eq!(
        fs::read(&alias).expect("unrelated nanh command should remain readable"),
        b"unrelated nanh command"
    );
    assert_eq!(
        fs::read(&binary).expect("existing canonical binary should remain readable"),
        b"existing canonical binary"
    );
}
