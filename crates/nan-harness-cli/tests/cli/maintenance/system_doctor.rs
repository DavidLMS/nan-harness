#[cfg(unix)]
use crate::support::{capture_one_http_request_with_response, run};
use std::process::Command;

#[test]
fn whole_system_doctor_json_exposes_compatibility_evidence() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let home = directory.path().join("home");
    let path = directory.path().join("bin");
    let state = directory.path().join("state");
    std::fs::create_dir_all(&home).expect("home should be created");
    std::fs::create_dir_all(&path).expect("PATH directory should be created");
    let executable = path.join("claude");
    std::fs::write(&executable, "#!/bin/sh\nprintf '%s\\n' 'claude 2.1.251'\n")
        .expect("fake executable should be written");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("fake executable should be executable");
    let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
        .args(["doctor", "--json"])
        .env("HOME", &home)
        .env("PATH", &path)
        .env("NAN_HARNESS_CONFIG_DIR", &state)
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env_remove("NAN_API_KEY")
        .env_remove("NAN_BASE_URL")
        .output()
        .expect("doctor should start");
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("doctor output should be JSON");
    let harness = report["harnesses"]
        .as_array()
        .expect("harnesses should be an array")
        .iter()
        .find(|harness| harness["id"] == "claude-code")
        .expect("Claude Code should be reported");

    assert!(output.status.success());
    assert_eq!(report["schemaVersion"], 5);
    assert_eq!(harness["lastCompatibleVersion"], "2.1.251");
    assert_eq!(harness["compatibleAt"], "2026-08-29T00:00:00Z");
    assert_eq!(harness["lastLiveVerifiedVersion"], "2.1.233");
    assert_eq!(harness["liveVerifiedAt"], "2026-08-18T00:00:00Z");
}

#[test]
fn whole_system_doctor_is_safe_and_nonfatal_without_optional_tools() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let home = directory.path().join("private-home");
    let empty_path = directory.path().join("empty-path");
    std::fs::create_dir_all(&home).expect("temporary home should be created");
    std::fs::create_dir_all(&empty_path).expect("temporary PATH should be created");
    let private_compatibility_url = "private-compatibility-token";

    let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
        .arg("doctor")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("PATH", &empty_path)
        .env("NAN_HARNESS_CONFIG_DIR", directory.path().join("state"))
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env("NAN_COMPATIBILITY_MANIFEST_URL", private_compatibility_url)
        .env_remove("NAN_NO_COMPATIBILITY_CHECK")
        .env_remove("NAN_API_KEY")
        .env_remove("NAN_BASE_URL")
        .output()
        .expect("system doctor should start");
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("error output should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("nan-harness\n[OK] Version:"));
    assert!(stdout.contains("[OK] Platform:"));
    assert!(stdout.contains("[INFO] API key: not configured"));
    assert!(stdout.contains("[SKIP] NaN API and model discovery: API key required"));
    assert!(stdout.contains("Harnesses"));
    for harness in [
        "claude-code",
        "codex",
        "opencode",
        "hermes",
        "pi",
        "prime-agent",
        "deepseek-harness",
        "openclaw",
        "cline",
        "qwen-code",
        "kimi-code",
        "aider",
        "goose",
        "fx",
    ] {
        assert!(
            stdout.contains(&format!("[INFO] {harness}: not installed")),
            "missing safe status for {harness}"
        );
    }
    assert!(stdout.contains("Managed harness configurations\n[INFO] None configured"));
    assert!(stdout.contains("Telemetry\n[INFO] Telemetry: off"));
    assert!(stdout.contains("Safe to share:"));
    assert!(!stdout.contains(home.to_string_lossy().as_ref()));
    assert!(!stdout.contains("NAN_API_KEY"));
    assert!(!stderr.contains(home.to_string_lossy().as_ref()));
    assert!(!stderr.contains(private_compatibility_url));
}

#[test]
fn whole_system_doctor_json_is_machine_readable_and_safe_to_share() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let home = directory.path().join("private-home");
    let empty_path = directory.path().join("empty-path");
    std::fs::create_dir_all(&home).expect("temporary home should be created");
    std::fs::create_dir_all(&empty_path).expect("temporary PATH should be created");

    let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
        .args(["doctor", "--json"])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("PATH", &empty_path)
        .env("NAN_HARNESS_CONFIG_DIR", directory.path().join("state"))
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env_remove("NAN_API_KEY")
        .env_remove("NAN_BASE_URL")
        .output()
        .expect("system doctor should start");
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("doctor output should be JSON");

    assert!(output.status.success());
    assert_eq!(report["schemaVersion"], 5);
    assert_eq!(report["nanHarnessVersion"], env!("CARGO_PKG_VERSION"));
    assert!(report.get("nanVersion").is_none());
    assert_eq!(report["provider"]["credential"], "not-configured");
    assert_eq!(report["provider"]["codingModels"], serde_json::json!([]));
    assert_eq!(report["harnesses"].as_array().map(Vec::len), Some(15));
    assert_eq!(
        report["experimentalHarnesses"].as_array().map(Vec::len),
        Some(5)
    );
    assert_eq!(report["safeToShare"], true);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains(home.to_string_lossy().as_ref()));
    assert!(!stdout.contains("NAN_API_KEY"));
}

#[test]
fn desktop_doctor_reports_local_experimental_evidence_without_discovery() {
    for harness in [
        "chatgpt-desktop",
        "codex-desktop",
        "claude-desktop",
        "hermes-desktop",
        "pen",
        "pen-desktop",
        "zed",
        "zed-desktop",
    ] {
        let output = run(&["doctor", harness, "--json"]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{harness}: {stderr}");
        let report: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("Desktop doctor should print JSON");
        assert_eq!(report["schemaVersion"], 5);
        assert_eq!(report["experimental"], true);
        assert_eq!(report["safeToShare"], true);
        assert!(matches!(
            report["evidence"].as_str(),
            Some("live-verified" | "contract-only" | "unavailable")
        ));
        assert!(report.get("executable").is_none());
        assert!(report.get("version").is_none());
    }
}

#[test]
fn whole_system_doctor_checks_nan_without_disclosing_connection_details() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let home = directory.path().join("private-home");
    let empty_path = directory.path().join("empty-path");
    std::fs::create_dir_all(&home).expect("temporary home should be created");
    std::fs::create_dir_all(&empty_path).expect("temporary PATH should be created");
    let response = r#"{"data":[{"id":"qwen3.6"},{"id":"gemma4"}]}"#;
    let (endpoint, request) = capture_one_http_request_with_response(response);
    let api_key = "nan_private_test_key";
    let base_url = format!("{endpoint}/v1");
    let state = directory.path().join("state");
    std::fs::create_dir_all(&state).expect("state directory should be created");
    std::fs::write(state.join("nan-api-key"), api_key).expect("credential should be written");
    std::fs::write(
        state.join("credential.json"),
        r#"{"schemaVersion":1,"backend":"private-file"}"#,
    )
    .expect("credential receipt should be written");

    let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
        .arg("doctor")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("PATH", &empty_path)
        .env("NAN_HARNESS_CONFIG_DIR", &state)
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env_remove("NAN_API_KEY")
        .env("NAN_BASE_URL", &base_url)
        .output()
        .expect("system doctor should start");
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
    let request = request.join().expect("model request should finish");

    assert!(output.status.success());
    assert!(stdout.contains("[OK] API key: configured"));
    assert!(stdout.contains("[OK] NaN API: reachable"));
    assert!(stdout.contains("[OK] Coding models: 2 available"));
    assert!(stdout.contains("[INFO] Model catalog: gemma4 · qwen3.6"));
    assert!(!stdout.contains("conservative default profile"));
    assert!(!stdout.contains(api_key));
    assert!(!stdout.contains(&base_url));
    assert!(request.starts_with("GET /v1/models HTTP/1.1"));
    assert!(request.contains(&format!("authorization: Bearer {api_key}")));
}

#[test]
fn whole_system_doctor_json_reports_sorted_model_capabilities_once() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let home = directory.path().join("private-home");
    let empty_path = directory.path().join("empty-path");
    std::fs::create_dir_all(&home).expect("temporary home should be created");
    std::fs::create_dir_all(&empty_path).expect("temporary PATH should be created");
    let response = r#"{"data":[{"id":"qwen3.6"},{"id":"future-model"},{"id":"gemma4"}]}"#;
    let (endpoint, request) = capture_one_http_request_with_response(response);
    let api_key = "nan_private_test_key";
    let base_url = format!("{endpoint}/v1");
    let state = directory.path().join("state");
    std::fs::create_dir_all(&state).expect("state directory should be created");
    std::fs::write(state.join("nan-api-key"), api_key).expect("credential should be written");
    std::fs::write(
        state.join("credential.json"),
        r#"{"schemaVersion":1,"backend":"private-file"}"#,
    )
    .expect("credential receipt should be written");

    let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
        .args(["doctor", "--json"])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("PATH", &empty_path)
        .env("NAN_HARNESS_CONFIG_DIR", &state)
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env_remove("NAN_API_KEY")
        .env("NAN_BASE_URL", &base_url)
        .output()
        .expect("system doctor should start");
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("doctor output should be JSON");
    let request = request.join().expect("model request should finish");

    assert!(output.status.success());
    assert_eq!(report["schemaVersion"], 5);
    assert_eq!(report["provider"]["codingModelCount"], 3);
    assert_eq!(
        report["provider"]["codingModels"],
        serde_json::json!([
            {
                "id": "future-model",
                "contextWindow": 262_144,
                "maxOutputTokens": 32_768,
                "imageInput": false,
                "reasoning": {"kind": "unknown"},
                "source": "generic"
            },
            {
                "id": "gemma4",
                "contextWindow": 262_144,
                "maxOutputTokens": 65_536,
                "imageInput": true,
                "reasoning": {"kind": "toggle", "defaultEnabled": false},
                "source": "bundled"
            },
            {
                "id": "qwen3.6",
                "contextWindow": 262_144,
                "maxOutputTokens": 65_536,
                "imageInput": true,
                "reasoning": {"kind": "toggle", "defaultEnabled": true},
                "source": "bundled"
            }
        ])
    );
    assert_eq!(request.matches("GET /v1/models HTTP/1.1").count(), 1);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains(api_key));
    assert!(!stdout.contains(&base_url));
}
