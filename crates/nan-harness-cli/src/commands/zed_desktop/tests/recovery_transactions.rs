use super::super::ZedDesktopError;
use super::super::session::{begin_session_for_test, restore_session};
use super::fixtures::{
    GATEWAY_URL, fixture_paths, generic_model, mutate_managed_field, write_settings,
};
use std::fs;

fn started_session() -> super::fixtures::FixturePaths {
    let fixture = fixture_paths();
    write_settings(
        &fixture.paths,
        br#"{"theme":"before"}
"#,
    );
    begin_session_for_test(
        &fixture.paths,
        GATEWAY_URL,
        &[generic_model()],
        "qwen3.6",
        false,
    )
    .expect("session should begin");
    fixture
}

#[test]
fn managed_edit_fails_closed_and_keeps_recovery_transaction() {
    let fixture = started_session();
    mutate_managed_field(&fixture.paths.settings, "provider");

    let error = restore_session(&fixture.paths).expect_err("managed edits must fail closed");

    assert!(matches!(
        error,
        ZedDesktopError::ManagedConfigurationChanged
    ));
    assert!(fixture.paths.settings.exists());
    assert!(fixture.paths.session_receipt.exists());
    assert!(
        fixture
            .paths
            .backup_directory
            .join("settings.backup")
            .exists()
    );
}

#[test]
fn tampered_backup_fails_closed_and_keeps_recovery_transaction() {
    let fixture = started_session();
    let applied = fs::read(&fixture.paths.settings).expect("settings should be readable");
    let backup = fixture.paths.backup_directory.join("settings.backup");
    fs::write(&backup, b"tampered").expect("backup should be changed");

    let error = restore_session(&fixture.paths).expect_err("tampered backup must fail closed");

    assert!(matches!(error, ZedDesktopError::BackupHashMismatch));
    assert_eq!(
        fs::read(&fixture.paths.settings).expect("settings should remain"),
        applied,
    );
    assert!(fixture.paths.session_receipt.exists());
    assert!(backup.exists());
}

#[test]
fn missing_backup_fails_closed_and_keeps_recovery_transaction() {
    let fixture = started_session();
    let applied = fs::read(&fixture.paths.settings).expect("settings should be readable");
    let backup = fixture.paths.backup_directory.join("settings.backup");
    fs::remove_file(&backup).expect("backup should be removed from the fixture");

    let error = restore_session(&fixture.paths).expect_err("missing backup must fail closed");

    assert!(matches!(error, ZedDesktopError::ReadBackup(_)));
    assert_eq!(
        fs::read(&fixture.paths.settings).expect("settings should remain"),
        applied,
    );
    assert!(fixture.paths.session_receipt.exists());
    assert!(fixture.paths.backup_directory.exists());
}
