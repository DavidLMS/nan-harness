use super::super::{
    ClaudeDesktopError, Receipt, SessionLock, apply_gateway, ensure_no_pending_recovery,
    restore_receipt,
};
use super::fixtures::paths;
use serde_json::Value;
use std::fs;

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
