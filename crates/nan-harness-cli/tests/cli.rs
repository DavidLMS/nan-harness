use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nan"))
        .args(arguments)
        .output()
        .expect("nan should start")
}

fn run_alias(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nan-harness"))
        .args(arguments)
        .output()
        .expect("nan-harness alias should start")
}

#[test]
fn help_is_english_and_lists_engineering_commands() {
    let output = run(&["--help"]);
    let stdout = String::from_utf8(output.stdout).expect("help output should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("Run AI coding harnesses through NaN"));
    assert!(stdout.contains("Usage: nan <COMMAND>"));
    assert!(!stdout.contains("  run"));
    assert!(stdout.contains("doctor"));
    assert!(stdout.contains("auth"));
    assert!(stdout.contains("update"));
    assert!(stdout.contains("uninstall"));
    assert!(stdout.contains("telemetry"));
    assert!(!stdout.contains("__record-installation"));
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

#[test]
fn supported_harnesses_expose_reversible_persistence_flags() {
    for harness in ["pi", "prime-agent", "opencode", "qwen", "dsh", "aider"] {
        let output = run(&[harness, "--help"]);
        let stdout = String::from_utf8(output.stdout).expect("help should be UTF-8");

        assert!(output.status.success());
        assert!(stdout.contains("--persist"));
        assert!(stdout.contains("--unpersist"));
    }
}

#[test]
fn removing_an_absent_persistent_integration_is_idempotent() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    for harness in ["pi", "prime-agent", "opencode", "qwen", "dsh", "aider"] {
        let output = Command::new(env!("CARGO_BIN_EXE_nan"))
            .args([harness, "--unpersist"])
            .env("NAN_HARNESS_CONFIG_DIR", directory.path().join("state"))
            .env("HOME", directory.path().join("home"))
            .output()
            .expect("nan should start");
        let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");

        assert!(output.status.success());
        assert!(stdout.contains("No persistent NaN provider is configured"));
    }
}

#[test]
fn root_help_lists_executable_commands_and_aliases() {
    let output = run(&["--help"]);
    let stdout = String::from_utf8(output.stdout).expect("help output should be UTF-8");

    assert!(output.status.success());
    for harness in [
        "claude",
        "claude-code",
        "codex",
        "opencode",
        "hermes",
        "pi",
        "prime-agent",
        "prime",
        "dsh",
        "deepseek",
        "deepseek-harness",
        "openclaw",
        "cline",
        "qwen",
        "qwen-code",
        "kimi",
        "kimi-code",
        "aider",
        "goose",
    ] {
        assert!(stdout.contains(harness), "missing {harness} from root help");
    }
}

#[test]
fn nan_harness_alias_exposes_the_same_command_surface() {
    let primary = run(&["--help"]);
    let alias = run_alias(&["--help"]);
    let alias_help = String::from_utf8(alias.stdout).expect("alias help should be UTF-8");

    assert!(primary.status.success());
    assert!(alias.status.success());
    assert!(alias_help.contains("Usage: nan-harness <COMMAND>"));
    for command in ["claude", "codex", "goose", "doctor", "auth", "telemetry"] {
        assert!(
            alias_help.contains(command),
            "alias help is missing {command}"
        );
    }
    assert!(!alias_help.contains("  run"));
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
    assert!(settings.get("installationId").is_none());
}

#[test]
fn version_matches_the_workspace() {
    let output = run(&["--version"]);
    let stdout = String::from_utf8(output.stdout).expect("version output should be UTF-8");

    assert!(output.status.success());
    assert_eq!(stdout.trim(), format!("nan {}", env!("CARGO_PKG_VERSION")));
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
    assert!(stderr.contains("error [NH-CREDENTIAL-001]"));
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

    let status = Command::new(env!("CARGO_BIN_EXE_nan"))
        .args(["auth", "status"])
        .env("NAN_HARNESS_CONFIG_DIR", &state)
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env_remove("NAN_API_KEY")
        .output()
        .expect("auth status should start");
    let stdout = String::from_utf8(status.stdout).expect("status output should be UTF-8");
    assert!(status.status.success());
    assert_eq!(
        stdout.trim(),
        "NaN API key: configured through the private NaN credential file."
    );
    assert!(!stdout.contains("nan-private-test-key"));

    let logout = Command::new(env!("CARGO_BIN_EXE_nan"))
        .args(["auth", "logout"])
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
    std::fs::write(&executable, "#!/bin/sh\nprintf '%s\\n' 'claude 2.1.233'\n")
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
    assert!(stdout.contains("NaN\n[OK] Version:"));
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
    assert!(stdout.contains("Persistent integrations\n[INFO] None configured"));
    assert!(stdout.contains("Telemetry\n[INFO] Telemetry: off"));
    assert!(stdout.contains("Safe to share:"));
    assert!(!stdout.contains(home.to_string_lossy().as_ref()));
    assert!(!stdout.contains("NAN_API_KEY"));
    assert!(!stderr.contains(home.to_string_lossy().as_ref()));
    assert!(!stderr.contains(private_compatibility_url));
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
    assert!(!stdout.contains(api_key));
    assert!(!stdout.contains(&base_url));
    assert!(request.starts_with("GET /v1/models HTTP/1.1"));
    assert!(request.contains(&format!("authorization: Bearer {api_key}")));
}

fn capture_one_http_request() -> (String, thread::JoinHandle<String>) {
    capture_one_http_request_with_response("{}")
}

fn capture_one_http_request_with_response(
    response_body: &'static str,
) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("capture listener should bind");
    let address = listener
        .local_addr()
        .expect("listener address should exist");
    let request = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("request should connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .expect("read timeout should configure");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        let expected_length = loop {
            let read = stream
                .read(&mut buffer)
                .expect("request should be readable");
            assert_ne!(read, 0, "request ended before its headers");
            request.extend_from_slice(&buffer[..read]);
            if let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                });
                break header_end + 4 + content_length.unwrap_or(0);
            }
        };
        while request.len() < expected_length {
            let read = stream.read(&mut buffer).expect("body should be readable");
            assert_ne!(read, 0, "request ended before its body");
            request.extend_from_slice(&buffer[..read]);
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
            response_body.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("response should be writable");
        String::from_utf8(request).expect("request should be UTF-8")
    });
    (format!("http://{address}"), request)
}

#[cfg(unix)]
#[test]
fn claude_code_dry_run_builds_a_safe_bridge_plan_without_an_api_key() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = fake_claude(directory.path());
    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
        .args([
            "claude",
            "--executable",
            executable.to_str().expect("path should be UTF-8"),
            "--dry-run",
            "--",
            "-p",
            "hello",
        ])
        .env_remove("NAN_API_KEY")
        .output()
        .expect("nan should start");
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
    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
        .args([
            "claude",
            "--executable",
            executable.to_str().expect("path should be UTF-8"),
            "--model",
            "mimo-v2.5",
            "--dry-run",
        ])
        .env_remove("NAN_API_KEY")
        .output()
        .expect("nan should start");
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
        "claude",
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
        "claude",
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
        "claude",
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
        "claude",
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
        ("hermes", "0.20.2", "NAN_API_KEY", "{artifact:hermes-home}"),
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
            "dsh",
            "0.1.0-rc.7",
            "NAN_API_KEY",
            "{artifact:deepseek-harness-patch}",
        ),
        (
            "openclaw",
            "2026.7.1-2",
            "NAN_API_KEY",
            "{artifact:openclaw-config}",
        ),
        (
            "cline",
            "3.0.55",
            "OPENAI_API_KEY",
            "{artifact:cline-config}",
        ),
        ("qwen", "0.21.13", "OPENAI_API_KEY", "OPENAI_MODEL"),
        (
            "kimi",
            "0.36.1",
            "KIMI_MODEL_API_KEY",
            "KIMI_MODEL_DISPLAY_NAME",
        ),
        (
            "aider",
            "aider 0.86.2",
            "AIDER_OPENAI_API_KEY",
            "AIDER_OPENAI_API_BASE",
        ),
        ("goose", "goose 1.46.0", "OPENAI_API_KEY", "GOOSE_PROVIDER"),
    ];

    for (harness, version, credential_target, marker) in cases {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let executable = fake_harness(directory.path(), version);
        let output = Command::new(env!("CARGO_BIN_EXE_nan"))
            .args([
                harness,
                "--executable",
                executable.to_str().expect("path should be UTF-8"),
                "--dry-run",
            ])
            .env_remove("NAN_API_KEY")
            .output()
            .expect("nan should start");
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
fn harness_aliases_remain_executable() {
    let cases = [
        ("claude-code", "2.1.233 (Claude Code)", "claude-code"),
        ("prime", "0.7.2", "prime-agent"),
        ("deepseek", "0.1.0-rc.7", "deepseek-harness"),
        ("deepseek-harness", "0.1.0-rc.7", "deepseek-harness"),
        ("qwen-code", "0.21.13", "qwen-code"),
        ("kimi-code", "0.36.1", "kimi-code"),
    ];

    for (command, version, harness_kind) in cases {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let executable = fake_harness(directory.path(), version);
        let output = run(&[
            command,
            "--executable",
            executable.to_str().expect("path should be UTF-8"),
            "--dry-run",
        ]);
        let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
        let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

        assert!(output.status.success(), "{command}: {stderr}");
        let plan: serde_json::Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|error| panic!("{command} should print a JSON plan: {error}"));

        assert_eq!(plan["harness"]["kind"], harness_kind, "{command}");
    }
}

#[cfg(unix)]
#[test]
fn codex_dry_run_builds_a_safe_responses_bridge_plan() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = fake_harness(directory.path(), "codex-cli 0.146.0");
    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
        .args([
            "codex",
            "--executable",
            executable.to_str().expect("path should be UTF-8"),
            "--dry-run",
        ])
        .env_remove("NAN_API_KEY")
        .output()
        .expect("nan should start");
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
