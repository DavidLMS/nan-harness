use std::fs;

use crate::fixtures::serve_release;
use crate::platform::{
    alias_path, assert_alias, assert_success, assert_version, binary_file_name,
    previous_alias_path, unrelated_nan_paths, write_previous_managed_alias,
};
use crate::support::{
    assert_installation_receipt, isolated_alias_command, isolated_command, path_exists,
    run_installer, wait_until_removed,
};

#[test]
fn release_installer_installs_the_binary_and_alias() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let install_directory = directory.path().join("bin");
    let state_directory = directory.path().join("state");
    fs::create_dir_all(&home).expect("isolated home should exist");
    fs::create_dir_all(&install_directory).expect("install directory should exist");
    let unrelated_nan = unrelated_nan_paths(&install_directory);
    for path in &unrelated_nan {
        fs::write(path, b"unrelated nan command")
            .expect("unrelated nan command should be writable");
    }

    let (base_url, server) = serve_release();
    let output = run_installer(
        directory.path(),
        &home,
        &install_directory,
        &state_directory,
        &base_url,
    );
    assert_success("installer", &output);
    let server_result = server.join().expect("release server should finish");
    server_result.expect("release server should deliver every file");

    let binary = install_directory.join(binary_file_name());
    assert_version(&binary);
    assert_alias(&install_directory);
    assert_installation_receipt(&state_directory, &binary, &install_directory);

    let mut command = isolated_command(&binary, directory.path(), &home, &state_directory);
    command
        .args(["__record-installation", "--executable"])
        .arg(&binary)
        .arg("--alias")
        .arg(alias_path(&install_directory));
    let output = command
        .output()
        .expect("installed binary should refresh its receipt");
    assert_success("receipt refresh", &output);

    let mut command = isolated_alias_command(
        &install_directory,
        directory.path(),
        &home,
        &state_directory,
    );
    command.arg("uninstall");
    let output = command
        .output()
        .expect("uninstall should enforce confirmation");
    let stderr = String::from_utf8(output.stderr).expect("uninstall error should be UTF-8");
    assert!(!output.status.success());
    assert!(stderr.contains("error [NH-UNINSTALL-001]"));
    assert!(binary.exists());
    assert!(state_directory.exists());

    fs::write(state_directory.join("test-state"), b"managed data")
        .expect("application state should be writable");
    let mut command = isolated_alias_command(
        &install_directory,
        directory.path(),
        &home,
        &state_directory,
    );
    command.args(["uninstall", "--yes"]);
    let output = command
        .output()
        .expect("installed binary should uninstall itself");
    assert_success("uninstall", &output);
    wait_until_removed(&binary);
    assert!(!binary.exists());
    assert!(!alias_path(&install_directory).exists());
    assert!(!state_directory.exists());
    assert!(!home.join(".hermes/profiles/nan").exists());
    for path in unrelated_nan {
        assert_eq!(
            fs::read(path).expect("unrelated nan command should remain readable"),
            b"unrelated nan command"
        );
    }
}

#[test]
fn release_installer_migrates_the_previous_managed_nan_alias() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let install_directory = directory.path().join("bin");
    let state_directory = directory.path().join("state");
    fs::create_dir_all(&home).expect("isolated home should exist");
    fs::create_dir_all(&install_directory).expect("install directory should exist");
    write_previous_managed_alias(&install_directory);

    let (base_url, server) = serve_release();
    let output = run_installer(
        directory.path(),
        &home,
        &install_directory,
        &state_directory,
        &base_url,
    );
    assert_success("installer migration", &output);
    server
        .join()
        .expect("release server should finish")
        .expect("release server should deliver every file");
    assert!(!path_exists(&previous_alias_path(&install_directory)));
    assert_version(&install_directory.join(binary_file_name()));
    assert_alias(&install_directory);
}

#[test]
fn updated_binary_uninstalls_with_the_previous_managed_alias_receipt() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let install_directory = directory.path().join("bin");
    let state_directory = directory.path().join("state");
    fs::create_dir_all(&home).expect("isolated home should exist");
    fs::create_dir_all(&install_directory).expect("install directory should exist");
    fs::create_dir_all(&state_directory).expect("state directory should exist");
    let binary = install_directory.join(binary_file_name());
    fs::copy(env!("CARGO_BIN_EXE_nan-harness"), &binary)
        .expect("installed binary should be copied");
    write_previous_managed_alias(&install_directory);
    let previous_alias = previous_alias_path(&install_directory);
    let receipt = serde_json::json!({
        "schemaVersion": 1,
        "executablePath": binary,
        "aliasPath": previous_alias,
        "userPathEntryAdded": false
    });
    fs::write(
        state_directory.join("installation.json"),
        serde_json::to_vec_pretty(&receipt).expect("receipt should serialize"),
    )
    .expect("receipt should be writable");

    let mut command = isolated_command(&binary, directory.path(), &home, &state_directory);
    command.args(["uninstall", "--yes"]);
    let output = command
        .output()
        .expect("updated binary should uninstall itself");
    assert_success("previous alias uninstall", &output);
    wait_until_removed(&binary);
    wait_until_removed(&previous_alias);
    wait_until_removed(&state_directory);
    assert!(!binary.exists());
    assert!(!path_exists(&previous_alias));
    assert!(!state_directory.exists());
}

#[test]
fn release_installer_preserves_unrelated_nan_command() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let install_directory = directory.path().join("bin");
    let state_directory = directory.path().join("state");
    fs::create_dir_all(&home).expect("isolated home should exist");
    fs::create_dir_all(&install_directory).expect("install directory should exist");
    let unrelated = unrelated_nan_paths(&install_directory);
    for path in &unrelated {
        fs::write(path, b"unrelated nan command")
            .expect("unrelated nan command should be writable");
    }

    let (base_url, server) = serve_release();
    let output = run_installer(
        directory.path(),
        &home,
        &install_directory,
        &state_directory,
        &base_url,
    );
    assert_success("installer with unrelated nan", &output);
    server
        .join()
        .expect("release server should finish")
        .expect("release server should deliver every file");
    for path in unrelated {
        assert_eq!(
            fs::read(path).expect("unrelated command should remain readable"),
            b"unrelated nan command"
        );
    }
    assert_version(&install_directory.join(binary_file_name()));
    assert_alias(&install_directory);
}
