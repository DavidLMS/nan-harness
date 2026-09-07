use crate::support::{assert_success, diagnostics, root};
use std::fs;

#[test]
fn missing_settings_report_off_without_publishing() {
    let root = root();
    let output = diagnostics(root.path(), &["status"]);
    assert_success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("Local diagnostics: off"));
    assert!(!root.path().join("state/diagnostics/settings.json").exists());
}

#[test]
fn invalid_settings_require_backup_before_off_and_survive_purge() {
    let root = root();
    let directory = root.path().join("state/diagnostics");
    fs::create_dir(&directory).unwrap();
    let path = directory.join("settings.json");
    let original = br#"{"schema_version":2,"enabled":true,"private":"synthetic-private-value"}"#;
    fs::write(&path, original).unwrap();
    for action in ["status", "on"] {
        let output = diagnostics(root.path(), &[action]);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("NH-COORD-005"));
        assert!(!error.contains("synthetic-private-value"));
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!directory.join("settings-backups").exists());
        assert!(!directory.join("captures").exists());
    }
    assert_success(&diagnostics(root.path(), &["off"]));
    let backups: Vec<_> = fs::read_dir(directory.join("settings-backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read(&backups[0]).unwrap(), original);
    let settings: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(settings["enabled"], false);
    assert_success(&diagnostics(root.path(), &["purge", "--yes"]));
    assert_eq!(fs::read(&backups[0]).unwrap(), original);
}

#[test]
fn backup_failure_preserves_invalid_settings_and_user_owned_obstruction() {
    let root = root();
    let directory = root.path().join("state/diagnostics");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("settings.json"), b"invalid synthetic state").unwrap();
    fs::write(directory.join("settings-backups"), b"keep").unwrap();
    let output = diagnostics(root.path(), &["off"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("NH-COORD-002"));
    assert_eq!(
        fs::read(directory.join("settings.json")).unwrap(),
        b"invalid synthetic state"
    );
    assert_eq!(
        fs::read(directory.join("settings-backups")).unwrap(),
        b"keep"
    );
}
