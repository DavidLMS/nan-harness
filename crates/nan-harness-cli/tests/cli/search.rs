use crate::support::run_with_embedded_compatibility;
use nan_harness_runtime::search_docker::DockerSearchPaths;
use nan_harness_runtime::searxng::{SearxngInstallPaths, SearxngPlatform};
use nan_harness_runtime::{SearchInterest, active_search_interests};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn isolated_search_command(directory: &Path, arguments: &[&str]) -> Command {
    // Search state is derived from the child home on every supported platform;
    // keep both Unix and Windows conventions inside the temporary fixture.
    let home = directory.join("home");
    let mut command = Command::new(env!("CARGO_BIN_EXE_nan-harness"));
    command
        .args(arguments)
        .env("NAN_HARNESS_CONFIG_DIR", directory)
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("APPDATA", home.join("AppData/Roaming"))
        .env("XDG_CONFIG_HOME", home.join(".config"));
    command
}

fn run_isolated_search(directory: &Path, arguments: &[&str]) -> Output {
    isolated_search_command(directory, arguments)
        .output()
        .expect("search command should run")
}

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
fn search_setup_without_a_backend_prints_choices_without_changing_state() {
    let directory = tempfile::tempdir().expect("state directory should exist");
    let output = run_isolated_search(directory.path(), &["search", "setup"]);

    let stdout = String::from_utf8(output.stdout).expect("setup guidance should be UTF-8");
    assert!(output.status.success());
    for choice in ["--local", "--docker", "--url"] {
        assert!(stdout.contains(choice), "missing {choice}: {stdout}");
    }
    assert!(stdout.contains("Credentials are not required"));
    assert!(!directory.path().join("search.json").exists());
}

#[test]
fn search_status_is_json_and_does_not_require_nan_credentials() {
    let directory = tempfile::tempdir().expect("state directory should exist");
    let foreign_home = tempfile::tempdir().expect("foreign home should exist");
    let platform = SearxngPlatform::current().expect("test platform should support SearXNG");
    let foreign_local = SearxngInstallPaths::for_user_home(foreign_home.path(), platform);
    let foreign_docker = DockerSearchPaths::for_user_home(foreign_home.path());
    fs::create_dir_all(foreign_local.root()).expect("foreign local root should exist");
    fs::create_dir_all(foreign_docker.root()).expect("foreign Docker root should exist");
    let local_interest = SearchInterest::acquire(foreign_local.root())
        .expect("foreign local session should be acquired");
    let docker_interest = SearchInterest::acquire(foreign_docker.root())
        .expect("foreign Docker session should be acquired");
    assert_eq!(
        active_search_interests(foreign_local.root()).expect("local interest should be visible"),
        1
    );
    assert_eq!(
        active_search_interests(foreign_docker.root()).expect("Docker interest should be visible"),
        1
    );

    let output = isolated_search_command(directory.path(), &["search", "status", "--json"])
        .env("NAN_API_KEY", "not-a-credential")
        .output()
        .expect("search status should run");

    assert!(output.status.success());
    let status: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("status should contain only JSON");
    assert_eq!(status["configured"], false);
    assert_eq!(status["enabled"], false);
    assert_eq!(status["mode"], serde_json::Value::Null);
    assert_eq!(status["version"], serde_json::Value::Null);
    assert_eq!(status["state"], "disabled");
    assert_eq!(status["interestedSessions"], 0);
    assert_eq!(status["problem"], serde_json::Value::Null);
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {:?}",
        output.stderr
    );
    drop(local_interest);
    drop(docker_interest);
}

#[test]
fn search_setup_failing_endpoint_does_not_publish_configuration() {
    let directory = tempfile::tempdir().expect("state directory should exist");
    let output = run_isolated_search(
        directory.path(),
        &["search", "setup", "--url", "https://127.0.0.1:1"],
    );

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("verify"));
    assert!(!directory.path().join("search.json").exists());
}

#[test]
fn search_status_reports_remote_problem_without_starting_a_backend() {
    let directory = tempfile::tempdir().expect("state directory should exist");
    fs::write(
        directory.path().join("search.json"),
        br#"{"schemaVersion":1,"mode":"remote","baseUrl":"https://127.0.0.1:1/"}"#,
    )
    .expect("search config should write");
    let output = run_isolated_search(directory.path(), &["search", "status", "--json"]);

    assert!(output.status.success());
    let status: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("status should contain only JSON");
    assert_eq!(status["mode"], "remote");
    assert_eq!(status["state"], "unavailable");
    assert!(status["problem"].is_string());
}

#[test]
fn search_setup_rejects_insecure_urls_before_persisting_state() {
    let directory = tempfile::tempdir().expect("state directory should exist");
    let output = run_isolated_search(
        directory.path(),
        &["search", "setup", "--url", "http://search.example.test"],
    );

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
    let output = run_isolated_search(directory.path(), &["search", "disable"]);

    assert!(output.status.success());
    assert!(!directory.path().join("search.json").exists());
    assert!(String::from_utf8_lossy(&output.stdout).contains("disabled"));
}
