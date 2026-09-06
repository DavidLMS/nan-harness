//! Interrupted backup and restoration transactions for Claude Desktop.
//!
//! Capture and restoration are ordered, receipt-owned sequences over the four
//! documents in `DesktopPaths::documents`, not atomic transactions. A failure
//! part-way through capture removes the incomplete backup directory and leaves
//! the originals untouched; a failure part-way through restoration keeps the
//! receipt and every backup in place so the next `--restore` can finish the
//! work, accepting the documents already restored. These tests pin that
//! contract with ordinary files and directories: every expectation is written
//! against saved literal bytes rather than against a helper under test.

use super::super::paths::DesktopPaths;
use super::super::{ClaudeDesktopError, RECEIPT_SCHEMA, Receipt, restore_after, restore_receipt};
use super::fixtures::paths as session_paths;
use serde_json::{Value, json};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const NORMAL_ORIGINAL: &[u8] = b"{\"deploymentMode\":\"1p\",\"userNormal\":\"original\"}\n";
const THIRD_PARTY_ORIGINAL: &[u8] = b"{\"userThirdParty\":\"original\"}\n";
const META_ORIGINAL: &[u8] = b"{\"userMeta\":\"original\"}\n";
const PROFILE_ORIGINAL: &[u8] = b"{\"userProfile\":\"original\"}\n";

const NORMAL_ACTIVE: &[u8] = b"{\"deploymentMode\":\"3p\",\"userNormal\":\"active\"}\n";
const THIRD_PARTY_ACTIVE: &[u8] = b"{\"userThirdParty\":\"active\"}\n";
const META_ACTIVE: &[u8] = b"{\"userMeta\":\"active\"}\n";
const PROFILE_ACTIVE: &[u8] = b"{\"userProfile\":\"active\"}\n";

const SENTINEL_BYTES: &[u8] = b"{\"sentinel\":\"user data behind the obstruction\"}\n";
const UNRELATED_BYTES: &[u8] = b"{\"unrelated\":\"neither captured nor restored\"}\n";
const CORRUPT_BACKUP_BYTES: &[u8] = b"{\"userMeta\":\"corrupted backup\"}\n";

/// Index of each document in `DesktopPaths::documents`, which fixes both the
/// capture/restore order and the `document-{index}.backup` file names.
const NORMAL: usize = 0;
const THIRD_PARTY: usize = 1;
const META: usize = 2;
const PROFILE: usize = 3;

fn seed(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().expect("document parent"))
        .unwrap_or_else(|error| panic!("{} directory should exist: {error}", path.display()));
    fs::write(path, bytes)
        .unwrap_or_else(|error| panic!("{} should be written: {error}", path.display()));
}

fn read_file(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()))
}

fn assert_bytes(path: &Path, expected: &[u8], context: &str) {
    assert_eq!(read_file(path), expected, "{context}");
}

/// Asserts absence specifically, so an unrelated metadata failure cannot pass
/// as a safe removal.
fn assert_absent(path: &Path, context: &str) {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Ok(_) => panic!("{} should be absent: {context}", path.display()),
        Err(error) => panic!("{} absence is unverifiable: {error}", path.display()),
    }
}

fn backup(paths: &DesktopPaths, index: usize) -> PathBuf {
    paths
        .backup_directory
        .join(format!("document-{index}.backup"))
}

/// A captured session whose four documents were then overwritten with
/// synthetic active bytes, exactly as an interrupted run would leave them.
fn prepared_session() -> (tempfile::TempDir, DesktopPaths) {
    let (root, paths) = session_paths();
    seed(&paths.normal_config, NORMAL_ORIGINAL);
    seed(&paths.third_party_config, THIRD_PARTY_ORIGINAL);
    seed(&paths.meta, META_ORIGINAL);
    seed(&paths.profile, PROFILE_ORIGINAL);
    Receipt::capture(&paths)
        .expect("capture should succeed for four readable documents")
        .write(&paths.receipt)
        .expect("receipt should be written");
    seed(&paths.normal_config, NORMAL_ACTIVE);
    seed(&paths.third_party_config, THIRD_PARTY_ACTIVE);
    seed(&paths.meta, META_ACTIVE);
    seed(&paths.profile, PROFILE_ACTIVE);
    (root, paths)
}

fn assert_originals_restored(paths: &DesktopPaths) {
    assert_bytes(&paths.normal_config, NORMAL_ORIGINAL, "normal config");
    assert_bytes(
        &paths.third_party_config,
        THIRD_PARTY_ORIGINAL,
        "third-party config",
    );
    assert_bytes(&paths.meta, META_ORIGINAL, "profile meta");
    assert_bytes(&paths.profile, PROFILE_ORIGINAL, "profile");
}

fn assert_active_documents(paths: &DesktopPaths) {
    assert_bytes(&paths.normal_config, NORMAL_ACTIVE, "normal config");
    assert_bytes(
        &paths.third_party_config,
        THIRD_PARTY_ACTIVE,
        "third-party config",
    );
    assert_bytes(&paths.meta, META_ACTIVE, "profile meta");
    assert_bytes(&paths.profile, PROFILE_ACTIVE, "profile");
}

/// Every backup still holds the exact original bytes it captured.
fn assert_backups_authentic(paths: &DesktopPaths) {
    assert_bytes(&backup(paths, NORMAL), NORMAL_ORIGINAL, "normal backup");
    assert_bytes(
        &backup(paths, THIRD_PARTY),
        THIRD_PARTY_ORIGINAL,
        "third-party backup",
    );
    assert_bytes(&backup(paths, META), META_ORIGINAL, "meta backup");
    assert_bytes(&backup(paths, PROFILE), PROFILE_ORIGINAL, "profile backup");
}

fn assert_evidence_cleared(paths: &DesktopPaths) {
    assert_absent(&paths.receipt, "a completed recovery removes its receipt");
    assert_absent(
        &paths.backup_directory,
        "a completed recovery removes its backups",
    );
}

#[test]
fn capture_failure_removes_partial_backups_and_allows_a_clean_retry() {
    let (root, paths) = session_paths();
    let unrelated = root.path().join("unrelated.json");
    seed(&paths.normal_config, NORMAL_ORIGINAL);
    seed(&paths.third_party_config, THIRD_PARTY_ORIGINAL);
    seed(&unrelated, UNRELATED_BYTES);
    // A directory where the third document belongs fails the read after the
    // first two snapshots have already been written.
    fs::create_dir_all(&paths.meta).expect("meta obstruction should be created");
    let sentinel = paths.meta.join("keep-me.json");
    seed(&sentinel, SENTINEL_BYTES);

    let error = Receipt::capture(&paths).expect_err("an unreadable document must fail capture");

    assert!(matches!(error, ClaudeDesktopError::ReadConfig(_)));
    assert_bytes(&paths.normal_config, NORMAL_ORIGINAL, "normal config");
    assert_bytes(
        &paths.third_party_config,
        THIRD_PARTY_ORIGINAL,
        "third-party config",
    );
    assert_bytes(&sentinel, SENTINEL_BYTES, "obstructed user data");
    assert_bytes(&unrelated, UNRELATED_BYTES, "unrelated state");
    assert_absent(
        &paths.backup_directory,
        "an incomplete capture leaves no backups behind",
    );
    assert_absent(&paths.receipt, "a failed capture writes no receipt");

    fs::remove_file(&sentinel).expect("sentinel should be removable");
    fs::remove_dir(&paths.meta).expect("obstruction should be removable");
    seed(&paths.meta, META_ORIGINAL);

    Receipt::capture(&paths)
        .expect("capture should succeed once the document is readable")
        .write(&paths.receipt)
        .expect("receipt should be written");
    seed(&paths.normal_config, NORMAL_ACTIVE);
    seed(&paths.meta, META_ACTIVE);
    restore_receipt(&paths).expect("the retried session should restore");

    assert_bytes(&paths.normal_config, NORMAL_ORIGINAL, "normal config");
    assert_bytes(
        &paths.third_party_config,
        THIRD_PARTY_ORIGINAL,
        "third-party config",
    );
    assert_bytes(&paths.meta, META_ORIGINAL, "profile meta");
    assert_absent(
        &paths.profile,
        "a document that never existed stays removed after restoration",
    );
    assert_bytes(&unrelated, UNRELATED_BYTES, "unrelated state");
    assert_evidence_cleared(&paths);
}

#[test]
fn corrupted_backup_keeps_partial_restoration_recoverable() {
    let (_root, paths) = prepared_session();
    let receipt_bytes = read_file(&paths.receipt);
    let authentic_meta_backup = read_file(&backup(&paths, META));
    fs::write(backup(&paths, META), CORRUPT_BACKUP_BYTES).expect("backup should be corrupted");

    let error = restore_receipt(&paths).expect_err("a mismatched backup must not be restored");

    assert!(matches!(error, ClaudeDesktopError::BackupHashMismatch));
    assert_bytes(&paths.normal_config, NORMAL_ORIGINAL, "normal config");
    assert_bytes(
        &paths.third_party_config,
        THIRD_PARTY_ORIGINAL,
        "third-party config",
    );
    assert_bytes(&paths.meta, META_ACTIVE, "profile meta");
    assert_bytes(&paths.profile, PROFILE_ACTIVE, "profile");
    assert_bytes(&paths.receipt, &receipt_bytes, "receipt");
    assert_bytes(&backup(&paths, NORMAL), NORMAL_ORIGINAL, "normal backup");
    assert_bytes(
        &backup(&paths, THIRD_PARTY),
        THIRD_PARTY_ORIGINAL,
        "third-party backup",
    );
    assert_bytes(
        &backup(&paths, META),
        CORRUPT_BACKUP_BYTES,
        "corrupted backup is left for inspection",
    );
    assert_bytes(&backup(&paths, PROFILE), PROFILE_ORIGINAL, "profile backup");

    fs::write(backup(&paths, META), &authentic_meta_backup).expect("backup should be repaired");
    restore_receipt(&paths).expect("the repaired session should restore");

    assert_originals_restored(&paths);
    assert_evidence_cleared(&paths);
    assert!(matches!(
        restore_receipt(&paths),
        Err(ClaudeDesktopError::NoReceipt)
    ));
}

#[test]
fn missing_backup_is_a_read_failure_that_keeps_the_remaining_evidence() {
    let (_root, paths) = prepared_session();
    let receipt_bytes = read_file(&paths.receipt);
    let authentic_profile_backup = read_file(&backup(&paths, PROFILE));
    fs::remove_file(backup(&paths, PROFILE)).expect("backup should be removable");

    let error = restore_receipt(&paths).expect_err("a missing backup must not report success");

    assert!(matches!(error, ClaudeDesktopError::ReadBackup(_)));
    assert_bytes(&paths.normal_config, NORMAL_ORIGINAL, "normal config");
    assert_bytes(
        &paths.third_party_config,
        THIRD_PARTY_ORIGINAL,
        "third-party config",
    );
    assert_bytes(&paths.meta, META_ORIGINAL, "profile meta");
    assert_bytes(&paths.profile, PROFILE_ACTIVE, "profile");
    assert_bytes(&paths.receipt, &receipt_bytes, "receipt");
    assert_bytes(&backup(&paths, NORMAL), NORMAL_ORIGINAL, "normal backup");
    assert_bytes(
        &backup(&paths, THIRD_PARTY),
        THIRD_PARTY_ORIGINAL,
        "third-party backup",
    );
    assert_bytes(&backup(&paths, META), META_ORIGINAL, "meta backup");
    assert_absent(
        &backup(&paths, PROFILE),
        "a failed restoration invents no replacement backup",
    );

    fs::write(backup(&paths, PROFILE), &authentic_profile_backup)
        .expect("backup should be restored");
    restore_receipt(&paths).expect("the completed session should restore");

    assert_originals_restored(&paths);
    assert_evidence_cleared(&paths);
}

#[test]
fn obstructed_absent_document_stays_recoverable_without_touching_user_data() {
    let (_root, paths) = session_paths();
    seed(&paths.normal_config, NORMAL_ORIGINAL);
    seed(&paths.third_party_config, THIRD_PARTY_ORIGINAL);
    Receipt::capture(&paths)
        .expect("capture should record the two absent documents")
        .write(&paths.receipt)
        .expect("receipt should be written");
    let receipt_bytes = read_file(&paths.receipt);
    seed(&paths.normal_config, NORMAL_ACTIVE);
    seed(&paths.third_party_config, THIRD_PARTY_ACTIVE);
    seed(&paths.profile, PROFILE_ACTIVE);
    fs::create_dir_all(&paths.meta).expect("meta obstruction should be created");
    let sentinel = paths.meta.join("keep-me.json");
    seed(&sentinel, SENTINEL_BYTES);

    let error =
        restore_receipt(&paths).expect_err("an unremovable document must not report success");

    assert!(matches!(error, ClaudeDesktopError::Restore(_)));
    assert_bytes(&paths.normal_config, NORMAL_ORIGINAL, "normal config");
    assert_bytes(
        &paths.third_party_config,
        THIRD_PARTY_ORIGINAL,
        "third-party config",
    );
    assert_bytes(&sentinel, SENTINEL_BYTES, "obstructed user data");
    assert_bytes(&paths.profile, PROFILE_ACTIVE, "profile");
    assert_bytes(&paths.receipt, &receipt_bytes, "receipt");
    assert_bytes(&backup(&paths, NORMAL), NORMAL_ORIGINAL, "normal backup");
    assert_bytes(
        &backup(&paths, THIRD_PARTY),
        THIRD_PARTY_ORIGINAL,
        "third-party backup",
    );
    assert_absent(
        &backup(&paths, META),
        "an absent document is captured without a backup",
    );
    assert_absent(
        &backup(&paths, PROFILE),
        "an absent document is captured without a backup",
    );

    fs::remove_file(&sentinel).expect("sentinel should be removable");
    fs::remove_dir(&paths.meta).expect("obstruction should be removable");
    restore_receipt(&paths).expect("the unobstructed session should restore");

    assert_bytes(&paths.normal_config, NORMAL_ORIGINAL, "normal config");
    assert_bytes(
        &paths.third_party_config,
        THIRD_PARTY_ORIGINAL,
        "third-party config",
    );
    assert_absent(
        &paths.meta,
        "a document that never existed is not recreated",
    );
    assert_absent(&paths.profile, "a document that never existed is removed");
    assert_evidence_cleared(&paths);
}

#[test]
fn recovery_failure_outranks_the_reported_operation_error() {
    let (_root, paths) = prepared_session();
    let receipt_bytes = read_file(&paths.receipt);
    let authentic_profile_backup = read_file(&backup(&paths, PROFILE));
    fs::write(backup(&paths, PROFILE), CORRUPT_BACKUP_BYTES).expect("backup should be corrupted");

    let error = restore_after(&paths, Err(ClaudeDesktopError::ConfigRoot))
        .expect_err("a failed recovery must be reported");

    assert!(
        matches!(error, ClaudeDesktopError::BackupHashMismatch),
        "the unrestored configuration outranks the operation error"
    );
    assert_bytes(&paths.normal_config, NORMAL_ORIGINAL, "normal config");
    assert_bytes(
        &paths.third_party_config,
        THIRD_PARTY_ORIGINAL,
        "third-party config",
    );
    assert_bytes(&paths.meta, META_ORIGINAL, "profile meta");
    assert_bytes(&paths.profile, PROFILE_ACTIVE, "profile");
    assert_bytes(&paths.receipt, &receipt_bytes, "receipt");
    assert_bytes(
        &backup(&paths, PROFILE),
        CORRUPT_BACKUP_BYTES,
        "corrupted backup is left for inspection",
    );

    fs::write(backup(&paths, PROFILE), &authentic_profile_backup)
        .expect("backup should be repaired");

    assert_eq!(
        restore_after(&paths, Ok(7)).expect("the repaired session should restore"),
        7
    );
    assert_originals_restored(&paths);
    assert_evidence_cleared(&paths);
}

/// A named mutation applied to an otherwise valid serialized receipt.
type ReceiptMutation = (&'static str, fn(&mut Value));

/// Receipt validation is a top-level gate: a rejected receipt must leave every
/// target, backup and receipt byte exactly as it found them.
#[test]
fn invalid_receipts_are_rejected_before_any_document_is_touched() {
    let cases: [ReceiptMutation; 3] = [
        ("unsupported schema", |receipt| {
            receipt["schema"] = json!(RECEIPT_SCHEMA + 1);
        }),
        ("missing snapshot", |receipt| {
            receipt["snapshots"]
                .as_array_mut()
                .expect("snapshots array")
                .truncate(3);
        }),
        ("reordered documents", |receipt| {
            receipt["snapshots"]
                .as_array_mut()
                .expect("snapshots array")
                .swap(NORMAL, META);
        }),
    ];

    for (label, mutate) in cases {
        let (_root, paths) = prepared_session();
        let mut receipt: Value =
            serde_json::from_slice(&read_file(&paths.receipt)).expect("receipt should parse");
        mutate(&mut receipt);
        let mutated = serde_json::to_vec(&receipt).expect("mutated receipt should serialize");
        fs::write(&paths.receipt, &mutated).expect("mutated receipt should be written");

        let error = restore_receipt(&paths).expect_err("an invalid receipt must be rejected");

        assert!(
            matches!(error, ClaudeDesktopError::UnsupportedReceipt),
            "{label} should report an unsupported receipt"
        );
        assert_bytes(&paths.receipt, &mutated, label);
        assert_active_documents(&paths);
        assert_backups_authentic(&paths);
    }
}
