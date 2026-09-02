use crate::support::{
    capture_one_http_request_with_response, run, run_from_removed_cwd,
    run_with_embedded_compatibility,
};
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

#[test]
fn uninstall_requires_an_installer_managed_executable() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_nan"));
    let output = Command::new(binary)
        .args(["uninstall", "--yes"])
        .env("HOME", directory.path().join("home"))
        .env("NAN_HARNESS_CONFIG_DIR", directory.path().join("state"))
        .output()
        .expect("uninstall should start");
    let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

    assert!(!output.status.success());
    assert!(stderr.contains("error [NH-UNINSTALL-002]"));
    assert!(stderr.contains("not managed by the release installer"));
    assert!(binary.exists());
}

#[test]
fn manual_update_explains_when_a_build_has_no_release_channel() {
    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
        .arg("update")
        .env_remove("NAN_UPDATE_MANIFEST_URL")
        .output()
        .expect("nan update should start");
    let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

    assert!(!output.status.success());
    assert!(stderr.contains("error [NH-UPDATE-001]"));
    assert!(stderr.contains("does not have an update channel configured"));
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
        let output = Command::new(env!("CARGO_BIN_EXE_nan"))
            .args([harness, "--dry-run"])
            .env("PATH", path.path())
            .env("HOME", home.path())
            .env_remove("USERPROFILE")
            .env_remove("NAN_UPDATE_MANIFEST_URL")
            .env_remove("NAN_HARNESS_GLITCHTIP_DSN")
            .output()
            .expect("nan should start");
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
    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
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
    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
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
    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
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
fn uninstall_kimi_script_removes_binaries_and_optionally_user_data() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let kimi_home = home.path().join(".kimi-code");
    std::fs::create_dir_all(kimi_home.join("bin")).expect("Kimi bin directory should exist");
    std::fs::write(kimi_home.join("bin/kimi"), "fake kimi").expect("binary should exist");
    std::fs::write(kimi_home.join("config.toml"), "user config").expect("config should exist");
    std::fs::write(
        home.path().join(".zshrc"),
        "export PATH=\"$HOME/.kimi-code/bin:$PATH\"\nkeep=true\n",
    )
    .expect("shell configuration should exist");

    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/uninstall-kimi.sh");
    let output = Command::new("bash")
        .arg(&script)
        .env("HOME", home.path())
        .output()
        .expect("uninstall helper should run");
    assert!(output.status.success());
    assert!(!kimi_home.join("bin/kimi").exists());
    assert!(kimi_home.join("config.toml").exists());
    let shell_config = std::fs::read_to_string(home.path().join(".zshrc"))
        .expect("shell configuration should remain");
    assert_eq!(shell_config, "keep=true\n");

    std::fs::write(kimi_home.join("bin/kimi"), "fake kimi").expect("binary should exist");
    let output = Command::new("bash")
        .args([
            script.to_str().expect("script path should be UTF-8"),
            "--purge",
            "--yes",
        ])
        .env("HOME", home.path())
        .output()
        .expect("purge helper should run");
    assert!(output.status.success());
    assert!(!kimi_home.exists());
}

#[cfg(unix)]
#[test]
fn uninstall_kimi_script_separates_install_and_data_directories() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let install_directory = home.path().join("custom-kimi-install");
    let data_directory = home.path().join("custom-kimi-data");
    std::fs::create_dir_all(install_directory.join("bin"))
        .expect("Kimi install directory should exist");
    std::fs::create_dir_all(&data_directory).expect("Kimi data directory should exist");
    std::fs::write(install_directory.join("bin/kimi"), "fake kimi").expect("binary should exist");
    std::fs::write(data_directory.join("config.toml"), "user config").expect("config should exist");
    std::fs::write(
        home.path().join(".profile"),
        format!(
            "export PATH=\"{}/bin:$PATH\"\nkeep=true\n",
            install_directory.display()
        ),
    )
    .expect("shell configuration should exist");

    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/uninstall-kimi.sh");
    let output = Command::new("bash")
        .arg(&script)
        .env("HOME", home.path())
        .env("KIMI_INSTALL_DIR", &install_directory)
        .env("KIMI_CODE_HOME", &data_directory)
        .output()
        .expect("uninstall helper should run");

    assert!(output.status.success());
    assert!(!install_directory.join("bin/kimi").exists());
    assert!(data_directory.join("config.toml").exists());
    let shell_config = std::fs::read_to_string(home.path().join(".profile"))
        .expect("shell configuration should remain");
    assert_eq!(shell_config, "keep=true\n");
}

#[cfg(unix)]
#[test]
fn uninstall_kimi_script_rejects_home_with_a_trailing_slash_as_data_directory() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let sentinel = home.path().join("keep.txt");
    std::fs::write(&sentinel, "keep").expect("sentinel should exist");
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/uninstall-kimi.sh");
    let unsafe_data_directory = format!("{}/", home.path().display());

    let output = Command::new("bash")
        .args([
            script.to_str().expect("script path should be UTF-8"),
            "--purge",
            "--yes",
        ])
        .env("HOME", home.path())
        .env("KIMI_CODE_HOME", unsafe_data_directory)
        .output()
        .expect("uninstall helper should run");

    assert!(!output.status.success());
    assert!(sentinel.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsafe Kimi Code home"));
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

#[cfg(unix)]
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
    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
        .args(["doctor", "--json"])
        .env("HOME", &home)
        .env("PATH", &path)
        .env("NAN_HARNESS_CONFIG_DIR", &state)
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
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

    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
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

    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
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
        Some(4)
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

    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
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

    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
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
