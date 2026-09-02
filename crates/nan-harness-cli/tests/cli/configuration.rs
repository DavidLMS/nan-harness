use crate::support::{
    capture_one_http_request, capture_one_http_request_with_response, config_command,
    write_private_credential_fixture,
};
use std::process::Command;

#[test]
fn removing_an_absent_native_configuration_is_idempotent() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    for harness in [
        "opencode",
        "hermes",
        "pi",
        "prime-agent",
        "dsh",
        "openclaw",
        "cline",
        "qwen",
        "kimi",
        "aider",
        "goose",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_nan"))
            .args(["config", harness, "--remove"])
            .env("NAN_HARNESS_CONFIG_DIR", directory.path().join("state"))
            .env("HOME", directory.path().join("home"))
            .env("NAN_NO_COMPATIBILITY_CHECK", "1")
            .output()
            .expect("nan should start");
        let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");

        assert!(output.status.success());
        assert!(stdout.contains("No NaN configuration managed by nan-harness was found"));
    }
}

#[test]
fn telemetry_exposes_only_on_and_off_and_persists_the_choice() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let help = Command::new(env!("CARGO_BIN_EXE_nan"))
        .args(["telemetry", "--help"])
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .output()
        .expect("telemetry help should run");
    let help = String::from_utf8(help.stdout).expect("help should be UTF-8");
    assert!(help.contains("on"));
    assert!(help.contains("off"));
    assert!(!help.contains("  help"));

    let enabled = Command::new(env!("CARGO_BIN_EXE_nan"))
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
    assert!(
        settings["installationId"]
            .as_str()
            .is_some_and(|value| value.starts_with("installation_"))
    );

    let disabled = Command::new(env!("CARGO_BIN_EXE_nan"))
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
    let settings: serde_json::Value = serde_json::from_slice(
        &std::fs::read(directory.path().join("telemetry.json"))
            .expect("disabled settings should be persisted"),
    )
    .expect("disabled settings should be JSON");
    assert_eq!(settings["enabled"], false);
    assert!(
        settings["installationId"]
            .as_str()
            .is_some_and(|value| value.starts_with("installation_"))
    );
}

#[cfg(unix)]
#[test]
fn missing_api_key_is_reported_before_harness_installation_non_interactively() {
    let path = tempfile::tempdir().expect("temporary PATH directory should exist");
    let state = path.path().join("state");
    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
        .args(["kimi"])
        .env("PATH", path.path())
        .env("HOME", path.path())
        .env("NAN_HARNESS_CONFIG_DIR", &state)
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env_remove("USERPROFILE")
        .env_remove("NAN_API_KEY")
        .env_remove("NAN_BASE_URL")
        .env_remove("NAN_UPDATE_MANIFEST_URL")
        .env_remove("NAN_HARNESS_GLITCHTIP_DSN")
        .output()
        .expect("nan should start");
    let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

    assert!(!output.status.success());
    assert!(!stderr.contains("NH-CREDENTIAL-001"));
    assert!(stderr.contains("no NaN API key is configured"));
    assert!(stderr.contains("run `nan auth login`"));
    assert!(!stderr.contains("kimi-code was not found"));
    assert!(!stderr.contains("installation"));
}

#[test]
fn auth_status_and_logout_manage_a_saved_private_credential() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let state = directory.path().join("state");
    std::fs::create_dir_all(&state).expect("state directory should be created");
    std::fs::write(state.join("nan-api-key"), "nan-private-test-key")
        .expect("credential should be written");
    std::fs::write(
        state.join("credential.json"),
        r#"{"schemaVersion":1,"backend":"private-file"}"#,
    )
    .expect("credential receipt should be written");
    let (endpoint, request) =
        capture_one_http_request_with_response(r#"{"data":[{"id":"qwen3.6"}]}"#);

    let status = Command::new(env!("CARGO_BIN_EXE_nan"))
        .args(["auth", "status"])
        .env("NAN_HARNESS_CONFIG_DIR", &state)
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env("NAN_BASE_URL", format!("{endpoint}/v1"))
        .env_remove("NAN_API_KEY")
        .output()
        .expect("auth status should start");
    let stdout = String::from_utf8(status.stdout).expect("status output should be UTF-8");
    assert!(status.status.success());
    assert!(stdout.contains("Effective launch key: not set in NAN_API_KEY."));
    assert!(stdout.contains("Saved configuration key:"));
    assert!(stdout.contains("the private nan-harness credential file"));
    assert!(stdout.contains("Managed harness configurations: 0 total, 0 needing attention."));
    assert!(!stdout.contains("nan-private-test-key"));
    request
        .join()
        .expect("credential verification should finish");

    let logout = Command::new(env!("CARGO_BIN_EXE_nan"))
        .args(["auth", "logout", "--yes"])
        .env("NAN_HARNESS_CONFIG_DIR", &state)
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env_remove("NAN_API_KEY")
        .output()
        .expect("auth logout should start");
    let stdout = String::from_utf8(logout.stdout).expect("logout output should be UTF-8");
    assert!(logout.status.success());
    assert_eq!(stdout.trim(), "Saved NaN API key removed.");
    assert!(!state.join("nan-api-key").exists());
    assert!(!state.join("credential.json").exists());
}

#[test]
fn config_requires_a_saved_key_and_never_copies_the_environment_key() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
        .args(["config", "pi", "--yes"])
        .env("HOME", directory.path().join("home"))
        .env("NAN_HARNESS_CONFIG_DIR", directory.path().join("state"))
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env("NAN_API_KEY", "environment-only-secret")
        .output()
        .expect("nan config should start");
    let stderr = String::from_utf8(output.stderr).expect("error output should be UTF-8");

    assert!(!output.status.success());
    assert!(stderr.contains("no API key is saved by nan-harness"));
    assert!(!stderr.contains("NH-CREDENTIAL"));
    assert!(!directory.path().join("home/.pi/agent/auth.json").exists());
}

#[test]
fn config_requires_consent_before_credentials_or_provider_access() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
        .args(["config", "pi"])
        .env("HOME", directory.path().join("home"))
        .env("NAN_HARNESS_CONFIG_DIR", directory.path().join("state"))
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env_remove("NAN_API_KEY")
        .output()
        .expect("nan config should start");
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("error output should be UTF-8");

    assert!(!output.status.success());
    assert!(
        stderr.contains("this configuration change requires an interactive confirmation or --yes")
    );
    assert!(stdout.is_empty());
    assert!(!stderr.contains("Enter your NaN API key"));
    assert!(!stderr.contains("no API key is saved"));
    assert!(!directory.path().join("home/.pi/agent/auth.json").exists());
}

#[test]
fn config_tracks_key_rotation_until_the_harness_is_refreshed() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let state = directory.path().join("state");
    std::fs::create_dir_all(&state).expect("state directory should be created");
    write_private_credential_fixture(&state, "first-private-key");
    let response = r#"{"data":[{"id":"qwen3.6"},{"id":"gemma4"}]}"#;
    let (endpoint, request) = capture_one_http_request_with_response(response);
    let base_url = format!("{endpoint}/v1");

    let configured = config_command(&home, &state, &base_url)
        .args(["config", "kimi", "--yes"])
        .output()
        .expect("native configuration should start");
    assert!(
        configured.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&configured.stdout),
        String::from_utf8_lossy(&configured.stderr)
    );
    assert!(
        String::from_utf8_lossy(&configured.stdout)
            .contains("Web search: automatic NaN fallback active.")
    );
    request.join().expect("model request should finish");
    let kimi_config = home.join(".kimi-code/config.toml");
    assert!(
        std::fs::read_to_string(&kimi_config)
            .expect("Kimi configuration should exist")
            .contains("first-private-key")
    );
    let receipts = std::fs::read_to_string(state.join("configurations.json"))
        .expect("configuration receipts should exist");
    assert!(!receipts.contains("first-private-key"));

    let unchanged = config_command(&home, &state, "http://127.0.0.1:1/v1")
        .args(["config", "kimi"])
        .output()
        .expect("existing configuration inspection should start");
    assert!(unchanged.status.success());
    let unchanged_stdout = String::from_utf8_lossy(&unchanged.stdout);
    assert!(unchanged_stdout.contains("configured, unchanged"));
    assert!(unchanged_stdout.contains("Web search: automatic NaN fallback active."));

    write_private_credential_fixture(&state, "second-private-key");
    let stale_status = config_command(&home, &state, &base_url)
        .args(["config", "kimi", "--status"])
        .output()
        .expect("configuration status should start");
    let stale_stdout = String::from_utf8(stale_status.stdout).expect("status should be UTF-8");
    assert!(stale_status.status.success());
    assert!(stale_stdout.contains("copied key needs `nan config kimi-code --refresh`"));
    assert!(stale_stdout.contains("Web search: automatic NaN fallback active."));

    let (endpoint, request) = capture_one_http_request_with_response(response);
    let refreshed_base_url = format!("{endpoint}/v1");
    let refreshed = config_command(&home, &state, &refreshed_base_url)
        .args(["config", "kimi", "--refresh"])
        .output()
        .expect("configuration refresh should start");
    assert!(refreshed.status.success());
    request.join().expect("refresh request should finish");
    assert!(
        std::fs::read_to_string(&kimi_config)
            .expect("Kimi configuration should remain")
            .contains("second-private-key")
    );
    let receipts = std::fs::read_to_string(state.join("configurations.json"))
        .expect("configuration receipts should remain");
    assert!(!receipts.contains("second-private-key"));

    let removed = config_command(&home, &state, &refreshed_base_url)
        .args(["config", "kimi", "--remove"])
        .output()
        .expect("configuration removal should start");
    assert!(removed.status.success());
    assert!(!kimi_config.exists());
}

#[test]
fn config_explains_launch_only_harnesses_without_requesting_a_key() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
        .args(["config", "claude"])
        .env("HOME", directory.path().join("home"))
        .env("NAN_HARNESS_CONFIG_DIR", directory.path().join("state"))
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env_remove("NAN_API_KEY")
        .output()
        .expect("nan config should start");
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("uses launch-scoped routing"));
    assert!(stdout.contains("Launch it with `nan claude`."));
}

#[test]
fn config_bridge_only_mode_precedes_status_for_launch_only_harnesses() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
        .args(["config", "claude", "--status"])
        .env("HOME", directory.path().join("home"))
        .env("NAN_HARNESS_CONFIG_DIR", directory.path().join("state"))
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env_remove("NAN_API_KEY")
        .output()
        .expect("nan config should start");
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("uses launch-scoped routing"));
    assert!(!stdout.contains("claude-code: not configured"));
}

#[test]
fn pen_configuration_status_and_absent_removal_are_offline_and_home_relative() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let home = directory.path().join("a-different-user");
    let state = directory.path().join("state");
    for arguments in [
        ["config", "pen", "--status"],
        ["config", "pen-desktop", "--remove"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_nan"))
            .args(arguments)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("NAN_HARNESS_CONFIG_DIR", &state)
            .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
            .env_remove("NAN_API_KEY")
            .output()
            .expect("Pen configuration command should start");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stderr}");
        assert!(stdout.contains("Pen Desktop: not configured by nan-harness"));
        assert!(!stdout.contains("david"));
        assert!(!stderr.contains("Enter your NaN API key"));
    }
    assert!(!home.join(".pencil").exists());
}

#[test]
fn pen_native_configuration_discovers_all_text_models_and_removes_cleanly() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let home = directory.path().join("portable-home");
    let state = directory.path().join("state");
    std::fs::create_dir_all(&state).expect("state directory should be created");
    write_private_credential_fixture(&state, "pen-private-key");
    let response = r#"{"data":[{"id":"qwen3.6"},{"id":"minimax-h3"},{"id":"glm5.3-flash"}]}"#;
    let (endpoint, request) = capture_one_http_request_with_response(response);
    let base_url = format!("{endpoint}/v1");

    let configured = config_command(&home, &state, &base_url)
        .args(["config", "pen", "--yes"])
        .output()
        .expect("Pen native configuration should start");
    assert!(
        configured.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&configured.stdout),
        String::from_utf8_lossy(&configured.stderr)
    );
    request.join().expect("model request should finish");
    let models_path = home.join(".pencil/models.json");
    let auth_path = home.join(".pencil/agent-auth");
    let models: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&models_path).expect("Pen models should exist"))
            .expect("Pen models should be JSON");
    let ids = models["providers"]["nan"]["models"]
        .as_array()
        .expect("models array")
        .iter()
        .filter_map(|model| model["id"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["qwen3.6", "glm5.3-flash"]);
    let auth = std::fs::read_to_string(&auth_path).expect("Pen auth should exist");
    assert!(auth.contains("pen-private-key"));
    let receipt = std::fs::read_to_string(state.join("pen-desktop/configuration.json"))
        .expect("Pen receipt should exist");
    assert!(!receipt.contains("pen-private-key"));

    let status = config_command(&home, &state, &base_url)
        .args(["config", "pen", "--status"])
        .output()
        .expect("Pen status should start");
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("current saved key (2 models)"));

    let removed = config_command(&home, &state, &base_url)
        .args(["config", "pen", "--remove"])
        .output()
        .expect("Pen removal should start");
    assert!(removed.status.success());
    assert!(!models_path.exists());
    assert!(!auth_path.exists());
}

#[test]
fn telemetry_export_failure_preserves_the_original_cli_failure() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let settings = directory.path().join("telemetry.json");
    std::fs::write(&settings, "{\"enabled\":true}\n")
        .expect("telemetry settings should be written");
    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
        .args([
            "doctor",
            "claude",
            "--executable",
            "/definitely/missing/claude",
        ])
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env(
            "NAN_HARNESS_GLITCHTIP_DSN",
            "http://public_key@127.0.0.1:9/42",
        )
        .output()
        .expect("nan should start");
    let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr.contains("error [NH-DISCOVERY-002]"));
    assert!(stderr.contains("not an executable file"));
}

#[test]
fn enabled_telemetry_emits_one_allowlisted_umami_event_from_the_binary() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let enabled = Command::new(env!("CARGO_BIN_EXE_nan"))
        .args(["telemetry", "on"])
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .output()
        .expect("telemetry on should run");
    assert!(enabled.status.success());
    let (endpoint, request) = capture_one_http_request();

    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
        .args([
            "doctor",
            "claude",
            "--executable",
            "/definitely/missing/claude",
        ])
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env("NAN_HARNESS_UMAMI_URL", endpoint)
        .env(
            "NAN_HARNESS_UMAMI_WEBSITE_ID",
            "59cf95d9-bb3d-410d-95c5-5ac94a24b74e",
        )
        .env("NAN_HARNESS_GLITCHTIP_DSN", "")
        .output()
        .expect("nan should start");

    assert_eq!(output.status.code(), Some(1));
    let request = request.join().expect("capture thread should finish");
    let (_, body) = request
        .split_once("\r\n\r\n")
        .expect("HTTP request should contain a body");
    let body: serde_json::Value = serde_json::from_str(body).expect("body should be JSON");
    assert_eq!(body["type"], "event");
    assert_eq!(body["payload"]["name"], "nan-operation-doctor");
    assert_eq!(body["payload"]["tag"], "harness:claude-code");
    assert_eq!(body["payload"]["data"]["operation"], "doctor");
    assert_eq!(body["payload"]["data"]["harness"], "claude-code");
    assert!(body["payload"]["data"].get("model").is_none());
}
