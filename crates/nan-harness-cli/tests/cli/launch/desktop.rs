use std::process::Command;

#[test]
fn desktop_dry_runs_are_offline_inert_and_typed() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let state = directory.path().join("state-that-must-not-exist");
    let hermes_home = directory.path().join("hermes-that-must-not-exist");
    for (arguments, harness, transport) in [
        (
            vec!["chatgpt-desktop", "--model", "qwen3.6", "--dry-run"],
            "chatgpt-desktop",
            "responses-bridge",
        ),
        (
            vec!["claude-desktop", "--force-search", "--dry-run"],
            "claude-desktop",
            "anthropic-bridge",
        ),
        (
            vec![
                "hermes-desktop",
                "--no-chat-gateway",
                "--dry-run",
                "--",
                "--source",
                "local",
            ],
            "hermes-desktop",
            "direct-chat-completions",
        ),
        (
            vec!["pen", "--model", "qwen3.6", "--dry-run"],
            "pen-desktop",
            "chat-completions-gateway",
        ),
        (
            vec!["zed", "--model", "qwen3.6", "--dry-run"],
            "zed-desktop",
            "chat-completions-gateway",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
            .args(arguments)
            .env("NAN_HARNESS_CONFIG_DIR", &state)
            .env("HERMES_HOME", &hermes_home)
            .env_remove("NAN_API_KEY")
            .env("NAN_BASE_URL", "not-a-valid-provider-url")
            .output()
            .expect("Desktop dry run should start");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{harness}: {stderr}");
        let plan: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("dry run should print JSON");
        assert_eq!(plan["schemaVersion"], 1);
        assert_eq!(plan["harness"], harness);
        assert_eq!(plan["transport"], transport);
        assert_eq!(plan["experimental"], true);
        assert!(plan.get("credential").is_none());
        assert!(!state.exists(), "{harness} dry run wrote state");
        assert!(!hermes_home.exists(), "{harness} dry run wrote a profile");
    }
}

#[test]
fn zed_dry_run_redacts_private_launch_inputs() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let state = directory.path().join("state-that-must-not-exist");
    let private_workspace = directory.path().join("private-workspace-marker");
    let private_executable = directory.path().join("private-executable-marker");
    let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
        .args([
            "zed",
            "--model",
            "qwen3.6",
            "--user-data-dir",
            "private-profile-marker",
            "--executable",
            private_executable
                .to_str()
                .expect("temporary path should be UTF-8"),
            "--dry-run",
            private_workspace
                .to_str()
                .expect("temporary path should be UTF-8"),
            "--",
            "--private-option",
            "private-argument-marker",
        ])
        .env("NAN_HARNESS_CONFIG_DIR", &state)
        .env_remove("NAN_API_KEY")
        .output()
        .expect("Zed dry run should start");
    let stdout = String::from_utf8(output.stdout).expect("dry run should be UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

    assert!(output.status.success(), "{stderr}");
    assert!(stdout.contains("<explicit-executable>"));
    assert!(stdout.contains("<workspace>"));
    assert!(stdout.contains("<2 native arguments>"));
    assert!(!stdout.contains("private-workspace-marker"));
    assert!(!stdout.contains("private-executable-marker"));
    assert!(!stdout.contains("private-profile-marker"));
    assert!(!stdout.contains("private-argument-marker"));
    assert!(!stdout.contains("NAN_API_KEY"));
    assert!(!state.exists());
}

#[test]
fn zed_provider_override_is_inert_in_dry_run_and_conflicts_with_restore() {
    for harness in ["zed", "zed-desktop"] {
        let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
            .args([
                harness,
                "--provider-base-url",
                "private-invalid-url",
                "--dry-run",
            ])
            .env_remove("NAN_API_KEY")
            .output()
            .expect("dry run should start");
        assert!(output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("private-invalid-url"));
        let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
            .args([
                harness,
                "--provider-base-url",
                "http://127.0.0.1:1",
                "--restore",
            ])
            .output()
            .expect("argument validation should start");
        assert_eq!(output.status.code(), Some(2));
    }
}

#[cfg(unix)]
#[test]
fn zed_launch_discovers_models_only_at_the_explicit_provider() {
    use std::os::unix::fs::PermissionsExt as _;

    let directory = tempfile::tempdir().expect("isolated home");
    let executable = directory.path().join("synthetic-editor");
    std::fs::write(
        &executable,
        "#!/bin/sh\n[ \"$1\" = --version ] || exit 99\necho 'Zed 1.18.0'\n",
    )
    .expect("write synthetic version command");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("make version command executable");
    let (endpoint, stop, requests) = crate::support::monitor_http_requests();
    let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
        .env_clear()
        .args([
            "zed",
            "--provider-base-url",
            &endpoint,
            "--model",
            "absent-test-model",
            "--executable",
        ])
        .arg(&executable)
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", directory.path())
        .env("NAN_HARNESS_CONFIG_DIR", directory.path().join("state"))
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env("NAN_API_KEY", "synthetic-routing-key")
        .env("HTTPS_PROXY", "http://127.0.0.1:1")
        .env("HTTP_PROXY", "http://127.0.0.1:1")
        .env("NO_PROXY", "127.0.0.1")
        .output()
        .expect("isolated Zed launch should run");
    stop.send(()).expect("stop synthetic provider");
    let requests = requests.join().expect("provider requests");
    assert!(
        !output.status.success(),
        "an unavailable model must stop before launch"
    );
    assert!(
        !requests.is_empty(),
        "explicit provider must receive model discovery: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    for request in requests {
        assert!(request.starts_with("GET /models ") || request.starts_with("GET /v1/models "));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer synthetic-routing-key")
        );
    }
}
