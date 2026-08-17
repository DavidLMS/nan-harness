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
    assert!(stdout.contains("telemetry"));
}

#[test]
fn run_help_lists_every_available_harness() {
    let output = run(&["run", "--help"]);
    let stdout = String::from_utf8(output.stdout).expect("help output should be UTF-8");

    assert!(output.status.success());
    for harness in [
        "claude-code",
        "codex",
        "opencode",
        "hermes",
        "pi",
        "prime-agent",
        "deepseek-harness",
    ] {
        assert!(stdout.contains(harness), "missing {harness} from run help");
    }
}

#[test]
fn telemetry_exposes_only_on_and_off_and_persists_the_choice() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let help = Command::new(env!("CARGO_BIN_EXE_nan-harness"))
        .args(["telemetry", "--help"])
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .output()
        .expect("telemetry help should run");
    let help = String::from_utf8(help.stdout).expect("help should be UTF-8");
    assert!(help.contains("on"));
    assert!(help.contains("off"));
    assert!(!help.contains("  help"));

    let enabled = Command::new(env!("CARGO_BIN_EXE_nan-harness"))
        .args(["telemetry", "on"])
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .output()
        .expect("telemetry on should run");
    assert!(enabled.status.success());
    assert_eq!(
        String::from_utf8(enabled.stdout)
            .expect("output should be UTF-8")
            .trim(),
        "Telemetry is on."
    );
    let settings: serde_json::Value = serde_json::from_slice(
        &std::fs::read(directory.path().join("telemetry.json"))
            .expect("settings should be persisted"),
    )
    .expect("settings should be JSON");
    assert_eq!(settings["enabled"], true);

    let disabled = Command::new(env!("CARGO_BIN_EXE_nan-harness"))
        .args(["telemetry", "off"])
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .output()
        .expect("telemetry off should run");
    assert!(disabled.status.success());
    assert_eq!(
        String::from_utf8(disabled.stdout)
            .expect("output should be UTF-8")
            .trim(),
        "Telemetry is off."
    );
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
    assert!(!stderr.contains("Send an anonymous error report?"));
}

#[test]
fn telemetry_export_failure_preserves_the_original_cli_failure() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let settings = directory.path().join("telemetry.json");
    std::fs::write(&settings, "{\"enabled\":true}\n")
        .expect("telemetry settings should be written");
    let file = tempfile::NamedTempFile::new().expect("temporary file should be created");
    std::fs::write(file.path(), "{}").expect("invalid plan should be written");
    let output = Command::new(env!("CARGO_BIN_EXE_nan-harness"))
        .args([
            "validate-plan",
            file.path()
                .to_str()
                .expect("temporary path should be UTF-8"),
        ])
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .env(
            "NAN_HARNESS_GLITCHTIP_DSN",
            "http://public_key@127.0.0.1:9/42",
        )
        .output()
        .expect("nan-harness should start");
    let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
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
#[test]
fn direct_harness_dry_runs_build_safe_native_overlays() {
    let cases = [
        ("opencode", "1.18.4", "NAN_API_KEY", "nan/qwen3.6"),
        ("hermes", "0.20.2", "OPENAI_API_KEY", "CUSTOM_BASE_URL"),
        (
            "pi",
            "0.84.2",
            "NAN_API_KEY",
            "{artifact:pi-provider-extension}",
        ),
        (
            "prime-agent",
            "0.7.2",
            "NAN_API_KEY",
            "{artifact:pi-provider-extension}",
        ),
        (
            "deepseek-harness",
            "0.1.0-rc.7",
            "NAN_API_KEY",
            "{artifact:deepseek-harness-patch}",
        ),
    ];

    for (harness, version, credential_target, marker) in cases {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let executable = fake_harness(directory.path(), version);
        let output = Command::new(env!("CARGO_BIN_EXE_nan-harness"))
            .args([
                "run",
                harness,
                "--executable",
                executable.to_str().expect("path should be UTF-8"),
                "--dry-run",
            ])
            .env_remove("NAN_API_KEY")
            .output()
            .expect("nan-harness should start");
        let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
        let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

        assert!(output.status.success(), "{harness}: {stderr}");
        assert!(stdout.contains("\"kind\": \"direct-chat\""));
        assert!(stdout.contains("{runtime:provider_base_url}"));
        assert!(stdout.contains(credential_target));
        assert!(stdout.contains(marker));
        assert!(!stdout.contains("nan-secret-value"));
    }
}

#[cfg(unix)]
#[test]
fn codex_dry_run_builds_a_safe_responses_bridge_plan() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = fake_harness(directory.path(), "codex-cli 0.146.0");
    let output = Command::new(env!("CARGO_BIN_EXE_nan-harness"))
        .args([
            "run",
            "codex",
            "--executable",
            executable.to_str().expect("path should be UTF-8"),
            "--dry-run",
        ])
        .env_remove("NAN_API_KEY")
        .output()
        .expect("nan-harness should start");
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

    assert!(output.status.success(), "{stderr}");
    assert!(stdout.contains("\"kind\": \"responses-bridge\""));
    assert!(stdout.contains("{runtime:bridge_base_url}/v1"));
    assert!(stdout.contains("NAN_HARNESS_SESSION_TOKEN"));
    assert!(stdout.contains("supports_standalone_web_search=true"));
    assert!(!stdout.contains("nan-secret-value"));
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

#[cfg(unix)]
fn fake_harness(directory: &std::path::Path, version: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let executable = directory.join("fake-harness");
    std::fs::write(
        &executable,
        format!("#!/bin/sh\nprintf '%s\\n' '{version}'\n"),
    )
    .expect("fake executable should be written");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("fake executable should be executable");
    executable
}
