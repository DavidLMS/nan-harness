use std::process::{Command, Output};

fn isolated(arguments: &[&str], root: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nanh"))
        .args(arguments)
        .env("PATH", root.join("empty-path"))
        .env("HOME", root)
        .env("USERPROFILE", root)
        .env("APPDATA", root.join("AppData/Roaming"))
        .env("NAN_HARNESS_STATE_DIR", root.join("state"))
        .env("NAN_HARNESS_LANG", "en")
        .env_remove("NAN_UPDATE_MANIFEST_URL")
        .env_remove("NAN_HARNESS_GLITCHTIP_DSN")
        .output()
        .expect("CLI starts without an installed launcher")
}

#[test]
fn gentle_shell_missing_shows_install_command_without_running_npm() {
    let root = tempfile::tempdir().unwrap();
    for dry_run in [false, true] {
        let arguments = if dry_run {
            vec!["gentle-shell", "--dry-run"]
        } else {
            vec!["gentle-shell"]
        };
        let output = isolated(&arguments, root.path());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert_eq!(output.status.success(), dry_run, "{stderr}");
        assert!(
            stderr.contains("npm install --global gentle-pi@latest"),
            "{stderr}"
        );
        assert!(stderr.contains("nanh gentle-shell"), "{stderr}");
        assert!(
            !stderr.contains("NH-INSTALL"),
            "must not invoke npm: {stderr}"
        );
        assert!(!stderr.contains("anonymous error report"), "{stderr}");
    }
}

#[test]
fn gentle_shell_explicit_bad_executable_never_offers_an_installation() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing-launcher");
    let output = isolated(
        &[
            "gentle-shell",
            "--dry-run",
            "--executable",
            missing.to_str().unwrap(),
        ],
        root.path(),
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stderr.contains("npm install"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn gentle_shell_known_npm_location_reaches_version_validation() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join(".npm-global/bin");
    std::fs::create_dir_all(&directory).unwrap();
    let executable = directory.join("gentle-shell");
    // A failing version probe distinguishes discovery from launch without installing Node.
    std::fs::write(&executable, "#!/bin/sh\nexit 27\n").unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    for arguments in [
        vec!["gentle-shell", "--dry-run"],
        vec!["doctor", "gentle-shell", "--offline"],
    ] {
        let output = isolated(&arguments, root.path());
        assert!(!output.status.success());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("27"), "{stderr}");
        assert!(!stderr.contains("npm install"), "{stderr}");
    }
}
