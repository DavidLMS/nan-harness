use super::super::{
    ClaudeDesktopError, Receipt, WaitOutcome, apply_gateway, complete_and_restore,
    prepare_session_lock, restore_after, wait_for_exit_or_signal,
};
use super::fixtures::{FakeProcess, paths};
use std::fs;
use std::sync::atomic::Ordering;

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
