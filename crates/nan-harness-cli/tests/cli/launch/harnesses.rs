use crate::support::{fake_harness, run, run_with_embedded_compatibility};
use std::process::Command;

#[cfg(unix)]
#[test]
fn direct_harness_dry_runs_build_safe_native_overlays() {
    let cases = [
        ("opencode", "1.18.4", "NAN_API_KEY", "nan/qwen3.6"),
        ("hermes", "0.20.2", "NAN_API_KEY", "{artifact:hermes-home}"),
        (
            "pi",
            "0.84.2",
            "NAN_API_KEY",
            "{artifact:pi-provider-extension}",
        ),
        (
            "omp",
            "18.0.11",
            "NAN_API_KEY",
            "{artifact:omp-provider-extension}",
        ),
        (
            "prime-agent",
            "0.7.2",
            "NAN_API_KEY",
            "{artifact:pi-provider-extension}",
        ),
        (
            "dsh",
            "0.1.0-rc.7",
            "NAN_API_KEY",
            "{artifact:deepseek-harness-patch}",
        ),
        (
            "openclaw",
            "2026.7.1-2",
            "NAN_API_KEY",
            "{artifact:openclaw-config}",
        ),
        (
            "cline",
            "3.0.55",
            "OPENAI_API_KEY",
            "{artifact:cline-config}",
        ),
        ("qwen", "0.21.13", "OPENAI_API_KEY", "OPENAI_MODEL"),
        (
            "kimi",
            "0.36.1",
            "KIMI_MODEL_API_KEY",
            "KIMI_MODEL_DISPLAY_NAME",
        ),
        (
            "aider",
            "aider 0.86.2",
            "AIDER_OPENAI_API_KEY",
            "AIDER_OPENAI_API_BASE",
        ),
        ("goose", "goose 1.46.0", "OPENAI_API_KEY", "GOOSE_PROVIDER"),
    ];

    for (harness, version, credential_target, marker) in cases {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let executable = fake_harness(directory.path(), version);
        let empty_path = directory.path().join("empty-path");
        std::fs::create_dir(&empty_path).expect("empty PATH should exist");
        let mut command = Command::new(env!("CARGO_BIN_EXE_nan-harness"));
        command
            .args([
                harness,
                "--executable",
                executable.to_str().expect("path should be UTF-8"),
                "--dry-run",
            ])
            .env("NAN_HARNESS_CONFIG_DIR", directory.path().join("nan-state"))
            .env("NAN_NO_COMPATIBILITY_CHECK", "1")
            .env_remove("NAN_API_KEY")
            .env("HOME", directory.path().join("home"));
        if harness == "dsh" {
            command.env("PATH", empty_path);
        }
        let output = command.output().expect("nanh should start");
        let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
        let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

        assert!(output.status.success(), "{harness}: {stderr}");
        assert!(stdout.contains("\"kind\": \"direct-chat\""));
        assert!(stdout.contains("{runtime:provider_base_url}"));
        assert!(stdout.contains(credential_target));
        assert!(
            stdout.contains(marker),
            "{harness}: expected marker {marker}, got:\n{stdout}"
        );
        assert!(!stdout.contains("nan-secret-value"));
    }
}

#[cfg(unix)]
#[test]
fn gateway_escape_hatch_dry_run_explains_its_effect() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = fake_harness(directory.path(), "0.84.2");
    let output = run_with_embedded_compatibility(&[
        "pi",
        "--executable",
        executable.to_str().expect("path should be UTF-8"),
        "--no-chat-gateway",
        "--dry-run",
    ]);
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

    assert!(output.status.success(), "{stderr}");
    assert!(stdout.contains("\"kind\": \"direct-chat\""));
    assert!(stderr.contains("gateway would be disabled for this launch"));
    assert!(stderr.contains("provider credential directly"));
    assert!(stderr.contains("usage accounting and gateway-dependent features"));
}

#[cfg(unix)]
#[test]
fn harness_aliases_remain_executable() {
    let cases = [
        ("claude-code", "2.1.233 (Claude Code)", "claude-code"),
        ("oh-my-pi", "18.0.11", "omp"),
        ("prime", "0.7.2", "prime-agent"),
        ("deepseek", "0.1.0-rc.7", "deepseek-harness"),
        ("deepseek-harness", "0.1.0-rc.7", "deepseek-harness"),
        ("qwen-code", "0.21.13", "qwen-code"),
        ("kimi-code", "0.36.1", "kimi-code"),
    ];

    for (command, version, harness_kind) in cases {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let executable = fake_harness(directory.path(), version);
        let output = run(&[
            command,
            "--executable",
            executable.to_str().expect("path should be UTF-8"),
            "--dry-run",
        ]);
        let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
        let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

        assert!(output.status.success(), "{command}: {stderr}");
        let plan: serde_json::Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|error| panic!("{command} should print a JSON plan: {error}"));

        assert_eq!(plan["harness"]["kind"], harness_kind, "{command}");
    }
}

#[cfg(unix)]
#[test]
fn codex_dry_run_builds_a_safe_responses_bridge_plan() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = fake_harness(directory.path(), "codex-cli 0.146.0");
    let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
        .args([
            "codex",
            "--executable",
            executable.to_str().expect("path should be UTF-8"),
            "--dry-run",
        ])
        .env_remove("NAN_API_KEY")
        .output()
        .expect("nanh should start");
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

    assert!(output.status.success(), "{stderr}");
    assert!(stdout.contains("\"kind\": \"responses-bridge\""));
    assert!(stdout.contains("{runtime:bridge_base_url}/v1"));
    assert!(stdout.contains("NAN_HARNESS_SESSION_TOKEN"));
    assert!(stdout.contains("supports_standalone_web_search=true"));
    assert!(!stdout.contains("nan-secret-value"));
}
