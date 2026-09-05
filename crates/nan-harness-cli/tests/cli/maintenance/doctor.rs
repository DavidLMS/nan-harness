#[cfg(unix)]
use crate::support::run_from_removed_cwd;
use crate::support::{run, run_with_embedded_compatibility};
use std::process::Command;

#[cfg(unix)]
#[test]
fn inaccessible_terminal_cwd_shows_restart_guidance_before_discovery() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let state = directory.path().join("state");

    for (index, arguments) in [["pi", "--dry-run"].as_slice(), ["doctor"].as_slice()]
        .into_iter()
        .enumerate()
    {
        let cwd = directory.path().join(format!("removed-cwd-{index}"));
        std::fs::create_dir(&cwd).expect("temporary cwd should be created");
        let output = run_from_removed_cwd(&cwd, &state, arguments);
        let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
        let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

        assert!(!output.status.success());
        assert!(stdout.is_empty(), "unexpected stdout: {stdout}");
        assert!(stderr.contains(
            "warning: The current terminal session cannot access the project directory. Please close this terminal, open a new terminal in the project directory, and try again."
        ));
        assert!(!stderr.contains("error [NH-CLI-005]"));
        assert!(!stderr.contains("NH-DISCOVERY-003"));
    }
}

#[cfg(unix)]
#[test]
fn missing_installable_harness_is_nonfatal_during_dry_run() {
    let path = tempfile::tempdir().expect("temporary PATH directory should exist");
    let home = tempfile::tempdir().expect("temporary home directory should exist");
    for harness in [
        "claude",
        "codex",
        "opencode",
        "hermes",
        "pi",
        "prime-agent",
        "cline",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
            .args([harness, "--dry-run"])
            .env("PATH", path.path())
            .env("HOME", home.path())
            .env_remove("USERPROFILE")
            .env_remove("NAN_UPDATE_MANIFEST_URL")
            .env_remove("NAN_HARNESS_GLITCHTIP_DSN")
            .output()
            .expect("nanh should start");
        let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

        assert!(output.status.success(), "{harness}: {stderr}");
        assert!(stderr.contains("dry-run does not install harnesses"));
        assert!(!stderr.contains("Official installer:"));
    }
}

#[cfg(unix)]
#[test]
fn doctor_discovers_a_harness_from_path() {
    use std::os::unix::fs::PermissionsExt;

    let path = tempfile::tempdir().expect("temporary PATH directory should exist");
    let executable = path.path().join("claude");
    std::fs::write(&executable, "#!/bin/sh\nprintf '%s\\n' 'claude 2.1.251'\n")
        .expect("fake executable should be written");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("fake executable should be executable");
    let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
        .args(["doctor", "claude"])
        .env("PATH", path.path())
        .env_remove("NAN_UPDATE_MANIFEST_URL")
        .output()
        .expect("doctor should start");
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("Harness: claude-code"));
    assert!(stdout.contains(executable.to_str().expect("path should be UTF-8")));
}

#[cfg(unix)]
#[test]
fn harness_doctor_json_is_stable_and_omits_executable_paths() {
    use std::os::unix::fs::PermissionsExt;

    let path = tempfile::tempdir().expect("temporary PATH directory should exist");
    let executable = path.path().join("claude");
    std::fs::write(&executable, "#!/bin/sh\nprintf '%s\\n' 'claude 2.1.233'\n")
        .expect("fake executable should be written");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("fake executable should be executable");
    let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
        .args(["doctor", "claude", "--json"])
        .env("PATH", path.path())
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env_remove("NAN_UPDATE_MANIFEST_URL")
        .output()
        .expect("doctor should start");
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("doctor output should be JSON");

    assert!(output.status.success());
    assert_eq!(report["schemaVersion"], 5);
    assert_eq!(report["harness"], "claude-code");
    assert_eq!(report["level"], "ok");
    assert_eq!(report["installed"], true);
    assert_eq!(report["version"], "2.1.233");
    assert_eq!(report["safeToShare"], true);
    assert!(report.get("executable").is_none());
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains(executable.to_string_lossy().as_ref())
    );
}

#[test]
fn harness_doctor_json_reports_discovery_failures_as_json() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let empty_path = directory.path().join("empty-path");
    std::fs::create_dir_all(&empty_path).expect("temporary PATH should be created");
    let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
        .args(["doctor", "claude", "--json"])
        .env("PATH", &empty_path)
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env_remove("NAN_UPDATE_MANIFEST_URL")
        .output()
        .expect("doctor should start");
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("doctor error should be JSON");

    assert!(!output.status.success());
    assert_eq!(report["schemaVersion"], 5);
    assert_eq!(report["harness"], "claude-code");
    assert_eq!(report["level"], "error");
    assert_eq!(report["installed"], false);
    assert_eq!(report["errorCode"], "NH-DISCOVERY-002");
    assert_eq!(report["safeToShare"], true);
    assert!(report.get("version").is_none());
    assert!(output.stderr.is_empty());
}

#[cfg(unix)]
#[test]
fn explicit_missing_executable_remains_a_discovery_error() {
    let output = run(&[
        "kimi",
        "--executable",
        "/definitely/missing/kimi",
        "--dry-run",
    ]);
    let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

    assert!(!output.status.success());
    assert!(stderr.contains("error [NH-DISCOVERY-002]"));
    assert!(stderr.contains("is not an executable file"));
    assert!(!stderr.contains("Official installer:"));
}

#[cfg(unix)]
#[test]
fn doctor_checks_a_real_executable_boundary() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = directory.path().join("claude");
    std::fs::write(&executable, "#!/bin/sh\nprintf '%s\\n' 'claude 2.1.251'\n")
        .expect("fake executable should be written");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("fake executable should be executable");
    let output = run_with_embedded_compatibility(&[
        "doctor",
        "claude-code",
        "--executable",
        executable.to_str().expect("path should be UTF-8"),
    ]);
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("Harness: claude-code"));
    assert!(stdout.contains("Minimum supported: 2.1.233"));
    assert!(stdout.contains("Last compatible: 2.1.251"));
    assert!(stdout.contains("Compatible at: 2026-08-29T00:00:00Z"));
    assert!(stdout.contains("Last live verified: 2.1.233"));
    assert!(stdout.contains("Live verified at: 2026-08-18T00:00:00Z"));
    assert!(stdout.contains("Compatibility: tested"));
}

#[cfg(unix)]
#[test]
fn harness_doctor_json_exposes_compatibility_evidence() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = directory.path().join("claude");
    std::fs::write(&executable, "#!/bin/sh\nprintf '%s\\n' 'claude 2.1.251'\n")
        .expect("fake executable should be written");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("fake executable should be executable");
    let output = run_with_embedded_compatibility(&[
        "doctor",
        "claude",
        "--json",
        "--executable",
        executable.to_str().expect("path should be UTF-8"),
    ]);
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("doctor output should be JSON");

    assert!(output.status.success());
    assert_eq!(report["schemaVersion"], 5);
    assert_eq!(report["lastCompatibleVersion"], "2.1.251");
    assert_eq!(report["compatibleAt"], "2026-08-29T00:00:00Z");
    assert_eq!(report["lastLiveVerifiedVersion"], "2.1.233");
    assert_eq!(report["liveVerifiedAt"], "2026-08-18T00:00:00Z");
    assert!(report.get("lastVerifiedVersion").is_none());
    assert!(report.get("executable").is_none());
}
