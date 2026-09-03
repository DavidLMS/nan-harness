use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

fn paths() -> (tempfile::TempDir, DesktopPaths) {
    let root = tempfile::tempdir().expect("temp root");
    let paths = DesktopPaths::new(
        &root.path().join("Claude"),
        &root.path().join("Claude-3p"),
        &root.path().join("state"),
    );
    (root, paths)
}

#[test]
fn dry_run_plan_preserves_model_executable_diagnostics_and_search_policy() {
    let arguments = ClaudeDesktopArgs {
        model: Some("qwen3.6".to_owned()),
        provider_base_url: None,
        executable: Some(PathBuf::from("/tmp/claude")),
        allow_unsupported: false,
        allow_untested: false,
        search: crate::app::WebSearchArgs {
            no_search: false,
            force_search: true,
        },
        dry_run: true,
        show_auto: true,
        restore: false,
    };

    let plan = dry_run_plan(&arguments);

    assert_eq!(plan.harness, DesktopHarnessKind::Claude);
    assert_eq!(plan.transport, DesktopTransport::AnthropicBridge);
    assert_eq!(plan.executable, arguments.executable);
    assert_eq!(plan.selected_model, arguments.model);
    assert_eq!(plan.web_search_policy, WebSearchPolicy::Force);
    assert!(plan.private_diagnostics);
}

#[test]
fn macos_paths_use_application_support_and_accept_a_nan_override() {
    let environment = DesktopEnvironment {
        home: Some(PathBuf::from("/Users/tester")),
        nan_config: Some(PathBuf::from("/private/nan")),
        ..DesktopEnvironment::default()
    };

    let paths = DesktopPaths::from_platform_environment(DesktopPlatform::Macos, &environment)
        .expect("macOS paths");

    assert_eq!(
        paths.normal_config,
        PathBuf::from(
            "/Users/tester/Library/Application Support/Claude/claude_desktop_config.json"
        )
    );
    assert_eq!(
        paths.profile,
        PathBuf::from(
            "/Users/tester/Library/Application Support/Claude-3p/configLibrary/6e616e68-6172-4e65-8000-000000000001.json"
        )
    );
    assert_eq!(
        paths.receipt,
        PathBuf::from("/private/nan/claude-desktop-receipt.json")
    );
}

#[test]
fn linux_paths_follow_xdg_config_home() {
    let environment = DesktopEnvironment {
        home: Some(PathBuf::from("/home/tester")),
        xdg_config: Some(PathBuf::from("/var/lib/tester/config")),
        ..DesktopEnvironment::default()
    };

    let paths = DesktopPaths::from_platform_environment(DesktopPlatform::Linux, &environment)
        .expect("Linux XDG paths");

    assert_eq!(
        paths.normal_config,
        PathBuf::from("/var/lib/tester/config/Claude/claude_desktop_config.json")
    );
    assert_eq!(
        paths.third_party_config,
        PathBuf::from("/var/lib/tester/config/Claude-3p/claude_desktop_config.json")
    );
    assert_eq!(
        paths.lock,
        PathBuf::from("/var/lib/tester/config/nan-harness/claude-desktop.lock")
    );
}

#[test]
fn linux_paths_fall_back_to_the_home_config_directory() {
    let environment = DesktopEnvironment {
        home: Some(PathBuf::from("/home/tester")),
        ..DesktopEnvironment::default()
    };

    let paths = DesktopPaths::from_platform_environment(DesktopPlatform::Linux, &environment)
        .expect("Linux home paths");

    assert_eq!(
        paths.normal_config,
        PathBuf::from("/home/tester/.config/Claude/claude_desktop_config.json")
    );
    assert_eq!(
        paths.profile,
        PathBuf::from(
            "/home/tester/.config/Claude-3p/configLibrary/6e616e68-6172-4e65-8000-000000000001.json"
        )
    );
}

#[test]
fn windows_paths_separate_roaming_standard_state_from_local_third_party_state() {
    let environment = DesktopEnvironment {
        app_data: Some(PathBuf::from("roaming")),
        local_app_data: Some(PathBuf::from("local")),
        ..DesktopEnvironment::default()
    };

    let paths = DesktopPaths::from_platform_environment(DesktopPlatform::Windows, &environment)
        .expect("Windows paths");

    assert_eq!(
        paths.normal_config,
        PathBuf::from("roaming/Claude/claude_desktop_config.json")
    );
    assert_eq!(
        paths.third_party_config,
        PathBuf::from("local/Claude-3p/claude_desktop_config.json")
    );
    assert_eq!(
        paths.receipt,
        PathBuf::from("roaming/nan-harness/claude-desktop-receipt.json")
    );
}

#[test]
fn windows_tasklist_detection_ignores_localized_empty_output() {
    assert!(!tasklist_reports_desktop(
        b"INFO: No tasks are running which match the specified criteria.\r\n"
    ));
    assert!(tasklist_reports_desktop(
        b"\"Claude.exe\",\"2312\",\"Console\",\"1\",\"100,000 K\"\r\n"
    ));
    assert!(tasklist_reports_desktop(
        b"\"claude.exe\",\"2312\",\"Console\",\"1\",\"100,000 K\"\r\n"
    ));
}

#[test]
fn auto_mode_activity_renders_the_provider_request() {
    let message = render_bridge_activity(&BridgeActivity::ClaudeAutoModeReview {
        review_id: 7,
        stage: ClaudeAutoModeReviewStage::Initial,
        model_id: "qwen3.6".to_owned(),
        request: nan_harness_runtime::ClaudeAutoModeTracePayload::new(
            r#"{"model":"qwen3.6","temperature":0}"#,
        ),
    });

    assert_eq!(
        message,
        concat!(
            "[Auto #7] Claude requested a permission review (stage 1, classifier qwen3.6).\n",
            "[Auto #7] NaN request:\n",
            "{\n  \"model\": \"qwen3.6\",\n  \"temperature\": 0\n}"
        )
    );
}

#[test]
fn auto_mode_response_pretty_prints_json_and_preserves_non_json_bodies() {
    let response = render_bridge_activity(&BridgeActivity::ClaudeAutoModeReviewResponse {
        review_id: 7,
        status: 200,
        response: nan_harness_runtime::ClaudeAutoModeTracePayload::new(
            r#"{"choices":[{"message":{"content":"reviewed"}}]}"#,
        ),
    });
    assert!(response.contains("[Auto #7] NaN response (HTTP 200):"));
    assert!(response.contains("\"content\": \"reviewed\""));

    let plain_text = "provider response body\n";
    let response = render_bridge_activity(&BridgeActivity::ClaudeAutoModeReviewResponse {
        review_id: 8,
        status: 200,
        response: nan_harness_runtime::ClaudeAutoModeTracePayload::new(plain_text),
    });
    assert!(response.ends_with(plain_text));
}

#[test]
fn auto_mode_failure_is_correlated_without_transport_details() {
    let message = render_bridge_activity(&BridgeActivity::ClaudeAutoModeReviewFailed {
        review_id: 9,
        error_code: "NH-BRIDGE-103",
    });

    assert_eq!(
        message,
        "[Auto #9] NaN request failed before a response was received (NH-BRIDGE-103)."
    );
}

#[test]
fn launch_message_mentions_auto_only_when_tracing_is_enabled() {
    assert_eq!(
        launch_message(false),
        "Claude Desktop launched through NaN."
    );
    assert!(!launch_message(false).contains("Auto"));
    assert!(launch_message(true).contains("Auto traces will appear here"));
    assert!(launch_message(true).contains("private data"));
}

#[test]
fn apply_preserves_unknown_fields_and_restore_is_exact() {
    let (_root, paths) = paths();
    fs::create_dir_all(paths.normal_config.parent().expect("parent")).expect("dir");
    fs::write(
        &paths.normal_config,
        b"{\"unknown\":{\"kept\":true},\"deploymentMode\":\"1p\"}\n",
    )
    .expect("original");
    let original = fs::read(&paths.normal_config).expect("read original");
    let receipt = Receipt::capture(&paths).expect("capture");
    receipt.write(&paths.receipt).expect("receipt");
    apply_gateway(&paths, "http://127.0.0.1:1234", "session-only").expect("apply");
    let active: Value =
        serde_json::from_slice(&fs::read(&paths.normal_config).expect("read active"))
            .expect("json");
    assert_eq!(active["unknown"]["kept"], true);
    let active_profile: Value =
        serde_json::from_slice(&fs::read(&paths.profile).expect("read active profile"))
            .expect("profile json");
    assert_eq!(active_profile["modelDiscoveryEnabled"], true);
    assert_eq!(active_profile["autoModeEnabled"], true);
    restore_receipt(&paths).expect("restore");
    assert_eq!(
        fs::read(&paths.normal_config).expect("read restored"),
        original
    );
    assert!(!paths.profile.exists());
}

#[test]
fn receipt_json_never_contains_backed_up_config_or_provider_key() {
    let (_root, paths) = paths();
    let provider_key = "real-provider-secret";
    fs::create_dir_all(paths.profile.parent().expect("parent")).expect("profile directory");
    fs::write(
        &paths.profile,
        format!(r#"{{"inferenceGatewayApiKey":"{provider_key}","unknown":true}}"#),
    )
    .expect("original profile");
    let receipt = Receipt::capture(&paths).expect("capture");
    receipt.write(&paths.receipt).expect("receipt");
    apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");
    let receipt_text = fs::read_to_string(&paths.receipt).expect("receipt text");
    assert!(
        !receipt_text.contains(provider_key),
        "receipt metadata copied original configuration contents"
    );
    assert!(!receipt_text.contains("inferenceGatewayApiKey"));
    assert!(!receipt_text.contains("session-token"));
    assert!(
        !fs::read_to_string(&paths.profile)
            .expect("profile text")
            .contains(provider_key)
    );
    assert!(
        fs::read_to_string(&paths.profile)
            .expect("profile text")
            .contains("session-token")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let backup = paths.backup_directory.join("document-3.backup");
        assert_eq!(
            fs::metadata(backup)
                .expect("backup metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn stale_receipt_recovers_all_documents() {
    let (_root, paths) = paths();
    fs::create_dir_all(paths.meta.parent().expect("parent")).expect("dir");
    fs::write(&paths.meta, b"{\"before\":1}").expect("original");
    Receipt::capture(&paths)
        .expect("capture")
        .write(&paths.receipt)
        .expect("receipt");
    fs::write(&paths.meta, b"{\"after\":2}").expect("changed");
    restore_receipt(&paths).expect("restore");
    assert_eq!(fs::read(&paths.meta).expect("restored"), b"{\"before\":1}");
    assert!(!paths.receipt.exists());
}

#[test]
fn normal_start_rejects_orphan_backup_without_deleting_it() {
    let (_root, paths) = paths();
    fs::create_dir_all(&paths.backup_directory).expect("backup directory");
    let sentinel = paths.backup_directory.join("inspect-me.backup");
    fs::write(&sentinel, b"recoverable configuration").expect("orphan backup");

    let error = ensure_no_pending_recovery(&paths).expect_err("orphan should block startup");

    assert!(matches!(error, ClaudeDesktopError::OrphanBackup));
    assert_eq!(
        fs::read(sentinel).expect("orphan backup should remain"),
        b"recoverable configuration"
    );
}

#[test]
fn restore_reports_orphan_backup_when_receipt_is_missing() {
    let (_root, paths) = paths();
    fs::create_dir_all(&paths.backup_directory).expect("backup directory");
    let sentinel = paths.backup_directory.join("inspect-me.backup");
    fs::write(&sentinel, b"recoverable configuration").expect("orphan backup");

    let error = restore_receipt(&paths).expect_err("orphan should require inspection");

    assert!(matches!(error, ClaudeDesktopError::OrphanBackup));
    assert!(sentinel.exists(), "orphan backup should remain recoverable");
}

#[test]
fn session_lock_rejects_concurrency() {
    let (_root, paths) = paths();
    let _first = SessionLock::acquire(&paths.lock).expect("first lock");
    assert!(matches!(
        SessionLock::acquire(&paths.lock),
        Err(ClaudeDesktopError::ConcurrentSession)
    ));
}

#[cfg(unix)]
#[test]
fn configuration_symlinks_are_rejected_without_touching_the_target() {
    use std::os::unix::fs::symlink;

    let (_root, paths) = paths();
    let target = paths
        .normal_config
        .parent()
        .expect("normal parent")
        .join("user-owned.json");
    fs::create_dir_all(target.parent().expect("target parent")).expect("target directory");
    fs::write(&target, b"{\"private\":true}").expect("target contents");
    symlink(&target, &paths.normal_config).expect("configuration symlink");

    let error = Receipt::capture(&paths).expect_err("symlink must be rejected");

    assert!(matches!(error, ClaudeDesktopError::UnsafeSymlink));
    assert_eq!(
        fs::read(&target).expect("target should remain readable"),
        b"{\"private\":true}"
    );
    assert!(
        fs::symlink_metadata(&paths.normal_config)
            .expect("symlink should remain")
            .file_type()
            .is_symlink()
    );
    assert!(!paths.backup_directory.exists());
}

struct FakeProcess {
    profile: PathBuf,
    available: AtomicBool,
    running: AtomicBool,
    terminated: AtomicBool,
    force_terminated: AtomicBool,
    terminated_while_gateway_active: AtomicBool,
    fail_checks: AtomicBool,
    transient_check_failures: AtomicUsize,
    fail_terminate: AtomicBool,
    fail_force_terminate: AtomicBool,
}

impl FakeProcess {
    fn running(profile: PathBuf) -> Self {
        Self {
            profile,
            available: AtomicBool::new(true),
            running: AtomicBool::new(true),
            terminated: AtomicBool::new(false),
            force_terminated: AtomicBool::new(false),
            terminated_while_gateway_active: AtomicBool::new(false),
            fail_checks: AtomicBool::new(false),
            transient_check_failures: AtomicUsize::new(0),
            fail_terminate: AtomicBool::new(false),
            fail_force_terminate: AtomicBool::new(false),
        }
    }
}

impl DesktopProcess for FakeProcess {
    fn ensure_available(&self) -> Result<(), ClaudeDesktopError> {
        if self.available.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(ClaudeDesktopError::AppNotFound { platform: "test" })
        }
    }

    fn is_running(&self) -> Result<bool, ClaudeDesktopError> {
        let transient_failure = self
            .transient_check_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok();
        if self.fail_checks.load(Ordering::SeqCst) || transient_failure {
            return Err(ClaudeDesktopError::ProcessCheck(std::io::Error::other(
                "synthetic process check failure",
            )));
        }
        Ok(self.running.load(Ordering::SeqCst))
    }

    fn launch(&self) -> Result<(), ClaudeDesktopError> {
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn terminate(&self) -> Result<(), ClaudeDesktopError> {
        if self.fail_terminate.load(Ordering::SeqCst) {
            return Err(ClaudeDesktopError::Terminate(std::io::Error::other(
                "synthetic termination failure",
            )));
        }
        let gateway_active = read_json_object(&self.profile).is_ok_and(|profile| {
            profile.get("inferenceProvider").and_then(Value::as_str) == Some("gateway")
        });
        self.terminated_while_gateway_active
            .store(gateway_active, Ordering::SeqCst);
        self.terminated.store(true, Ordering::SeqCst);
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn force_terminate(&self) -> Result<(), ClaudeDesktopError> {
        self.force_terminated.store(true, Ordering::SeqCst);
        if self.fail_force_terminate.load(Ordering::SeqCst) {
            return Err(ClaudeDesktopError::Terminate(std::io::Error::other(
                "synthetic forced termination failure",
            )));
        }
        self.terminated.store(true, Ordering::SeqCst);
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn missing_desktop_is_rejected_before_session_state_setup() {
    let (_root, paths) = paths();
    let process = FakeProcess::running(paths.profile.clone());
    process.available.store(false, Ordering::SeqCst);

    assert!(matches!(
        prepare_session_lock(&paths, &process),
        Err(ClaudeDesktopError::AppNotFound { .. })
    ));
    assert!(!paths.lock.exists());
    assert!(!paths.receipt.exists());
    assert!(!paths.backup_directory.exists());
}

#[tokio::test]
async fn signal_terminates_desktop_before_exact_restore() {
    let (_root, paths) = paths();
    fs::create_dir_all(paths.profile.parent().expect("parent")).expect("profile directory");
    let original = b"{\"userField\":\"original\"}\n";
    fs::write(&paths.profile, original).expect("original profile");
    Receipt::capture(&paths)
        .expect("capture")
        .write(&paths.receipt)
        .expect("receipt");
    apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");
    let process = FakeProcess::running(paths.profile.clone());

    let exit_code = complete_and_restore(&paths, &process, Ok(WaitOutcome::Signaled(130)))
        .await
        .expect("signal cleanup");

    assert_eq!(exit_code, 130);
    assert!(process.terminated.load(Ordering::SeqCst));
    assert!(
        process
            .terminated_while_gateway_active
            .load(Ordering::SeqCst),
        "profile was restored before Claude Desktop was terminated"
    );
    assert_eq!(
        fs::read(&paths.profile).expect("restored profile"),
        original
    );
    assert!(!paths.receipt.exists());
    assert!(!paths.backup_directory.exists());
}

#[tokio::test]
async fn process_wait_error_still_restores_exact_configuration() {
    let (_root, paths) = paths();
    fs::create_dir_all(paths.normal_config.parent().expect("parent")).expect("config directory");
    let original = b"{\"deploymentMode\":\"1p\",\"kept\":7}\n";
    fs::write(&paths.normal_config, original).expect("original config");
    Receipt::capture(&paths)
        .expect("capture")
        .write(&paths.receipt)
        .expect("receipt");
    apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");
    let process = FakeProcess::running(paths.profile.clone());
    process.transient_check_failures.store(1, Ordering::SeqCst);
    let wait_error = wait_for_exit_or_signal(&process).await;

    let error = complete_and_restore(&paths, &process, wait_error)
        .await
        .expect_err("process error should propagate");

    assert!(matches!(error, ClaudeDesktopError::ProcessCheck(_)));
    assert!(process.terminated.load(Ordering::SeqCst));
    assert_eq!(
        fs::read(&paths.normal_config).expect("restored config"),
        original
    );
    assert!(!paths.receipt.exists());
    assert!(!paths.backup_directory.exists());
}

#[test]
fn apply_error_restores_before_launch() {
    let (_root, paths) = paths();
    fs::create_dir_all(paths.normal_config.parent().expect("parent")).expect("config directory");
    let original = b"{\"deploymentMode\":\"1p\",\"kept\":8}\n";
    fs::write(&paths.normal_config, original).expect("original config");
    Receipt::capture(&paths)
        .expect("capture")
        .write(&paths.receipt)
        .expect("receipt");
    apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");

    let error = restore_after(&paths, Err(ClaudeDesktopError::ConfigRoot))
        .expect_err("apply error should propagate");

    assert!(matches!(error, ClaudeDesktopError::ConfigRoot));
    assert_eq!(
        fs::read(&paths.normal_config).expect("restored config"),
        original
    );
    assert!(!paths.receipt.exists());
    assert!(!paths.backup_directory.exists());
}

#[tokio::test]
async fn launch_error_terminates_partial_launch_before_restore() {
    let (_root, paths) = paths();
    fs::create_dir_all(paths.profile.parent().expect("parent")).expect("profile directory");
    let original = b"{\"userField\":\"before-launch\"}\n";
    fs::write(&paths.profile, original).expect("original profile");
    Receipt::capture(&paths)
        .expect("capture")
        .write(&paths.receipt)
        .expect("receipt");
    apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");
    let process = FakeProcess::running(paths.profile.clone());

    let error = complete_and_restore(
        &paths,
        &process,
        Err(ClaudeDesktopError::LaunchFailed(Some(1))),
    )
    .await
    .expect_err("launch error should propagate");

    assert!(matches!(error, ClaudeDesktopError::LaunchFailed(Some(1))));
    assert!(process.terminated.load(Ordering::SeqCst));
    assert!(
        process
            .terminated_while_gateway_active
            .load(Ordering::SeqCst)
    );
    assert_eq!(
        fs::read(&paths.profile).expect("restored profile"),
        original
    );
    assert!(!paths.receipt.exists());
    assert!(!paths.backup_directory.exists());
}

#[tokio::test]
async fn termination_failure_leaves_active_config_and_recovery_state() {
    let (_root, paths) = paths();
    fs::create_dir_all(paths.profile.parent().expect("parent")).expect("profile directory");
    let original = b"{\"userField\":\"original\"}\n";
    fs::write(&paths.profile, original).expect("original profile");
    Receipt::capture(&paths)
        .expect("capture")
        .write(&paths.receipt)
        .expect("receipt");
    apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");
    let active = fs::read(&paths.profile).expect("active profile");
    let process = FakeProcess::running(paths.profile.clone());
    process.fail_terminate.store(true, Ordering::SeqCst);
    process.fail_force_terminate.store(true, Ordering::SeqCst);

    let error = complete_and_restore(&paths, &process, Ok(WaitOutcome::Signaled(143)))
        .await
        .expect_err("unsafe cleanup should fail");

    assert!(matches!(error, ClaudeDesktopError::Terminate(_)));
    assert!(process.force_terminated.load(Ordering::SeqCst));
    assert_eq!(
        fs::read(&paths.profile).expect("profile should remain active"),
        active
    );
    assert!(paths.receipt.exists(), "receipt should remain recoverable");
    assert!(
        paths.backup_directory.exists(),
        "backup should remain recoverable"
    );
}

#[tokio::test]
async fn persistent_process_check_error_does_not_restore_without_confirmation() {
    let (_root, paths) = paths();
    fs::create_dir_all(paths.profile.parent().expect("parent")).expect("profile directory");
    fs::write(&paths.profile, b"{\"userField\":\"original\"}\n").expect("original profile");
    Receipt::capture(&paths)
        .expect("capture")
        .write(&paths.receipt)
        .expect("receipt");
    apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");
    let active = fs::read(&paths.profile).expect("active profile");
    let process = FakeProcess::running(paths.profile.clone());
    process.fail_checks.store(true, Ordering::SeqCst);

    let error = complete_and_restore(
        &paths,
        &process,
        Err(ClaudeDesktopError::ProcessCheck(std::io::Error::other(
            "synthetic wait failure",
        ))),
    )
    .await
    .expect_err("unconfirmed termination should fail");

    assert!(matches!(error, ClaudeDesktopError::ProcessCheck(_)));
    assert!(process.terminated.load(Ordering::SeqCst));
    assert!(process.force_terminated.load(Ordering::SeqCst));
    assert_eq!(
        fs::read(&paths.profile).expect("profile should remain active"),
        active
    );
    assert!(paths.receipt.exists(), "receipt should remain recoverable");
    assert!(
        paths.backup_directory.exists(),
        "backup should remain recoverable"
    );
}
