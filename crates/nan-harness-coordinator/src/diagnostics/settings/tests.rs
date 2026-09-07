use super::{disable_settings, lock_settings, prepare_private_file, read_settings, write_settings};
use crate::CoordinatorError;
use crate::diagnostics::{CaptureSettings, active_capture_in, directory_usage, purge_captures};
use std::fs;

#[test]
fn missing_settings_default_off_but_invalid_settings_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    assert!(!read_settings(root.path()).unwrap().enabled);
    assert!(active_capture_in(root.path().to_path_buf()).is_none());
    for bytes in [
        b"malformed synthetic settings".as_slice(),
        br#"{"schema_version":2,"enabled":true}"#,
        br#"{"schema_version":1,"enabled":"synthetic-private-value"}"#,
    ] {
        fs::write(root.path().join("settings.json"), bytes).unwrap();
        let error = read_settings(root.path()).unwrap_err();
        assert_eq!(error.code(), "NH-COORD-005");
        assert!(!error.to_string().contains("synthetic-private-value"));
        assert!(active_capture_in(root.path().to_path_buf()).is_none());
        assert_eq!(fs::read(root.path().join("settings.json")).unwrap(), bytes);
    }
}

#[test]
fn off_backs_up_exact_invalid_bytes_before_recovery_and_purge_preserves_them() {
    let root = tempfile::tempdir().unwrap();
    let originals = [
        b"malformed\n\xff synthetic".as_slice(),
        br#"{"schema_version":2,"enabled":true,"future":"synthetic"}"#,
    ];
    for bytes in originals {
        fs::write(root.path().join("settings.json"), bytes).unwrap();
        let (settings, recovered) = disable_settings(root.path()).unwrap();
        assert!(recovered);
        assert!(!settings.enabled);
        assert!(!read_settings(root.path()).unwrap().enabled);
    }
    purge_captures(root.path()).unwrap();
    let backups: Vec<_> = fs::read_dir(root.path().join("settings-backups"))
        .unwrap()
        .map(|entry| fs::read(entry.unwrap().path()).unwrap())
        .collect();
    assert_eq!(backups.len(), 2);
    for original in originals {
        assert!(backups.iter().any(|bytes| bytes == original));
    }
    assert_eq!(
        directory_usage(&root.path().join("captures")).unwrap(),
        (0, 0)
    );
    assert!(!disable_settings(root.path()).unwrap().1);
}

#[test]
fn backup_failure_preserves_original_settings() {
    let root = tempfile::tempdir().unwrap();
    let original = b"malformed synthetic settings";
    fs::write(root.path().join("settings.json"), original).unwrap();
    fs::write(root.path().join("settings-backups"), b"unrelated state").unwrap();
    assert!(disable_settings(root.path()).is_err());
    assert_eq!(
        fs::read(root.path().join("settings.json")).unwrap(),
        original
    );
    assert_eq!(
        fs::read(root.path().join("settings-backups")).unwrap(),
        b"unrelated state"
    );
}

#[test]
fn io_errors_are_not_recovered_as_corruption() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("settings.json")).unwrap();
    fs::write(root.path().join("settings.json/owned"), b"keep").unwrap();
    assert!(matches!(
        read_settings(root.path()),
        Err(CoordinatorError::State { .. })
    ));
    assert!(matches!(
        disable_settings(root.path()),
        Err(CoordinatorError::State { .. })
    ));
    assert!(active_capture_in(root.path().to_path_buf()).is_none());
    assert!(!root.path().join("settings-backups").exists());
    assert_eq!(
        fs::read(root.path().join("settings.json/owned")).unwrap(),
        b"keep"
    );
}

#[test]
fn failed_replacement_cleans_only_owned_temporary_file() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("settings.json")).unwrap();
    fs::write(root.path().join("settings.json/owned"), b"keep").unwrap();
    fs::write(root.path().join(".settings-unrelated"), b"keep").unwrap();
    assert!(write_settings(root.path(), &CaptureSettings::default()).is_err());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    assert_eq!(
        fs::read(root.path().join("settings.json/owned")).unwrap(),
        b"keep"
    );
    assert_eq!(
        fs::read(root.path().join(".settings-unrelated")).unwrap(),
        b"keep"
    );
}

#[test]
fn abandoned_staged_settings_preserve_published_bytes() {
    let root = tempfile::tempdir().unwrap();
    write_settings(root.path(), &CaptureSettings::default()).unwrap();
    let original = fs::read(root.path().join("settings.json")).unwrap();
    let staged = prepare_private_file(root.path(), b"different synthetic settings").unwrap();
    assert_eq!(
        fs::read(root.path().join("settings.json")).unwrap(),
        original
    );
    drop(staged);
    assert_eq!(
        fs::read(root.path().join("settings.json")).unwrap(),
        original
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn settings_writers_serialize_without_taking_the_capture_lock() {
    let root = tempfile::tempdir().unwrap();
    let lock = lock_settings(root.path()).unwrap();
    let contender =
        nan_harness_private_fs::open_private_truncate(&root.path().join("settings.lock")).unwrap();
    assert!(contender.try_lock().is_err());
    purge_captures(root.path()).unwrap();
    drop(lock);
    contender.try_lock().unwrap();
}

#[test]
fn concurrent_readers_observe_complete_old_or_new_settings() {
    let root = tempfile::tempdir().unwrap();
    let old = CaptureSettings::default();
    let new = CaptureSettings {
        schema_version: 1,
        enabled: true,
        capture_id: Some("synthetic-capture".to_owned()),
        enabled_at_unix_seconds: Some(123),
    };
    write_settings(root.path(), &old).unwrap();
    std::thread::scope(|scope| {
        let reader = scope.spawn(|| {
            for _ in 0..500 {
                let settings = read_settings(root.path()).unwrap();
                if settings.enabled {
                    assert_eq!(settings.capture_id, new.capture_id);
                    assert_eq!(settings.enabled_at_unix_seconds, Some(123));
                } else {
                    assert!(settings.capture_id.is_none());
                    assert!(settings.enabled_at_unix_seconds.is_none());
                }
            }
        });
        for _ in 0..100 {
            write_settings(root.path(), &new).unwrap();
            write_settings(root.path(), &old).unwrap();
        }
        reader.join().unwrap();
    });
}

#[cfg(unix)]
#[test]
fn temporary_published_and_backup_settings_are_private() {
    use std::os::unix::fs::PermissionsExt as _;
    let root = tempfile::tempdir().unwrap();
    let temporary = prepare_private_file(root.path(), b"synthetic").unwrap();
    let temporary_path = temporary.path().to_path_buf();
    assert_eq!(
        temporary.as_file().metadata().unwrap().permissions().mode() & 0o777,
        0o600
    );
    drop(temporary);
    assert!(!temporary_path.exists());
    fs::write(root.path().join("settings.json"), b"invalid").unwrap();
    disable_settings(root.path()).unwrap();
    let backup = fs::read_dir(root.path().join("settings-backups"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    for path in [root.path().join("settings.json"), backup] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert_eq!(
        fs::metadata(root.path().join("settings-backups"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[cfg(unix)]
#[test]
fn failed_write_preserves_published_bytes() {
    use std::os::unix::fs::PermissionsExt as _;
    let root = tempfile::tempdir().unwrap();
    write_settings(root.path(), &CaptureSettings::default()).unwrap();
    let original = fs::read(root.path().join("settings.json")).unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o500)).unwrap();
    let result = write_settings(root.path(), &CaptureSettings::default());
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(result, Err(CoordinatorError::State { .. })));
    assert_eq!(
        fs::read(root.path().join("settings.json")).unwrap(),
        original
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[cfg(windows)]
#[test]
fn temporary_published_and_backup_settings_have_private_windows_dacls() {
    use nan_harness_test_support::windows_acl::{
        assert_private_directory, assert_private_file, make_permissive_directory,
    };

    let root = tempfile::tempdir().unwrap();
    make_permissive_directory(root.path()).unwrap();
    let temporary = prepare_private_file(root.path(), b"synthetic").unwrap();
    assert_private_file(temporary.path()).unwrap();
    let temporary_path = temporary.path().to_path_buf();
    drop(temporary);
    assert!(!temporary_path.exists());

    let settings = root.path().join("settings.json");
    fs::write(&settings, b"invalid synthetic settings").unwrap();
    disable_settings(root.path()).unwrap();
    assert_private_file(&settings).unwrap();
    let backups = root.path().join("settings-backups");
    assert_private_directory(&backups).unwrap();
    let backup = fs::read_dir(&backups)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_private_file(&backup).unwrap();
    assert_eq!(fs::read(backup).unwrap(), b"invalid synthetic settings");

    // Replacing an existing destination must preserve the private DACL too.
    write_settings(root.path(), &CaptureSettings::default()).unwrap();
    assert_private_file(&settings).unwrap();
}

#[cfg(windows)]
#[test]
fn windows_sharing_violation_preserves_published_bytes_and_cleans_staging() {
    use std::os::windows::fs::OpenOptionsExt as _;
    const FILE_SHARE_READ: u32 = 1;

    let root = tempfile::tempdir().unwrap();
    write_settings(root.path(), &CaptureSettings::default()).unwrap();
    let path = root.path().join("settings.json");
    let original = fs::read(&path).unwrap();
    // Deny delete sharing to reproduce a native replacement failure.
    let reader = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&path)
        .unwrap();
    let changed = CaptureSettings {
        enabled: true,
        ..CaptureSettings::default()
    };
    assert!(matches!(
        write_settings(root.path(), &changed),
        Err(CoordinatorError::State { .. })
    ));
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    drop(reader);
    write_settings(root.path(), &changed).unwrap();
    assert!(read_settings(root.path()).unwrap().enabled);
}

#[cfg(unix)]
#[test]
fn unreadable_and_dangling_settings_are_not_missing_or_recoverable() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("settings.json");
    fs::write(&path, b"invalid").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    let read = read_settings(root.path());
    let disable = disable_settings(root.path());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(matches!(read, Err(CoordinatorError::State { .. })));
    assert!(matches!(disable, Err(CoordinatorError::State { .. })));
    assert_eq!(fs::read(&path).unwrap(), b"invalid");
    fs::remove_file(&path).unwrap();
    symlink(root.path().join("absent"), &path).unwrap();
    assert!(matches!(
        read_settings(root.path()),
        Err(CoordinatorError::State { .. })
    ));
    assert!(disable_settings(root.path()).is_err());
    assert!(path.is_symlink());
    assert!(!root.path().join("settings-backups").exists());
}
