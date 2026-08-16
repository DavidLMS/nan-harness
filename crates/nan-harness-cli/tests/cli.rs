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
    assert!(stdout.contains("Compatibility: tested"));
}
