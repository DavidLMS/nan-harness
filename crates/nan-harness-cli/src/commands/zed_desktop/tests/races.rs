use super::super::ZedDesktopError;
use super::super::session::{begin_session_for_test, begin_session_with_check};
use super::fixtures::{GATEWAY_URL, fixture_paths, generic_model, write_settings};
use std::fs;

#[test]
fn startup_race_does_not_touch_settings_or_leave_recovery_state() {
    let fixture = fixture_paths();
    let original = b"{\"theme\":\"race-safe\"}\n";
    write_settings(&fixture.paths, original);

    let error = begin_session_for_test(
        &fixture.paths,
        GATEWAY_URL,
        &[generic_model()],
        "qwen3.6",
        true,
    )
    .expect_err("a process race should fail");

    assert!(matches!(error, ZedDesktopError::AlreadyRunning));
    assert_eq!(
        fs::read(&fixture.paths.settings).expect("settings should remain"),
        original
    );
    assert!(!fixture.paths.session_receipt.exists());
    assert!(!fixture.paths.backup_directory.exists());
}

#[test]
fn settings_race_before_write_fails_without_overwriting_the_new_bytes() {
    let fixture = fixture_paths();
    let original = b"{\"theme\":\"before\"}\n";
    let raced = b"{\"theme\":\"changed-by-user\"}\n";
    write_settings(&fixture.paths, original);

    let error = begin_session_with_check(
        &fixture.paths,
        GATEWAY_URL,
        &[generic_model()],
        "qwen3.6",
        || {
            fs::write(&fixture.paths.settings, raced).expect("racing write should succeed");
            Ok(false)
        },
    )
    .expect_err("settings race should fail");

    assert!(matches!(error, ZedDesktopError::SettingsChangedBeforeWrite));
    assert_eq!(
        fs::read(&fixture.paths.settings).expect("racing settings should remain"),
        raced
    );
    assert!(!fixture.paths.session_receipt.exists());
    assert!(!fixture.paths.backup_directory.exists());
}
