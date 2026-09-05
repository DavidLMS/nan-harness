use std::process::Command;

#[test]
fn uninstall_requires_an_installer_managed_executable() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_nanh"));
    let output = Command::new(binary)
        .args(["uninstall", "--yes"])
        .env("HOME", directory.path().join("home"))
        .env("NAN_HARNESS_CONFIG_DIR", directory.path().join("state"))
        .output()
        .expect("uninstall should start");
    let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

    assert!(!output.status.success());
    assert!(stderr.contains("error [NH-UNINSTALL-002]"));
    assert!(stderr.contains("not managed by the release installer"));
    assert!(binary.exists());
}

#[test]
fn manual_update_explains_when_a_build_has_no_release_channel() {
    let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
        .arg("update")
        .env_remove("NAN_UPDATE_MANIFEST_URL")
        .output()
        .expect("nanh update should start");
    let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

    assert!(!output.status.success());
    assert!(stderr.contains("error [NH-UPDATE-001]"));
    assert!(stderr.contains("does not have an update channel configured"));
}

#[cfg(unix)]
#[test]
fn uninstall_kimi_script_removes_binaries_and_optionally_user_data() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let kimi_home = home.path().join(".kimi-code");
    std::fs::create_dir_all(kimi_home.join("bin")).expect("Kimi bin directory should exist");
    std::fs::write(kimi_home.join("bin/kimi"), "fake kimi").expect("binary should exist");
    std::fs::write(kimi_home.join("config.toml"), "user config").expect("config should exist");
    std::fs::write(
        home.path().join(".zshrc"),
        "export PATH=\"$HOME/.kimi-code/bin:$PATH\"\nkeep=true\n",
    )
    .expect("shell configuration should exist");

    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/uninstall-kimi.sh");
    let output = Command::new("bash")
        .arg(&script)
        .env("HOME", home.path())
        .output()
        .expect("uninstall helper should run");
    assert!(output.status.success());
    assert!(!kimi_home.join("bin/kimi").exists());
    assert!(kimi_home.join("config.toml").exists());
    let shell_config = std::fs::read_to_string(home.path().join(".zshrc"))
        .expect("shell configuration should remain");
    assert_eq!(shell_config, "keep=true\n");

    std::fs::write(kimi_home.join("bin/kimi"), "fake kimi").expect("binary should exist");
    let output = Command::new("bash")
        .args([
            script.to_str().expect("script path should be UTF-8"),
            "--purge",
            "--yes",
        ])
        .env("HOME", home.path())
        .output()
        .expect("purge helper should run");
    assert!(output.status.success());
    assert!(!kimi_home.exists());
}

#[cfg(unix)]
#[test]
fn uninstall_kimi_script_separates_install_and_data_directories() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let install_directory = home.path().join("custom-kimi-install");
    let data_directory = home.path().join("custom-kimi-data");
    std::fs::create_dir_all(install_directory.join("bin"))
        .expect("Kimi install directory should exist");
    std::fs::create_dir_all(&data_directory).expect("Kimi data directory should exist");
    std::fs::write(install_directory.join("bin/kimi"), "fake kimi").expect("binary should exist");
    std::fs::write(data_directory.join("config.toml"), "user config").expect("config should exist");
    std::fs::write(
        home.path().join(".profile"),
        format!(
            "export PATH=\"{}/bin:$PATH\"\nkeep=true\n",
            install_directory.display()
        ),
    )
    .expect("shell configuration should exist");

    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/uninstall-kimi.sh");
    let output = Command::new("bash")
        .arg(&script)
        .env("HOME", home.path())
        .env("KIMI_INSTALL_DIR", &install_directory)
        .env("KIMI_CODE_HOME", &data_directory)
        .output()
        .expect("uninstall helper should run");

    assert!(output.status.success());
    assert!(!install_directory.join("bin/kimi").exists());
    assert!(data_directory.join("config.toml").exists());
    let shell_config = std::fs::read_to_string(home.path().join(".profile"))
        .expect("shell configuration should remain");
    assert_eq!(shell_config, "keep=true\n");
}

#[cfg(unix)]
#[test]
fn uninstall_kimi_script_rejects_home_with_a_trailing_slash_as_data_directory() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let sentinel = home.path().join("keep.txt");
    std::fs::write(&sentinel, "keep").expect("sentinel should exist");
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/uninstall-kimi.sh");
    let unsafe_data_directory = format!("{}/", home.path().display());

    let output = Command::new("bash")
        .args([
            script.to_str().expect("script path should be UTF-8"),
            "--purge",
            "--yes",
        ])
        .env("HOME", home.path())
        .env("KIMI_CODE_HOME", unsafe_data_directory)
        .output()
        .expect("uninstall helper should run");

    assert!(!output.status.success());
    assert!(sentinel.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsafe Kimi Code home"));
}
