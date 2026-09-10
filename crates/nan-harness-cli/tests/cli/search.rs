use crate::support::run_with_embedded_compatibility;
use std::fs;

#[test]
fn search_help_lists_lifecycle_commands_and_backend_options() {
    let output = run_with_embedded_compatibility(&["search", "--help"]);
    let stdout = String::from_utf8(output.stdout).expect("help should be UTF-8");

    assert!(output.status.success());
    for command in ["setup", "status", "disable", "update", "remove"] {
        assert!(stdout.contains(command), "missing {command}: {stdout}");
    }

    let setup = run_with_embedded_compatibility(&["search", "setup", "--help"]);
    let setup_stdout = String::from_utf8(setup.stdout).expect("help should be UTF-8");
    assert!(setup.status.success());
    for option in ["--local", "--docker", "--url"] {
        assert!(
            setup_stdout.contains(option),
            "missing {option}: {setup_stdout}"
        );
    }
}

#[test]
fn search_status_is_json_and_does_not_require_nan_credentials() {
    let directory = tempfile::tempdir().expect("state directory should exist");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_nan-harness"))
        .args(["search", "status", "--json"])
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env("NAN_API_KEY", "not-a-credential")
        .output()
        .expect("search status should run");

    assert!(output.status.success());
    let status: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("status should contain only JSON");
    assert_eq!(status["configured"], false);
    assert_eq!(status["enabled"], false);
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {:?}",
        output.stderr
    );
}

#[test]
fn search_setup_rejects_insecure_urls_before_persisting_state() {
    let directory = tempfile::tempdir().expect("state directory should exist");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_nan-harness"))
        .args(["search", "setup", "--url", "http://search.example.test"])
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .output()
        .expect("search setup should run");

    assert!(!output.status.success());
    assert!(!directory.path().join("search.json").exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("HTTPS"));
}

#[test]
fn search_disable_removes_only_the_saved_endpoint() {
    let directory = tempfile::tempdir().expect("state directory should exist");
    fs::write(
        directory.path().join("search.json"),
        br#"{"schemaVersion":1,"mode":"remote","baseUrl":"https://search.example.test/"}"#,
    )
    .expect("search config should write");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_nan-harness"))
        .args(["search", "disable"])
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .output()
        .expect("search disable should run");

    assert!(output.status.success());
    assert!(!directory.path().join("search.json").exists());
    assert!(String::from_utf8_lossy(&output.stdout).contains("disabled"));
}
