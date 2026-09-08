use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

fn fixture(directory: &Path, name: &str, mode: &str) -> PathBuf {
    static BINARY: OnceLock<tempfile::TempDir> = OnceLock::new();
    let compiled = BINARY.get_or_init(|| {
        let directory = tempfile::tempdir().unwrap();
        assert!(
            Command::new("rustc")
                .args([
                    "--edition=2024",
                    "../nan-harness-runtime/tests/fixtures/probe.rs",
                    "-o"
                ])
                .arg(
                    directory
                        .path()
                        .join(format!("fixture{}", std::env::consts::EXE_SUFFIX))
                )
                .status()
                .unwrap()
                .success()
        );
        directory
    });
    let executable = directory.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    fs::copy(
        compiled
            .path()
            .join(format!("fixture{}", std::env::consts::EXE_SUFFIX)),
        &executable,
    )
    .unwrap();
    fs::write(executable.with_extension("mode"), mode).unwrap();
    executable
}

fn run(directory: &Path, arguments: &[&str]) -> Output {
    let stdout = tempfile::NamedTempFile::new().unwrap();
    let stderr = tempfile::NamedTempFile::new().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_nanh"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("NAN_") {
            command.env_remove(key);
        }
    }
    let mut child = command
        .args(arguments)
        .env("HOME", directory)
        .env("USERPROFILE", directory)
        .env("XDG_CONFIG_HOME", directory)
        .env("APPDATA", directory)
        .env("LOCALAPPDATA", directory)
        .env("TMPDIR", directory)
        .env("TMP", directory)
        .env("TEMP", directory)
        .env("PATH", directory)
        .env("NAN_HARNESS_CONFIG_DIR", directory.join("state"))
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env("NAN_NO_UPDATE_CHECK", "1")
        .stdin(Stdio::null())
        .stdout(stdout.reopen().unwrap())
        .stderr(stderr.reopen().unwrap())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(40);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("CLI exceeded outer deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    Output {
        status,
        stdout: fs::read(stdout.path()).unwrap(),
        stderr: fs::read(stderr.path()).unwrap(),
    }
}

#[test]
fn probe_system_doctor_keeps_healthy_entries_when_one_version_times_out() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path(), "codex", "success");
    fixture(directory.path(), "opencode", "hang");
    let output = run(directory.path(), &["doctor", "--offline", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let harnesses = report["harnesses"].as_array().unwrap();
    let healthy = harnesses
        .iter()
        .find(|entry| entry["id"] == "codex")
        .unwrap();
    let timed_out = harnesses
        .iter()
        .find(|entry| entry["id"] == "opencode")
        .unwrap();
    assert_eq!(healthy["version"], "0.153.4");
    assert_eq!(healthy["level"], "ok");
    assert_eq!(timed_out["errorCode"], "NH-DISCOVERY-006");
    let ids: Vec<_> = harnesses
        .iter()
        .map(|entry| {
            serde_json::from_value::<nan_harness_core::HarnessKind>(entry["id"].clone()).unwrap()
        })
        .collect();
    assert_eq!(ids, nan_harness_core::HarnessKind::ALL);
    assert!(!String::from_utf8_lossy(&output.stdout).contains(directory.path().to_str().unwrap()));
}

#[test]
fn probe_targeted_launch_reports_output_limit_with_recovery_guidance() {
    let directory = tempfile::tempdir().unwrap();
    let executable = fixture(directory.path(), "codex", "stdout-flood");
    let output = run(
        directory.path(),
        &[
            "codex",
            "--dry-run",
            "--executable",
            executable.to_str().unwrap(),
        ],
    );
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("NH-DISCOVERY-007"));
    assert!(error.contains("--executable"));
    assert!(!error.contains("xxxxxxxx"));
    assert_eq!(
        fs::read_to_string(executable.with_extension("calls"))
            .unwrap()
            .lines()
            .count(),
        1,
        "diagnostics must not repeat an overflowing probe"
    );
}

#[test]
fn probe_targeted_launch_reports_timeout_with_recovery_guidance() {
    let directory = tempfile::tempdir().unwrap();
    let executable = fixture(directory.path(), "codex", "hang");
    let output = run(
        directory.path(),
        &[
            "codex",
            "--dry-run",
            "--executable",
            executable.to_str().unwrap(),
        ],
    );
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("NH-DISCOVERY-006"));
    assert!(error.contains("--executable"));
    assert_eq!(
        fs::read_to_string(executable.with_extension("calls"))
            .unwrap()
            .lines()
            .count(),
        1,
        "diagnostics must not repeat a timed-out probe"
    );
}

#[test]
fn probe_optional_help_overflow_preserves_compatibility_fallback() {
    let directory = tempfile::tempdir().unwrap();
    let executable = fixture(directory.path(), "codex", "help-flood");
    let output = run(
        directory.path(),
        &[
            "doctor",
            "codex",
            "--offline",
            "--executable",
            executable.to_str().unwrap(),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("isolated compatibility mode"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("xxxxxxxx"));
}
