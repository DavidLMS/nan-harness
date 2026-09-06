use super::*;

const ORIGINAL_ENV: &str = "USER_SETTING=before\n";
const ORIGINAL_ACTIVE: &[u8] = b"{\"profile\":\"work\"}\n";
const SESSION_KEY: &str = "session-secret";
const ACTIVE_BACKUP: &str = "active-profile.backup";
const ENV_BACKUP: &str = "profile-env.backup";

/// The recovery inputs and outputs an interrupted restore must not disturb.
#[derive(Debug, PartialEq, Eq)]
struct RecoveryState {
    active_profile: Option<Vec<u8>>,
    environment: Option<Vec<u8>>,
    receipt: Option<Vec<u8>>,
    active_backup: Option<Vec<u8>>,
    environment_backup: Option<Vec<u8>>,
}

fn managed_env(paths: &DesktopPaths) -> PathBuf {
    paths.managed_profile.join(".env")
}

fn backup(paths: &DesktopPaths, name: &str) -> PathBuf {
    paths.backup_directory.join(name)
}

fn snapshot(paths: &DesktopPaths) -> RecoveryState {
    let read = |path: PathBuf| fs::read(path).ok();
    RecoveryState {
        active_profile: read(paths.active_profile.clone()),
        environment: read(managed_env(paths)),
        receipt: read(paths.session_receipt.clone()),
        active_backup: read(backup(paths, ACTIVE_BACKUP)),
        environment_backup: read(backup(paths, ENV_BACKUP)),
    }
}

/// Begins a real Persistent session over a synthetic profile that already has a
/// known `.env` and active selection.
fn persistent_session() -> (tempfile::TempDir, DesktopPaths) {
    let (root, paths) = paths();
    fs::create_dir_all(&paths.managed_profile).expect("managed profile");
    fs::create_dir_all(paths.active_profile.parent().expect("active parent"))
        .expect("active parent");
    fs::write(managed_env(&paths), ORIGINAL_ENV).expect("original env");
    fs::write(&paths.active_profile, ORIGINAL_ACTIVE).expect("original active");
    begin_session(
        &paths,
        &paths.managed_profile,
        SessionMode::Persistent,
        SESSION_KEY,
    )
    .expect("session setup");
    (root, paths)
}

fn saved_receipt(paths: &DesktopPaths) -> SessionReceipt {
    read_optional_json::<SessionReceipt>(&paths.session_receipt)
        .expect("receipt read")
        .expect("receipt")
}

/// Rewrites the saved receipt through `tamper`, then requires the following
/// restore to fail without changing any file it could have redirected.
fn reject_tampered_receipt(
    paths: &DesktopPaths,
    tamper: impl FnOnce(&mut SessionReceipt),
) -> HermesDesktopError {
    let mut receipt = saved_receipt(paths);
    tamper(&mut receipt);
    write_json_private(&paths.session_receipt, &receipt).expect("tampered receipt");
    let before = snapshot(paths);

    let error = restore_session(paths).expect_err("an invalid receipt must not drive recovery");

    assert_eq!(snapshot(paths), before);
    error
}

/// Requires `edited` to be refused as a changed managed credential, leaving the
/// receipt and both backups available for a later retry.
fn refuse_changed_credential(paths: &DesktopPaths, edited: &str) {
    fs::write(managed_env(paths), edited).expect("edited env");

    let error = restore_session(paths).expect_err("a changed credential must fail closed");

    assert!(matches!(
        error,
        HermesDesktopError::ManagedCredentialChanged
    ));
    assert_eq!(
        fs::read_to_string(managed_env(paths)).expect("edited env preserved"),
        edited
    );
    assert!(paths.session_receipt.exists());
    assert!(backup(paths, ACTIVE_BACKUP).exists());
    assert!(backup(paths, ENV_BACKUP).exists());
    // The active selection is restored before the environment step, so it is
    // already back to its original bytes when this failure is reported.
    assert_eq!(
        fs::read(&paths.active_profile).expect("active selection"),
        ORIGINAL_ACTIVE
    );
}

#[test]
fn restore_removes_only_the_owned_block_from_an_edited_environment() {
    let (_root, paths) = persistent_session();
    let applied = fs::read_to_string(managed_env(&paths)).expect("applied env");
    assert!(applied.contains(SESSION_KEY));
    fs::write(
        managed_env(&paths),
        format!("EDITOR_SETTING=1\n{applied}TRAILING_SETTING=2\n"),
    )
    .expect("unrelated user edits around the owned block");
    fs::write(&paths.active_profile, b"{\"profile\":\"user-choice\"}\n").expect("user switch");

    restore_session(&paths).expect("restore");

    assert_eq!(
        fs::read_to_string(managed_env(&paths)).expect("restored env"),
        "EDITOR_SETTING=1\nUSER_SETTING=before\nTRAILING_SETTING=2\n"
    );
    assert_eq!(
        fs::read(&paths.active_profile).expect("preserved active selection"),
        b"{\"profile\":\"user-choice\"}\n"
    );
    assert!(!paths.session_receipt.exists());
    assert!(!paths.backup_directory.exists());
}

#[test]
fn a_truncated_owned_block_fails_closed_and_recovers_after_repair() {
    let (_root, paths) = persistent_session();
    let applied = fs::read_to_string(managed_env(&paths)).expect("applied env");
    let truncated = applied.replace(ENV_BLOCK_END, "# unrelated comment");

    refuse_changed_credential(&paths, &truncated);

    fs::write(managed_env(&paths), "USER_SETTING=repaired\n").expect("repaired env");
    restore_session(&paths).expect("recovery resumes once the block is gone");

    assert_eq!(
        fs::read_to_string(managed_env(&paths)).expect("repaired env preserved"),
        "USER_SETTING=repaired\n"
    );
    assert!(!paths.session_receipt.exists());
    assert!(!paths.backup_directory.exists());
}

#[test]
fn an_unmarked_credential_fails_closed_and_recovers_after_repair() {
    let (_root, paths) = persistent_session();

    refuse_changed_credential(&paths, "USER_SETTING=before\nNAN_API_KEY=hand-written\n");

    fs::write(managed_env(&paths), ORIGINAL_ENV).expect("repaired env");
    restore_session(&paths).expect("recovery resumes once the credential is gone");

    assert_eq!(
        fs::read_to_string(managed_env(&paths)).expect("restored env"),
        ORIGINAL_ENV
    );
    assert!(!paths.session_receipt.exists());
}

#[test]
fn an_unsupported_receipt_schema_is_rejected_before_any_mutation() {
    let (_root, paths) = persistent_session();

    let error = reject_tampered_receipt(&paths, |receipt| {
        receipt.schema_version = SESSION_SCHEMA_VERSION + 1;
    });

    assert!(matches!(
        error,
        HermesDesktopError::UnsupportedSessionSchema
    ));
}

#[test]
fn a_redirected_profile_path_is_rejected_and_left_untouched() {
    let (root, paths) = persistent_session();
    let decoy = root.path().join("decoy-profile");
    fs::create_dir_all(&decoy).expect("decoy profile");
    fs::write(decoy.join(".env"), "USER_SECRET=decoy\n").expect("decoy sentinel");

    let error = reject_tampered_receipt(&paths, |receipt| receipt.profile = decoy.clone());

    assert!(matches!(error, HermesDesktopError::InvalidRecoveryReceipt));
    assert_eq!(
        fs::read_to_string(decoy.join(".env")).expect("decoy preserved"),
        "USER_SECRET=decoy\n"
    );
}

#[test]
fn a_redirected_active_backup_name_is_rejected_and_left_untouched() {
    let (_root, paths) = persistent_session();
    let decoy = backup(&paths, "decoy.backup");
    fs::write(&decoy, b"decoy backup").expect("decoy backup");

    let error = reject_tampered_receipt(&paths, |receipt| {
        receipt.active_profile.backup_file = "decoy.backup".to_owned();
    });

    assert!(matches!(error, HermesDesktopError::InvalidRecoveryReceipt));
    assert_eq!(fs::read(&decoy).expect("decoy preserved"), b"decoy backup");
}

#[test]
fn a_redirected_environment_backup_name_is_rejected_and_left_untouched() {
    let (_root, paths) = persistent_session();
    let decoy = backup(&paths, "decoy.backup");
    fs::write(&decoy, b"decoy backup").expect("decoy backup");

    let error = reject_tampered_receipt(&paths, |receipt| {
        receipt.environment.backup_file = "decoy.backup".to_owned();
    });

    assert!(matches!(error, HermesDesktopError::InvalidRecoveryReceipt));
    assert_eq!(fs::read(&decoy).expect("decoy preserved"), b"decoy backup");
}

#[test]
fn a_corrupted_active_backup_is_refused_before_anything_is_written() {
    let (_root, paths) = persistent_session();
    let before = snapshot(&paths);
    fs::write(backup(&paths, ACTIVE_BACKUP), b"corrupted backup").expect("corrupt active backup");

    let error = restore_session(&paths).expect_err("a corrupted backup must not be applied");

    assert!(matches!(error, HermesDesktopError::BackupHashMismatch));
    assert_eq!(
        fs::read(&paths.active_profile).expect("active selection"),
        before.active_profile.expect("applied active selection")
    );
    assert_eq!(
        fs::read(managed_env(&paths)).expect("environment"),
        before.environment.expect("applied environment")
    );
    assert!(paths.session_receipt.exists());
}

#[test]
fn a_corrupted_environment_backup_leaves_a_restartable_partial_restoration() {
    let (_root, paths) = persistent_session();
    let authentic = fs::read(backup(&paths, ENV_BACKUP)).expect("authentic env backup");
    let applied = fs::read(managed_env(&paths)).expect("applied env");
    fs::write(backup(&paths, ENV_BACKUP), b"corrupted backup").expect("corrupt env backup");

    let error = restore_session(&paths).expect_err("a corrupted backup must not be applied");

    assert!(matches!(error, HermesDesktopError::BackupHashMismatch));
    // The active selection is restored first, so a partial restoration is the
    // expected state here; the environment and the receipt must survive it.
    assert_eq!(
        fs::read(&paths.active_profile).expect("active selection"),
        ORIGINAL_ACTIVE
    );
    assert_eq!(fs::read(managed_env(&paths)).expect("environment"), applied);
    assert!(paths.session_receipt.exists());

    fs::write(backup(&paths, ENV_BACKUP), &authentic).expect("authentic backup restored");
    restore_session(&paths).expect("recovery resumes from the authentic backup");

    assert_eq!(
        fs::read_to_string(managed_env(&paths)).expect("restored env"),
        ORIGINAL_ENV
    );
    assert!(!paths.session_receipt.exists());
    assert!(!paths.backup_directory.exists());
}

#[test]
fn a_missing_backup_is_reported_instead_of_guessing_the_original() {
    let (_root, paths) = persistent_session();
    fs::remove_file(backup(&paths, ENV_BACKUP)).expect("remove env backup");

    let error = restore_session(&paths).expect_err("a missing backup must be reported");

    assert!(matches!(error, HermesDesktopError::ReadBackup(_)));
    assert!(paths.session_receipt.exists());
}

#[test]
fn a_failed_backup_directory_cleanup_keeps_a_usable_retry_receipt() {
    let (_root, paths) = persistent_session();
    let sentinel = paths.backup_directory.join("unrelated-file");
    fs::write(&sentinel, b"unrelated").expect("unrelated sentinel");

    let error = restore_session(&paths).expect_err("a non-empty backup directory is reported");

    assert!(matches!(error, HermesDesktopError::RemoveBackup(_)));
    assert_eq!(
        fs::read_to_string(managed_env(&paths)).expect("restored env"),
        ORIGINAL_ENV
    );
    assert_eq!(
        fs::read(&paths.active_profile).expect("restored active selection"),
        ORIGINAL_ACTIVE
    );
    assert_eq!(
        fs::read(&sentinel).expect("sentinel preserved"),
        b"unrelated"
    );
    assert!(!backup(&paths, ACTIVE_BACKUP).exists());
    assert!(!backup(&paths, ENV_BACKUP).exists());
    assert!(paths.session_receipt.exists());

    fs::remove_file(&sentinel).expect("remove sentinel");
    restore_session(&paths).expect("cleanup completes without the deleted backups");

    assert!(!paths.backup_directory.exists());
    assert!(!paths.session_receipt.exists());
}
