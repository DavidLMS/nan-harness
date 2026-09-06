use super::super::documents::{patched_auth_document, patched_models_document};
use super::{
    PenDesktopError, PenPaths, begin_session, read_json_object, restore_after, restore_session,
};
use nan_harness_core::CodingModelProfile;
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Placeholder credential owned by the simulated user, only ever present in the
/// original auth document and its private backup.
const USER_KEY: &str = "synthetic-user-key";
/// Placeholder credential the managed session applies while Pen is open.
const SESSION_TOKEN: &str = "synthetic-session-token";

struct SessionFixture {
    _root: TempDir,
    paths: PenPaths,
    original_models: Vec<u8>,
    original_auth: Vec<u8>,
    applied_models: Vec<u8>,
    applied_auth: Vec<u8>,
    receipt: Vec<u8>,
}

impl SessionFixture {
    fn backup_path(&self, file: &str) -> std::path::PathBuf {
        self.paths.session_backup_directory.join(file)
    }

    fn assert_evidence_retained(&self) {
        assert_eq!(read(&self.paths.session_receipt), self.receipt);
        assert_eq!(
            read(&self.backup_path("models.backup")),
            self.original_models
        );
        assert_eq!(read(&self.backup_path("auth.backup")), self.original_auth);
    }

    fn assert_originals_restored_and_cleaned_up(&self) {
        assert_eq!(read(&self.paths.models), self.original_models);
        assert_eq!(read(&self.paths.auth), self.original_auth);
        assert!(!self.paths.session_receipt.exists());
        assert!(!self.paths.session_backup_directory.exists());
    }
}

fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()))
}

/// Starts a real managed session over two seeded original documents and keeps
/// the exact original and applied bytes for later assertions.
fn begin_prepared_session() -> SessionFixture {
    let root = tempfile::tempdir().expect("a temporary Pen root should be created");
    let paths = PenPaths::new(&root.path().join("home"), &root.path().join("state"))
        .expect("explicit temporary Pen paths should be valid");
    fs::create_dir_all(paths.models.parent().expect("a pencil directory"))
        .expect("the pencil directory should be created");
    let original_models = b"{\"providers\":{\"other\":{\"name\":\"Other\"}},\"theme\":\"user\"}\n";
    let original_auth = format!("{{\"other\":{{\"type\":\"api_key\",\"key\":\"{USER_KEY}\"}}}}\n");
    fs::write(&paths.models, original_models).expect("the original models file should be written");
    fs::write(&paths.auth, &original_auth).expect("the original auth file should be written");

    let mut profile = CodingModelProfile::generic("qwen3.6");
    profile.image_input = true;
    let applied_models = patched_models_document(
        read_json_object(&paths.models).expect("the original models file should parse"),
        "http://127.0.0.1:1234/v1",
        &[profile],
    )
    .expect("the managed models document should be built");
    let applied_auth = patched_auth_document(
        read_json_object(&paths.auth).expect("the original auth file should parse"),
        SESSION_TOKEN,
    )
    .expect("the managed auth document should be built");

    begin_session(&paths, &applied_models, &applied_auth).expect("the session should begin");
    assert_eq!(read(&paths.models), applied_models);
    assert_eq!(read(&paths.auth), applied_auth);

    let receipt = read(&paths.session_receipt);
    SessionFixture {
        _root: root,
        paths,
        original_models: original_models.to_vec(),
        original_auth: original_auth.into_bytes(),
        applied_models,
        applied_auth,
        receipt,
    }
}

/// Rewrites the managed NaN auth entry the way a user editing credentials while
/// Pen is open would, leaving valid JSON behind.
fn user_edited_auth(fixture: &SessionFixture) -> Vec<u8> {
    let mut document: Value =
        serde_json::from_slice(&fixture.applied_auth).expect("the applied auth should be JSON");
    document["nan"]["key"] = Value::String("user-rotated-key".to_owned());
    let mut bytes = serde_json::to_vec_pretty(&document).expect("the edited auth should serialize");
    bytes.push(b'\n');
    bytes
}

#[test]
fn managed_auth_conflict_keeps_recoverable_evidence_and_a_retry_completes_restoration() {
    let fixture = begin_prepared_session();
    assert!(
        String::from_utf8_lossy(&fixture.applied_auth).contains(SESSION_TOKEN)
            && String::from_utf8_lossy(&fixture.original_auth).contains(USER_KEY),
        "the fixture must actually place both placeholder credentials in the auth documents"
    );
    let receipt = fs::read_to_string(&fixture.paths.session_receipt).expect("a readable receipt");
    assert!(!receipt.contains(SESSION_TOKEN) && !receipt.contains(USER_KEY));

    let edited_auth = user_edited_auth(&fixture);
    fs::write(&fixture.paths.auth, &edited_auth).expect("the edited auth should be written");
    match restore_session(&fixture.paths) {
        Err(PenDesktopError::ManagedConfigurationChanged(path)) => {
            assert_eq!(path, fixture.paths.auth);
        }
        other => panic!("expected a managed auth conflict, got {other:?}"),
    }
    // Restoration is sequential: models is restored before auth is inspected,
    // and the refused auth edit is left exactly as the user wrote it.
    assert_eq!(read(&fixture.paths.models), fixture.original_models);
    assert_eq!(read(&fixture.paths.auth), edited_auth);
    fixture.assert_evidence_retained();

    fs::write(&fixture.paths.auth, &fixture.applied_auth)
        .expect("the user should be able to put the managed auth document back");
    assert!(restore_session(&fixture.paths).expect("the retry should restore both documents"));
    fixture.assert_originals_restored_and_cleaned_up();
    assert!(
        !restore_session(&fixture.paths).expect("a settled session should be inspectable"),
        "a completed restoration must not report further pending work"
    );
}

#[test]
fn corrupted_auth_backup_is_refused_without_overwriting_the_auth_target() {
    let fixture = begin_prepared_session();
    let backup_path = fixture.backup_path("auth.backup");
    let authentic_backup = read(&backup_path);
    fs::write(&backup_path, b"{\"other\":{\"type\":\"api_key\"}}\n")
        .expect("the auth backup should be corruptible");

    assert!(matches!(
        restore_session(&fixture.paths),
        Err(PenDesktopError::BackupHashMismatch)
    ));
    assert_eq!(read(&fixture.paths.models), fixture.original_models);
    assert_eq!(
        read(&fixture.paths.auth),
        fixture.applied_auth,
        "an untrusted backup must never replace the live auth document"
    );
    assert_eq!(read(&fixture.paths.session_receipt), fixture.receipt);
    assert_eq!(
        read(&fixture.backup_path("models.backup")),
        fixture.original_models
    );

    fs::write(&backup_path, &authentic_backup).expect("the authentic backup should be restorable");
    assert!(restore_session(&fixture.paths).expect("the repaired retry should succeed"));
    fixture.assert_originals_restored_and_cleaned_up();
}

#[test]
fn a_deleted_models_target_fails_closed_instead_of_being_recreated() {
    let fixture = begin_prepared_session();
    fs::remove_file(&fixture.paths.models).expect("the managed models target should be removable");

    match restore_session(&fixture.paths) {
        Err(PenDesktopError::ManagedConfigurationChanged(path)) => {
            assert_eq!(path, fixture.paths.models);
        }
        other => panic!("expected a managed models conflict, got {other:?}"),
    }
    assert!(
        !fixture.paths.models.exists(),
        "a user deletion differs from an originally absent file and must not be undone silently"
    );
    assert_eq!(read(&fixture.paths.auth), fixture.applied_auth);
    fixture.assert_evidence_retained();

    fs::write(&fixture.paths.models, &fixture.applied_models)
        .expect("the user should be able to put the managed models document back");
    assert!(restore_session(&fixture.paths).expect("the retry should restore both documents"));
    fixture.assert_originals_restored_and_cleaned_up();
}

#[test]
fn a_successful_restoration_preserves_the_supplied_exit_code_and_operation_error() {
    let exited = begin_prepared_session();
    assert_eq!(
        restore_after(&exited.paths, Ok(17)).expect("a successful restoration keeps the exit code"),
        17
    );
    exited.assert_originals_restored_and_cleaned_up();

    let failed = begin_prepared_session();
    match restore_after(&failed.paths, Err(PenDesktopError::DidNotStart)) {
        Err(PenDesktopError::DidNotStart) => {}
        other => panic!("expected the original operation error, got {other:?}"),
    }
    failed.assert_originals_restored_and_cleaned_up();
}

#[test]
fn a_failed_restoration_reports_the_restoration_error_and_stays_retryable() {
    let fixture = begin_prepared_session();
    let backup_path = fixture.backup_path("auth.backup");
    let authentic_backup = read(&backup_path);
    fs::write(&backup_path, b"{}\n").expect("the auth backup should be corruptible");

    match restore_after(&fixture.paths, Err(PenDesktopError::AlreadyRunning)) {
        Err(PenDesktopError::BackupHashMismatch) => {}
        other => panic!("expected the restoration error to take precedence, got {other:?}"),
    }
    assert_eq!(read(&fixture.paths.auth), fixture.applied_auth);
    assert_eq!(read(&fixture.paths.models), fixture.original_models);
    assert_eq!(read(&fixture.paths.session_receipt), fixture.receipt);
    assert_eq!(read(&backup_path), b"{}\n");
    assert_eq!(
        read(&fixture.backup_path("models.backup")),
        fixture.original_models
    );

    fs::write(&backup_path, &authentic_backup).expect("the authentic backup should be restorable");
    assert!(restore_session(&fixture.paths).expect("the repaired retry should succeed"));
    fixture.assert_originals_restored_and_cleaned_up();
}

#[test]
fn a_receipt_naming_an_unexpected_backup_file_is_refused_before_any_target_changes() {
    let mut fixture = begin_prepared_session();
    let mut receipt: Value =
        serde_json::from_slice(&read(&fixture.paths.session_receipt)).expect("a JSON receipt");
    receipt["models"]["backupFile"] = Value::String("models.backup.old".to_owned());
    fs::write(
        &fixture.paths.session_receipt,
        serde_json::to_vec(&receipt).expect("the edited receipt should serialize"),
    )
    .expect("the receipt should be writable");
    fixture.receipt = read(&fixture.paths.session_receipt);

    assert!(matches!(
        restore_session(&fixture.paths),
        Err(PenDesktopError::InvalidReceipt)
    ));
    assert_eq!(read(&fixture.paths.models), fixture.applied_models);
    assert_eq!(read(&fixture.paths.auth), fixture.applied_auth);
    fixture.assert_evidence_retained();
}
