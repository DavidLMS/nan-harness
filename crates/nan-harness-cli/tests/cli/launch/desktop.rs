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
    assert!(!stdout.contains("private-argument-marker"));
    assert!(!stdout.contains("NAN_API_KEY"));
    assert!(!state.exists());
}
