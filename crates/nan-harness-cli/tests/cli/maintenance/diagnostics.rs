use std::process::{Command, Stdio};

fn private_diagnostics(state: &std::path::Path, arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_nanh"))
        .arg("diagnostics")
        .args(arguments)
        .env("NAN_HARNESS_CONFIG_DIR", state)
        .env("NAN_NO_UPDATE_CHECK", "1")
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env_remove("NAN_UPDATE_MANIFEST_URL")
        .env_remove("NAN_HARNESS_GLITCHTIP_DSN")
        .stdin(Stdio::null())
        .output()
        .expect("private diagnostics command should start")
}

#[test]
fn private_diagnostics_missing_settings_report_off_without_publishing_settings() {
    let root = tempfile::tempdir().unwrap();
    let status = private_diagnostics(root.path(), &["status"]);
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("Local diagnostics: off"));
    assert!(!root.path().join("diagnostics/settings.json").exists());
}

#[test]
fn private_diagnostics_invalid_settings_refuse_on_and_status_and_off_reports_backup() {
    for original in [
        br#"{"schema_version":1,"enabled":"synthetic-private-value"}"#.as_slice(),
        br#"{"schema_version":2,"enabled":true,"future":"synthetic-private-value"}"#,
    ] {
        let root = tempfile::tempdir().unwrap();
        let diagnostics = root.path().join("diagnostics");
        std::fs::create_dir(&diagnostics).unwrap();
        let settings = diagnostics.join("settings.json");
        std::fs::write(&settings, original).unwrap();
        for action in ["status", "on"] {
            let output = private_diagnostics(root.path(), &[action]);
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stdout.is_empty());
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(error.contains("NH-COORD-005"));
            assert!(error.contains("diagnostics off"));
            assert!(!error.contains("synthetic-private-value"));
            assert_eq!(std::fs::read(&settings).unwrap(), original);
            assert!(!diagnostics.join("captures").exists());
        }
        let off = private_diagnostics(root.path(), &["off"]);
        assert!(off.status.success());
        let notice = String::from_utf8_lossy(&off.stderr);
        assert!(notice.contains("OFF"));
        assert!(notice.contains("settings-backups"));
        assert!(!notice.contains("synthetic-private-value"));
        let backup = std::fs::read_dir(diagnostics.join("settings-backups"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(std::fs::read(&backup).unwrap(), original);
        let status = private_diagnostics(root.path(), &["status"]);
        assert!(status.status.success());
        assert!(String::from_utf8_lossy(&status.stdout).contains("Local diagnostics: off"));
        let purge = private_diagnostics(root.path(), &["purge", "--yes"]);
        assert!(purge.status.success());
        assert_eq!(std::fs::read(&backup).unwrap(), original);
    }
}

#[test]
fn private_diagnostics_io_errors_map_to_state_failure_without_recovery() {
    let root = tempfile::tempdir().unwrap();
    let diagnostics = root.path().join("diagnostics");
    std::fs::create_dir_all(diagnostics.join("settings.json")).unwrap();
    for action in ["on", "off", "status"] {
        let output = private_diagnostics(root.path(), &[action]);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("NH-COORD-002"));
        assert!(diagnostics.join("settings.json").is_dir());
        assert!(!diagnostics.join("settings-backups").exists());
    }
}

#[test]
fn private_diagnostics_backup_failure_does_not_disable_or_overwrite_invalid_state() {
    let root = tempfile::tempdir().unwrap();
    let diagnostics = root.path().join("diagnostics");
    std::fs::create_dir(&diagnostics).unwrap();
    std::fs::write(
        diagnostics.join("settings.json"),
        b"invalid synthetic state",
    )
    .unwrap();
    std::fs::write(diagnostics.join("settings-backups"), b"keep").unwrap();
    let off = private_diagnostics(root.path(), &["off"]);
    assert_eq!(off.status.code(), Some(1));
    let error = String::from_utf8_lossy(&off.stderr);
    assert!(error.contains("NH-COORD-002"));
    assert!(!error.contains("OFF"));
    assert_eq!(
        std::fs::read(diagnostics.join("settings.json")).unwrap(),
        b"invalid synthetic state"
    );
}

#[test]
fn private_diagnostics_purge_reports_recovery_and_preserves_backup() {
    let root = tempfile::tempdir().unwrap();
    let diagnostics = root.path().join("diagnostics");
    std::fs::create_dir_all(diagnostics.join("captures/synthetic")).unwrap();
    std::fs::write(
        diagnostics.join("settings.json"),
        b"invalid synthetic state",
    )
    .unwrap();
    std::fs::write(
        diagnostics.join("captures/synthetic/request.jsonl"),
        b"synthetic",
    )
    .unwrap();
    let purge = private_diagnostics(root.path(), &["purge", "--yes"]);
    assert!(purge.status.success());
    assert!(String::from_utf8_lossy(&purge.stderr).contains("settings-backups"));
    assert_eq!(
        std::fs::read_dir(diagnostics.join("settings-backups"))
            .unwrap()
            .count(),
        1
    );
    assert_eq!(
        std::fs::read_dir(diagnostics.join("captures"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn private_diagnostics_lifecycle_is_explicit_and_preserves_coordinator_learning() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let state = directory.path().join("state");
    let run_diagnostics = |arguments: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_nanh"))
            .arg("diagnostics")
            .args(arguments)
            .env("NAN_HARNESS_CONFIG_DIR", &state)
            .env_remove("NAN_UPDATE_MANIFEST_URL")
            .env_remove("NAN_HARNESS_GLITCHTIP_DSN")
            .output()
            .expect("private diagnostics command should start")
    };

    let enabled = run_diagnostics(&["on"]);
    let warning = String::from_utf8(enabled.stderr).expect("warning should be UTF-8");
    assert!(enabled.status.success());
    assert!(warning.contains("local diagnostics are ON"));
    assert!(warning.contains("Prompts, model output, tool data, and embedded attachments"));
    assert!(warning.contains("not deleted automatically"));

    let status = run_diagnostics(&["status"]);
    let status_text = String::from_utf8(status.stdout).expect("status should be UTF-8");
    assert!(status.status.success());
    assert!(status_text.contains("Local diagnostics: on"));
    assert!(status_text.contains("Enabled at:"));

    let captures = state.join("diagnostics/captures/manual-fixture");
    std::fs::create_dir_all(&captures).expect("capture fixture directory should exist");
    std::fs::write(captures.join("request.jsonl"), "synthetic fixture")
        .expect("capture fixture should exist");
    let coordinator = state.join("coordinator/v1");
    std::fs::create_dir_all(&coordinator).expect("coordinator fixture directory should exist");
    std::fs::write(coordinator.join("capacity.json"), "synthetic learning")
        .expect("coordinator fixture should exist");

    let purge = run_diagnostics(&["purge", "--yes"]);
    let purge_text = String::from_utf8(purge.stderr).expect("purge output should be UTF-8");
    assert!(purge.status.success());
    assert!(purge_text.contains("Diagnostic logs were deleted"));
    assert!(state.join("diagnostics/captures").is_dir());
    assert!(
        std::fs::read_dir(state.join("diagnostics/captures"))
            .expect("capture directory should be readable")
            .next()
            .is_none()
    );
    assert!(coordinator.join("capacity.json").exists());

    let disabled = run_diagnostics(&["status"]);
    assert!(String::from_utf8_lossy(&disabled.stdout).contains("Local diagnostics: off"));
}

#[test]
fn private_diagnostics_purge_requires_confirmation_without_a_terminal() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let state = directory.path().join("state");
    let run_diagnostics = |arguments: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_nanh"))
            .arg("diagnostics")
            .args(arguments)
            .env("NAN_HARNESS_CONFIG_DIR", &state)
            .env("NAN_NO_UPDATE_CHECK", "1")
            .env("NAN_NO_COMPATIBILITY_CHECK", "1")
            .env_remove("NAN_UPDATE_MANIFEST_URL")
            .env_remove("NAN_HARNESS_GLITCHTIP_DSN")
            .stdin(Stdio::null())
            .output()
            .expect("private diagnostics command should start")
    };

    let enabled = run_diagnostics(&["on"]);
    let enabled_text = String::from_utf8(enabled.stderr).expect("warning should be UTF-8");
    assert!(enabled.status.success());
    assert!(enabled_text.contains("local diagnostics are ON"));

    let status = run_diagnostics(&["status"]);
    let status_text = String::from_utf8(status.stdout).expect("status should be UTF-8");
    assert!(status.status.success());
    let capture_id = status_text
        .lines()
        .find_map(|line| line.strip_prefix("Capture: "))
        .expect("enabled diagnostics should report a capture ID");

    let captures = state.join("diagnostics/captures").join(capture_id);
    let capture_path = captures.join("request.jsonl");
    std::fs::write(&capture_path, "synthetic request").expect("capture fixture should exist");
    let coordinator = state.join("coordinator/v1");
    std::fs::create_dir_all(&coordinator).expect("coordinator fixture directory should exist");
    let learning_path = coordinator.join("capacity.json");
    std::fs::write(
        &learning_path,
        r#"{"schema_version":2,"scopes":{"synthetic-scope":{"window":3,"updated_at_unix_seconds":0,"healthy_since_penalty":0,"penalty_level":0}}}"#,
    )
    .expect("coordinator learning fixture should exist");

    let settings_before =
        std::fs::read(state.join("diagnostics/settings.json")).expect("settings should exist");
    let capture_before = std::fs::read(&capture_path).expect("capture should exist");
    let learning_before = std::fs::read(&learning_path).expect("learning should exist");

    let refused = run_diagnostics(&["purge"]);
    let refused_text = String::from_utf8(refused.stderr).expect("error should be UTF-8");

    assert!(!refused.status.success());
    assert!(refused_text.contains("purge requires an interactive terminal or --yes"));
    assert_eq!(
        std::fs::read(state.join("diagnostics/settings.json")).expect("settings should remain"),
        settings_before
    );
    assert_eq!(
        std::fs::read(&capture_path).expect("capture should remain"),
        capture_before
    );
    assert_eq!(
        std::fs::read(&learning_path).expect("learning should remain"),
        learning_before
    );

    let status = run_diagnostics(&["status"]);
    let status_text = String::from_utf8(status.stdout).expect("status should be UTF-8");
    assert!(status.status.success());
    assert!(status_text.contains("Local diagnostics: on"));
    assert!(status_text.contains(&format!("Capture: {capture_id}")));
}
