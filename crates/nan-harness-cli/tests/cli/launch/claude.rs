#[cfg(unix)]
use crate::support::{fake_claude, fake_claude_with_version, run, run_with_embedded_compatibility};
use std::process::Command;

#[cfg(unix)]
#[test]
fn claude_code_dry_run_builds_a_safe_bridge_plan_without_an_api_key() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = fake_claude(directory.path());
    let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
        .args([
            "claude",
            "--executable",
            executable.to_str().expect("path should be UTF-8"),
            "--dry-run",
            "--",
            "-p",
            "hello",
        ])
        .env_remove("NAN_API_KEY")
        .output()
        .expect("nanh should start");
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("\"kind\": \"anthropic-bridge\""));
    assert!(stdout.contains("{runtime:bridge_base_url}"));
    assert!(stdout.contains("{artifact:claude-settings}"));
    assert!(!stdout.contains("{runtime:claude_model_picker}"));
    assert!(!stdout.contains("nan-test-secret"));
}

#[cfg(unix)]
#[test]
fn claude_code_dry_run_enables_model_picker_for_supported_versions() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = fake_claude_with_version(directory.path(), "2.1.251 (Claude Code)");
    let output = run_with_embedded_compatibility(&[
        "claude",
        "--executable",
        executable.to_str().expect("path should be UTF-8"),
        "--dry-run",
    ]);
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("claude-model-picker"));
    assert!(stdout.contains("{runtime:claude_model_picker}"));
}

#[cfg(unix)]
#[test]
fn claude_code_dry_run_accepts_a_supported_non_default_nan_model() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = fake_claude(directory.path());
    let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
        .args([
            "claude",
            "--executable",
            executable.to_str().expect("path should be UTF-8"),
            "--model",
            "mimo-v2.5",
            "--dry-run",
        ])
        .env_remove("NAN_API_KEY")
        .output()
        .expect("nanh should start");
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("\"resolvedId\": \"mimo-v2.5\""));
    assert!(stdout.contains("anthropic/nan/mimo-v2.5"));
}

#[cfg(unix)]
#[test]
fn claude_code_dry_run_preserves_local_session_arguments() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = fake_claude(directory.path());
    let output = run(&[
        "claude",
        "--executable",
        executable.to_str().expect("path should be UTF-8"),
        "--dry-run",
        "--",
        "--resume",
        "auth-refactor",
        "--fork-session",
    ]);
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("\"--resume\""));
    assert!(stdout.contains("\"auth-refactor\""));
    assert!(stdout.contains("\"--fork-session\""));
}

#[cfg(unix)]
#[test]
fn claude_code_run_rejects_arguments_that_override_routing() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = fake_claude(directory.path());
    let output = run(&[
        "claude",
        "--executable",
        executable.to_str().expect("path should be UTF-8"),
        "--dry-run",
        "--",
        "--model",
        "other-model",
    ]);
    let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

    assert!(!output.status.success());
    assert!(stderr.contains("error [NH-PLAN-001]"));
    assert!(stderr.contains("conflicts with nan-harness routing"));
}

#[cfg(unix)]
#[test]
fn claude_code_dry_run_accepts_native_auto_mode_for_qwen() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = fake_claude(directory.path());
    let output = run(&[
        "claude",
        "--executable",
        executable.to_str().expect("path should be UTF-8"),
        "--dry-run",
        "--",
        "--permission-mode",
        "auto",
    ]);
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("\"--permission-mode\""));
    assert!(stdout.contains("\"auto\""));
    assert!(stdout.contains("\"opus\""));
}

#[cfg(unix)]
#[test]
fn claude_code_dry_run_warns_but_keeps_auto_on_newer_versions() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = fake_claude_with_version(directory.path(), "2.1.252 (Claude Code)");
    let output = run_with_embedded_compatibility(&[
        "claude",
        "--executable",
        executable.to_str().expect("path should be UTF-8"),
        "--dry-run",
        "--",
        "--permission-mode=auto",
    ]);
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

    assert!(output.status.success(), "{stderr}");
    assert!(stdout.contains("\"opus\""));
    assert!(stdout.contains("\"--permission-mode=auto\""));
    assert!(stderr.contains(
        "newer than the last version confirmed compatible with this nan-harness release"
    ));
    assert!(stderr.contains("2.1.251"));
    assert!(stderr.contains("forward-compatible safeguards"));
}
