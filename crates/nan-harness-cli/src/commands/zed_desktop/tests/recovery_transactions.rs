use super::super::ZedDesktopError;
use super::super::session::{begin_session_for_test, ensure_no_pending_session, restore_session};
use super::fixtures::{FixturePaths, GATEWAY_URL, fixture_paths, generic_model, write_settings};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

const ORIGINAL: &[u8] = b"{\n  // preserve exact user bytes\n  \"theme\": \"before\",\n}\n";

struct Session {
    fixture: FixturePaths,
    applied: Vec<u8>,
    receipt: Vec<u8>,
    backup: Vec<u8>,
}

impl Session {
    fn start() -> Self {
        let fixture = fixture_paths();
        write_settings(&fixture.paths, ORIGINAL);
        begin_session_for_test(
            &fixture.paths,
            GATEWAY_URL,
            &[generic_model()],
            "qwen3.6",
            false,
        )
        .expect("session should begin");
        let applied = fs::read(&fixture.paths.settings).expect("applied settings");
        let receipt = fs::read(&fixture.paths.session_receipt).expect("receipt");
        let backup = fs::read(fixture.paths.backup_directory.join("settings.backup"))
            .expect("original backup");
        assert_eq!(backup, ORIGINAL);
        assert_ne!(applied, ORIGINAL);
        Self {
            fixture,
            applied,
            receipt,
            backup,
        }
    }

    fn assert_pending(&self) {
        assert_eq!(
            fs::read(&self.fixture.paths.session_receipt).expect("retained receipt"),
            self.receipt,
        );
        assert!(matches!(
            ensure_no_pending_session(&self.fixture.paths),
            Err(ZedDesktopError::PendingRecovery),
        ));
    }

    fn assert_recovered(&self) {
        let paths = &self.fixture.paths;
        assert_eq!(
            fs::read(&paths.settings).expect("restored settings"),
            ORIGINAL
        );
        assert_absent(&paths.session_receipt);
        assert_absent(&paths.backup_directory);
        ensure_no_pending_session(paths).expect("pending guard should clear");
        assert!(!restore_session(paths).expect("further restoration should be inert"));
        assert_eq!(
            fs::read(&paths.settings).expect("unchanged settings"),
            ORIGINAL
        );
    }
}

fn assert_absent(path: &Path) {
    assert_eq!(
        fs::symlink_metadata(path)
            .expect_err("path should be absent")
            .kind(),
        ErrorKind::NotFound,
    );
}

#[test]
fn interrupted_cleanup_preserves_unknown_files_and_can_resume_without_backup() {
    let session = Session::start();
    let paths = &session.fixture.paths;
    let backup = paths.backup_directory.join("settings.backup");
    let sentinel = paths.backup_directory.join("user-note.txt");
    fs::write(&sentinel, b"keep this unknown file").expect("synthetic sentinel");

    for _ in 0..2 {
        assert!(matches!(
            restore_session(paths),
            Err(ZedDesktopError::RemoveBackup(_))
        ));
        assert_eq!(
            fs::read(&paths.settings).expect("restored settings"),
            ORIGINAL
        );
        assert_absent(&backup);
        assert_eq!(
            fs::read(&sentinel).expect("retained sentinel"),
            b"keep this unknown file"
        );
        session.assert_pending();
    }

    fs::remove_file(&sentinel).expect("explicitly resolve the fixture obstruction");
    assert!(restore_session(paths).expect("cleanup should resume without a backup"));
    session.assert_recovered();
}

#[test]
fn missing_backup_preserves_applied_settings_and_allows_repair() {
    let session = Session::start();
    let paths = &session.fixture.paths;
    let backup = paths.backup_directory.join("settings.backup");
    fs::remove_file(&backup).expect("remove only the fixture backup");

    assert!(matches!(
        restore_session(paths),
        Err(ZedDesktopError::ReadBackup(_))
    ));
    assert_eq!(
        fs::read(&paths.settings).expect("applied settings"),
        session.applied
    );
    assert_absent(&backup);
    assert!(
        fs::metadata(&paths.backup_directory)
            .expect("retained directory")
            .is_dir()
    );
    session.assert_pending();

    fs::write(&backup, &session.backup).expect("replace authentic fixture backup bytes");
    assert!(restore_session(paths).expect("restoration should succeed after repair"));
    session.assert_recovered();
}

#[test]
fn deleted_original_settings_remain_user_owned_until_explicit_resolution() {
    let session = Session::start();
    let paths = &session.fixture.paths;
    let backup = paths.backup_directory.join("settings.backup");
    fs::remove_file(&paths.settings).expect("delete settings in the fixture");

    for _ in 0..2 {
        assert!(matches!(
            restore_session(paths),
            Err(ZedDesktopError::ManagedConfigurationChanged)
        ));
        assert_absent(&paths.settings);
        assert_eq!(fs::read(&backup).expect("retained backup"), session.backup);
        session.assert_pending();
    }

    fs::write(&paths.settings, &session.applied).expect("explicitly resolve the fixture deletion");
    assert!(restore_session(paths).expect("restoration should succeed after resolution"));
    session.assert_recovered();
}
