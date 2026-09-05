use crate::support::{
    capture_interlocked_model_request, capture_one_http_request_with_response, fake_harness,
    fake_interlocked_harness, monitor_http_requests, run_direct_model_launch,
    write_current_verification_receipt, write_private_credential_fixture,
};
use std::process::Command;

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

        let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
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
            .expect("nanh should start");
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

    let mut dry_run = Command::new(env!("CARGO_BIN_EXE_nanh"));
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

    let mut missing = Command::new(env!("CARGO_BIN_EXE_nanh"));
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

    let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
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
        .expect("nanh should start");
    let request = request.join().expect("model request should finish");
    let stderr = String::from_utf8(output.stderr).expect("error output should be UTF-8");

    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("NH-DISCOVERY-005"), "{stderr}");
    assert!(!stderr.contains("NH-RUNTIME"), "{stderr}");
    assert_eq!(request.matches("GET /v1/models HTTP/1.1").count(), 1);
}

#[test]
fn credential_error_precedes_a_concurrent_discovery_error() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let home = directory.path().join("home");
    let state = directory.path().join("state");
    std::fs::create_dir_all(&home).expect("home directory should be created");
    let executable = fake_harness(directory.path(), "development build");

    let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
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
        .expect("nanh should start");
    let stderr = String::from_utf8(output.stderr).expect("error output should be UTF-8");

    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("no NaN API key is configured"), "{stderr}");
    assert!(!stderr.contains("NH-DISCOVERY-005"), "{stderr}");
}

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
