use super::ensure_no_pending_desktop_session;
use crate::commands::uninstall::UninstallError;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs as unix_fs;

#[test]
fn empty_state_does_not_block_uninstall() {
    let directory = TempDir::new().expect("temporary directory should exist");

    ensure_no_pending_desktop_session(directory.path())
        .expect("no desktop recovery state should allow uninstall");
}

#[test]
fn unrelated_backup_profile_does_not_block_uninstall() {
    let directory = TempDir::new().expect("temporary directory should exist");
    let unrelated_profile = directory.path().join("backup/profile.json");
    fs::create_dir_all(
        unrelated_profile
            .parent()
            .expect("profile parent should exist"),
    )
    .expect("unrelated profile parent should be created");
    fs::write(&unrelated_profile, b"unrelated profile")
        .expect("unrelated profile should be written");

    ensure_no_pending_desktop_session(directory.path())
        .expect("an unrelated backup profile should allow uninstall");
}

#[test]
fn chatgpt_recovery_receipt_blocks_uninstall() {
    assert_receipt_blocks_uninstall(
        "chatgpt-desktop/profile/.nan-session.json",
        "ChatGPT Desktop",
        b"chatgpt recovery receipt",
    );
}

#[test]
fn claude_recovery_receipt_blocks_uninstall() {
    assert_receipt_blocks_uninstall(
        "claude-desktop-receipt.json",
        "Claude Desktop",
        b"claude recovery receipt",
    );
}

#[test]
fn hermes_recovery_receipt_blocks_uninstall() {
    assert_receipt_blocks_uninstall(
        "hermes-desktop/session.json",
        "Hermes Desktop",
        b"hermes recovery receipt",
    );
}

#[test]
fn pen_recovery_receipt_blocks_uninstall() {
    assert_receipt_blocks_uninstall(
        "pen-desktop/session.json",
        "Pen Desktop",
        b"pen recovery receipt",
    );
}

#[cfg(unix)]
#[test]
fn dangling_receipt_symlink_blocks_uninstall() {
    let directory = TempDir::new().expect("temporary directory should exist");
    let receipt = directory.path().join("claude-desktop-receipt.json");
    let missing_target = directory.path().join("missing-recovery-target");
    unix_fs::symlink(&missing_target, &receipt)
        .expect("dangling receipt symlink should be created");

    let error = ensure_no_pending_desktop_session(directory.path())
        .expect_err("a dangling recovery symlink should block uninstall");
    assert!(matches!(
        error,
        UninstallError::DesktopRecoveryRequired("Claude Desktop")
    ));

    let link = fs::symlink_metadata(&receipt)
        .expect("receipt symlink should still exist")
        .file_type();
    assert!(link.is_symlink());
    assert_eq!(
        fs::read_link(&receipt).expect("receipt should remain a symlink"),
        missing_target
    );
    assert!(!missing_target.exists());
}

fn assert_receipt_blocks_uninstall(relative: &str, surface: &'static str, receipt: &[u8]) {
    let directory = TempDir::new().expect("temporary directory should exist");
    let receipt_path = directory.path().join(relative);
    create_parent(&receipt_path);
    fs::write(&receipt_path, receipt).expect("receipt should be written");

    let error = ensure_no_pending_desktop_session(directory.path())
        .expect_err("pending desktop recovery state should block uninstall");
    assert!(matches!(
        error,
        UninstallError::DesktopRecoveryRequired(expected) if expected == surface
    ));
    assert_eq!(
        fs::read(&receipt_path).expect("receipt should remain readable"),
        receipt
    );
}

fn create_parent(receipt: &Path) {
    fs::create_dir_all(receipt.parent().expect("receipt parent should exist"))
        .expect("receipt parent should be created");
}
