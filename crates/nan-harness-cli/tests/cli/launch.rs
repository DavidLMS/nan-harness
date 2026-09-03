#[cfg(unix)]
use crate::support::{
    capture_interlocked_model_request, capture_one_http_request_with_response, fake_claude,
    fake_claude_with_version, fake_harness, fake_interlocked_harness, monitor_http_requests, run,
    run_direct_model_launch, run_with_embedded_compatibility, write_current_verification_receipt,
    write_private_credential_fixture,
};
use std::process::Command;

#[cfg(unix)]
#[test]
fn explicit_absent_model_uses_one_catalog_with_and_without_gateway() {
    const WARNING: &str = "warning: model 'future-model' was not returned by live discovery and has no bundled capability profile; attempting it with conservative defaults because you selected it explicitly.";

    for disable_gateway in [false, true] {
        let (directory, output, request) =
            run_direct_model_launch(Some("future-model"), disable_gateway, None);
        let stderr = String::from_utf8(output.stderr).expect("error output should be UTF-8");

        assert!(output.status.success(), "{stderr}");
        assert_eq!(request.matches("GET /v1/models HTTP/1.1").count(), 1);
        assert_eq!(stderr.matches(WARNING).count(), 1);
        let gateway_warning = "warning: Chat Completions gateway disabled for this launch. The harness will receive the provider credential directly; usage accounting and gateway-dependent features are unavailable.";
        assert_eq!(
            stderr.matches(gateway_warning).count(),
            usize::from(disable_gateway)
        );
        assert!(directory.path().join("state/preferences.json").exists());
    }
}

#[cfg(unix)]
#[test]
fn harness_inspection_and_model_discovery_overlap_with_or_without_a_current_receipt() {
    for current_receipt in [false, true] {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let home = directory.path().join("home");
        let state = directory.path().join("state");
        std::fs::create_dir_all(&home).expect("home directory should be created");
        std::fs::create_dir_all(&state).expect("state directory should be created");
        write_private_credential_fixture(&state, "local-test-key");
        let version_started = directory.path().join("version-started");
        let models_started = directory.path().join("models-started");
        let (endpoint, request) =
            capture_interlocked_model_request(version_started.clone(), models_started.clone());
        let provider_base_url = format!("{endpoint}/v1");
        if current_receipt {
            write_current_verification_receipt(&state, &provider_base_url, "local-test-key");
        }
        let executable = fake_interlocked_harness(directory.path());

        let output = Command::new(env!("CARGO_BIN_EXE_nan"))
            .args([
                "pi",
                "--executable",
                executable.to_str().expect("path should be UTF-8"),
                "--provider-base-url",
                &provider_base_url,
                "--no-search",
            ])
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("NAN_HARNESS_CONFIG_DIR", &state)
            .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
            .env("NAN_NO_COMPATIBILITY_CHECK", "1")
            .env("NAN_TEST_VERSION_STARTED", &version_started)
            .env("NAN_TEST_MODELS_STARTED", &models_started)
            .env_remove("NAN_API_KEY")
            .env_remove("NAN_UPDATE_MANIFEST_URL")
            .env_remove("NAN_HARNESS_GLITCHTIP_DSN")
            .output()
            .expect("nan should start");
        let request = request.join().expect("model request should finish");
        let stderr = String::from_utf8(output.stderr).expect("error output should be UTF-8");

        assert!(
            output.status.success(),
            "current receipt {current_receipt}: {stderr}"
        );
        assert!(version_started.exists());
        assert!(models_started.exists());
        assert_eq!(request.matches("GET /v1/models HTTP/1.1").count(), 1);
    }
}

#[cfg(unix)]
#[test]
fn dry_run_and_missing_harness_do_not_contact_model_discovery() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let home = directory.path().join("home");
    let state = directory.path().join("state");
    let empty_path = directory.path().join("empty-path");
    std::fs::create_dir_all(&home).expect("home directory should be created");
    std::fs::create_dir_all(&state).expect("state directory should be created");
    std::fs::create_dir(&empty_path).expect("empty PATH should be created");
    let (endpoint, stop, requests) = monitor_http_requests();
    let provider_base_url = format!("{endpoint}/v1");
    let executable = fake_harness(directory.path(), "0.84.2");
    let common_environment = |command: &mut Command| {
        command
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("NAN_HARNESS_CONFIG_DIR", &state)
            .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
            .env("NAN_NO_COMPATIBILITY_CHECK", "1")
            .env("NAN_API_KEY", "local-test-key")
            .env_remove("NAN_UPDATE_MANIFEST_URL")
            .env_remove("NAN_HARNESS_GLITCHTIP_DSN");
    };

    let mut dry_run = Command::new(env!("CARGO_BIN_EXE_nan"));
    dry_run.args([
        "pi",
        "--executable",
        executable.to_str().expect("path should be UTF-8"),
        "--provider-base-url",
        &provider_base_url,
        "--dry-run",
    ]);
    common_environment(&mut dry_run);
    let dry_run = dry_run.output().expect("dry-run should start");

    let mut missing = Command::new(env!("CARGO_BIN_EXE_nan"));
    missing
        .args([
            "pi",
            "--provider-base-url",
            &provider_base_url,
            "--no-search",
        ])
        .env("PATH", &empty_path);
    common_environment(&mut missing);
    let missing = missing
        .output()
        .expect("missing harness launch should start");

    stop.send(()).expect("request monitor should stop");
    let requests = requests.join().expect("request monitor should finish");
    let dry_run_stderr = String::from_utf8_lossy(&dry_run.stderr);
    let missing_stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(dry_run.status.success(), "{dry_run_stderr}");
    assert!(!missing.status.success(), "{missing_stderr}");
    assert!(missing_stderr.contains("NH-DISCOVERY-002"));
    assert!(
        requests.is_empty(),
        "unexpected model requests: {requests:?}"
    );
}

#[cfg(unix)]
#[test]
fn discovery_error_precedes_a_concurrent_runtime_catalog_error() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let home = directory.path().join("home");
    let state = directory.path().join("state");
    std::fs::create_dir_all(&home).expect("home directory should be created");
    std::fs::create_dir_all(&state).expect("state directory should be created");
    write_private_credential_fixture(&state, "local-test-key");
    let (endpoint, request) = capture_one_http_request_with_response("{}");
    let provider_base_url = format!("{endpoint}/v1");
    write_current_verification_receipt(&state, &provider_base_url, "local-test-key");
    let executable = fake_harness(directory.path(), "development build");

    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
        .args([
            "pi",
            "--executable",
            executable.to_str().expect("path should be UTF-8"),
            "--provider-base-url",
            &provider_base_url,
            "--no-search",
        ])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("NAN_HARNESS_CONFIG_DIR", &state)
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env_remove("NAN_API_KEY")
        .env_remove("NAN_UPDATE_MANIFEST_URL")
        .env_remove("NAN_HARNESS_GLITCHTIP_DSN")
        .output()
        .expect("nan should start");
    let request = request.join().expect("model request should finish");
    let stderr = String::from_utf8(output.stderr).expect("error output should be UTF-8");

    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("NH-DISCOVERY-005"), "{stderr}");
    assert!(!stderr.contains("NH-RUNTIME"), "{stderr}");
    assert_eq!(request.matches("GET /v1/models HTTP/1.1").count(), 1);
}

#[cfg(unix)]
#[test]
fn credential_error_precedes_a_concurrent_discovery_error() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let home = directory.path().join("home");
    let state = directory.path().join("state");
    std::fs::create_dir_all(&home).expect("home directory should be created");
    let executable = fake_harness(directory.path(), "development build");

    let output = Command::new(env!("CARGO_BIN_EXE_nan"))
        .args([
            "pi",
            "--executable",
            executable.to_str().expect("path should be UTF-8"),
            "--no-search",
        ])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("NAN_HARNESS_CONFIG_DIR", &state)
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env_remove("NAN_API_KEY")
        .env_remove("NAN_UPDATE_MANIFEST_URL")
        .env_remove("NAN_HARNESS_GLITCHTIP_DSN")
        .output()
        .expect("nan should start");
    let stderr = String::from_utf8(output.stderr).expect("error output should be UTF-8");

    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("no NaN API key is configured"), "{stderr}");
    assert!(!stderr.contains("NH-DISCOVERY-005"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn successful_remembered_model_fallback_updates_preferences() {
    const WARNING: &str = "warning: model 'retired-model' is no longer available for this credential; using 'qwen3.6'.";
    let preferences = r#"{"schemaVersion":2,"lastSelectionByHarness":{"pi":{"model":"retired-model","reasoning":null}}}"#;
    let (directory, output, request) = run_direct_model_launch(None, false, Some(preferences));
    let stderr = String::from_utf8(output.stderr).expect("error output should be UTF-8");

    assert!(output.status.success(), "{stderr}");
    assert_eq!(request.matches("GET /v1/models HTTP/1.1").count(), 1);
    assert_eq!(stderr.matches(WARNING).count(), 1);
    let persisted: serde_json::Value = serde_json::from_slice(
        &std::fs::read(directory.path().join("state/preferences.json"))
            .expect("preferences should remain readable"),
    )
    .expect("preferences should remain valid JSON");
    assert_eq!(persisted["schemaVersion"], 3);
    assert_eq!(
        persisted["lastSelectionByHarness"]["pi"]["model"],
        "qwen3.6"
    );
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
    assert!(!stdout.contains("{runtime:claude_model_picker}"));
    assert!(!stdout.contains("nan-test-secret"));
}

#[cfg(unix)]
#[test]
fn claude_code_dry_run_enables_model_picker_for_supported_versions() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = fake_claude_with_version(directory.path(), "2.1.251 (Claude Code)");
    let output = run_with_embedded_compatibility(&[
        "claude",
        "--executable",
        executable.to_str().expect("path should be UTF-8"),
        "--dry-run",
    ]);
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("claude-model-picker"));
    assert!(stdout.contains("{runtime:claude_model_picker}"));
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
    assert!(stderr.contains("conflicts with nan-harness routing"));
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
    let executable = fake_claude_with_version(directory.path(), "2.1.252 (Claude Code)");
    let output = run_with_embedded_compatibility(&[
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
    assert!(stderr.contains(
        "newer than the last version confirmed compatible with this nan-harness release"
    ));
    assert!(stderr.contains("2.1.251"));
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
            "omp",
            "18.0.11",
            "NAN_API_KEY",
            "{artifact:omp-provider-extension}",
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
        let empty_path = directory.path().join("empty-path");
        std::fs::create_dir(&empty_path).expect("empty PATH should exist");
        let mut command = Command::new(env!("CARGO_BIN_EXE_nan-harness"));
        command
            .args([
                harness,
                "--executable",
                executable.to_str().expect("path should be UTF-8"),
                "--dry-run",
            ])
            .env("NAN_HARNESS_CONFIG_DIR", directory.path().join("nan-state"))
            .env("NAN_NO_COMPATIBILITY_CHECK", "1")
            .env_remove("NAN_API_KEY")
            .env("HOME", directory.path().join("home"));
        if harness == "dsh" {
            command.env("PATH", empty_path);
        }
        let output = command.output().expect("nan should start");
        let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
        let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

        assert!(output.status.success(), "{harness}: {stderr}");
        assert!(stdout.contains("\"kind\": \"direct-chat\""));
        assert!(stdout.contains("{runtime:provider_base_url}"));
        assert!(stdout.contains(credential_target));
        assert!(
            stdout.contains(marker),
            "{harness}: expected marker {marker}, got:\n{stdout}"
        );
        assert!(!stdout.contains("nan-secret-value"));
    }
}

#[cfg(unix)]
#[test]
fn gateway_escape_hatch_dry_run_explains_its_effect() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = fake_harness(directory.path(), "0.84.2");
    let output = run_with_embedded_compatibility(&[
        "pi",
        "--executable",
        executable.to_str().expect("path should be UTF-8"),
        "--no-chat-gateway",
        "--dry-run",
    ]);
    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");

    assert!(output.status.success(), "{stderr}");
    assert!(stdout.contains("\"kind\": \"direct-chat\""));
    assert!(stderr.contains("gateway would be disabled for this launch"));
    assert!(stderr.contains("provider credential directly"));
    assert!(stderr.contains("usage accounting and gateway-dependent features"));
}

#[cfg(unix)]
#[test]
fn harness_aliases_remain_executable() {
    let cases = [
        ("claude-code", "2.1.233 (Claude Code)", "claude-code"),
        ("oh-my-pi", "18.0.11", "omp"),
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

#[test]
fn desktop_dry_runs_are_offline_inert_and_typed() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let state = directory.path().join("state-that-must-not-exist");
    let hermes_home = directory.path().join("hermes-that-must-not-exist");
    for (arguments, harness, transport) in [
        (
            vec!["chatgpt-desktop", "--model", "qwen3.6", "--dry-run"],
            "chatgpt-desktop",
            "responses-bridge",
        ),
        (
            vec!["claude-desktop", "--force-search", "--dry-run"],
            "claude-desktop",
            "anthropic-bridge",
        ),
        (
            vec![
                "hermes-desktop",
                "--no-chat-gateway",
                "--dry-run",
                "--",
                "--source",
                "local",
            ],
            "hermes-desktop",
            "direct-chat-completions",
        ),
        (
            vec!["pen", "--model", "qwen3.6", "--dry-run"],
            "pen-desktop",
            "chat-completions-gateway",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_nan"))
            .args(arguments)
            .env("NAN_HARNESS_CONFIG_DIR", &state)
            .env("HERMES_HOME", &hermes_home)
            .env_remove("NAN_API_KEY")
            .env("NAN_BASE_URL", "not-a-valid-provider-url")
            .output()
            .expect("Desktop dry run should start");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{harness}: {stderr}");
        let plan: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("dry run should print JSON");
        assert_eq!(plan["schemaVersion"], 1);
        assert_eq!(plan["harness"], harness);
        assert_eq!(plan["transport"], transport);
        assert_eq!(plan["experimental"], true);
        assert!(plan.get("credential").is_none());
        assert!(!state.exists(), "{harness} dry run wrote state");
        assert!(!hermes_home.exists(), "{harness} dry run wrote a profile");
    }
}
