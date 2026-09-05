use std::process::Command;

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
    let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
        .args(["diagnostics", "purge"])
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .output()
        .expect("private diagnostics command should start");
    let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

    assert!(!output.status.success());
    assert!(stderr.contains("purge requires an interactive terminal or --yes"));
}
