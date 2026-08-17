use std::process::{Command, Output};

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nan-harness"))
        .args(arguments)
        .output()
        .expect("nan-harness should start")
}

#[test]
fn help_is_english_and_lists_engineering_commands() {
    let output = run(&["--help"]);
    let stdout = String::from_utf8(output.stdout).expect("help output should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("Run AI coding harnesses through NaN"));
    assert!(stdout.contains("Usage: nan-harness <COMMAND>"));
    assert!(stdout.contains("run"));
    assert!(stdout.contains("doctor"));
    assert!(stdout.contains("validate-plan"));
}

#[test]
fn version_matches_the_workspace() {
    let output = run(&["--version"]);
    let stdout = String::from_utf8(output.stdout).expect("version output should be UTF-8");

    assert!(output.status.success());
    assert_eq!(stdout.trim(), "nan-harness 0.1.0");
}

#[test]
fn validate_plan_prints_safe_normalized_json() {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../nan-harness-core/tests/fixtures/launch-plan.direct.json"
    );
    let output = run(&["validate-plan", fixture]);
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("\"launchId\": \"launch_01exampledirect\""));
    assert!(stdout.contains("\"nan_api_key\""));
    assert!(!stdout.contains("nan-secret-value"));
}

#[test]
fn invalid_plan_reports_a_stable_english_error() {
    let file = tempfile::NamedTempFile::new().expect("temporary file should be created");
    std::fs::write(file.path(), "{}").expect("invalid plan should be written");
    let output = run(&[
        "validate-plan",
        file.path()
            .to_str()
            .expect("temporary path should be UTF-8"),
    ]);
    let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

    assert!(!output.status.success());
    assert!(stderr.contains("error [NH-CLI-002]"));
    assert!(stderr.contains("launch plan"));
}

#[cfg(unix)]
#[test]
fn doctor_checks_a_real_executable_boundary() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = directory.path().join("claude");
    std::fs::write(&executable, "#!/bin/sh\nprintf '%s\\n' 'claude 2.1.233'\n")
        .expect("fake executable should be written");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("fake executable should be executable");
    let output = run(&[
        "doctor",
        "claude-code",
        "--executable",
        executable.to_str().expect("path should be UTF-8"),
    ]);
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("Harness: claude-code"));
    assert!(stdout.contains("Minimum supported: 2.1.233"));
    assert!(stdout.contains("Last verified: 2.1.233"));
    assert!(stdout.contains("Compatibility: tested"));
}

#[cfg(unix)]
#[test]
fn claude_code_dry_run_builds_a_safe_bridge_plan_without_an_api_key() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = fake_claude(directory.path());
    let output = Command::new(env!("CARGO_BIN_EXE_nan-harness"))
        .args([
            "run",
            "claude-code",
            "--executable",
            executable.to_str().expect("path should be UTF-8"),
            "--dry-run",
            "--",
            "-p",
            "hello",
        ])
        .env_remove("NAN_API_KEY")
        .output()
        .expect("nan-harness should start");
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("\"kind\": \"anthropic-bridge\""));
    assert!(stdout.contains("{runtime:bridge_base_url}"));
    assert!(stdout.contains("{artifact:claude-settings}"));
    assert!(!stdout.contains("nan-test-secret"));
}

#[cfg(unix)]
#[test]
fn claude_code_dry_run_accepts_a_supported_non_default_nan_model() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = fake_claude(directory.path());
    let output = Command::new(env!("CARGO_BIN_EXE_nan-harness"))
        .args([
            "run",
            "claude-code",
            "--executable",
            executable.to_str().expect("path should be UTF-8"),
            "--model",
            "mimo-v2.5",
            "--dry-run",
        ])
        .env_remove("NAN_API_KEY")
        .output()
        .expect("nan-harness should start");
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
        "run",
        "claude-code",
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
        "run",
        "claude-code",
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
    assert!(stderr.contains("conflicts with NaN Harness routing"));
}

#[cfg(unix)]
#[test]
fn claude_code_dry_run_accepts_native_auto_mode_for_qwen() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = fake_claude(directory.path());
    let output = run(&[
        "run",
        "claude-code",
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
    let executable = fake_claude_with_version(directory.path(), "2.1.234 (Claude Code)");
    let output = run(&[
        "run",
        "claude-code",
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
    assert!(stderr.contains("newer than the last version verified"));
    assert!(stderr.contains("2.1.233"));
    assert!(stderr.contains("forward-compatible safeguards"));
}

#[cfg(unix)]
fn fake_claude(directory: &std::path::Path) -> std::path::PathBuf {
    fake_claude_with_version(directory, "2.1.233 (Claude Code)")
}

#[cfg(unix)]
fn fake_claude_with_version(directory: &std::path::Path, version: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let executable = directory.join("claude");
    std::fs::write(
        &executable,
        format!("#!/bin/sh\nprintf '%s\\n' '{version}'\n"),
    )
    .expect("fake executable should be written");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("fake executable should be executable");
    executable
}
