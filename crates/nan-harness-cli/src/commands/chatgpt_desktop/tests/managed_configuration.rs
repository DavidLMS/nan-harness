//! Overlaying and restoring the managed `ChatGPT` Desktop configuration.
//!
//! A managed session owns only the settings it writes. These tests pin what a
//! profile looks like before, during and after a session for every reachable
//! original state: no configuration, a native configuration written by the app
//! itself, and a configuration the app changed while the session ran.

use super::super::profile::{ManagedProfile, ensure_managed_profile};
use super::super::session::settings::managed_document;
use super::super::session::{apply_managed_session, restore_session, selected_model_from_config};
use super::super::{CONFIG_BACKUP_NAME, ChatGptDesktopError};
use std::fs;
use std::path::Path;
use toml_edit::DocumentMut;

const CATALOG_JSON: &str = "{\"models\":[{\"id\":\"qwen3.6\"}]}\n";
const BRIDGE_URL: &str = "http://127.0.0.1:43123";
const SELECTED_MODEL: &str = "qwen3.6";

/// A native configuration written by the app during its own onboarding, with
/// preferences and an unrelated provider nan-harness must never touch.
const NATIVE_CONFIG: &str = concat!(
    "model = \"gpt-native\"\n",
    "preferred_auth_method = \"chatgpt\"\n\n",
    "[features]\n",
    "apps = true\n",
    "native_preference = true\n\n",
    "[model_providers.other]\n",
    "name = \"other\"\n",
    "base_url = \"https://example.invalid/v1\"\n",
);

fn temporary_root() -> tempfile::TempDir {
    tempfile::tempdir().expect("temporary directory should exist")
}

fn managed_profile(root: &Path) -> ManagedProfile {
    let profile = super::profile(root);
    ensure_managed_profile(&profile).expect("managed profile should be created");
    profile
}

fn apply(profile: &ManagedProfile) -> Result<(), ChatGptDesktopError> {
    apply_managed_session(
        profile,
        &managed_document(SELECTED_MODEL, BRIDGE_URL, &profile.catalog, true),
        CATALOG_JSON,
    )
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()))
}

fn document(path: &Path) -> DocumentMut {
    read(path)
        .parse::<DocumentMut>()
        .expect("configuration should be valid TOML")
}

fn assert_absent(path: &Path) {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) => panic!("{} should not exist", path.display()),
        Err(error) => panic!("{} could not be inspected: {error}", path.display()),
    }
}

fn assert_session_state_cleared(profile: &ManagedProfile) {
    assert_absent(&profile.catalog);
    assert_absent(&profile.receipt);
    assert_absent(&profile.config_backup);
}

/// Rewrites the configuration the way the app would while the session runs.
fn app_writes(profile: &ManagedProfile, edit: impl FnOnce(&mut DocumentMut)) {
    let mut applied = document(&profile.config);
    edit(&mut applied);
    fs::write(&profile.config, applied.to_string()).expect("app write should succeed");
}

#[test]
fn a_native_configuration_is_overlaid_and_restored_byte_for_byte() {
    let directory = temporary_root();
    let profile = managed_profile(&directory.path().join("profile"));
    fs::write(&profile.config, NATIVE_CONFIG).expect("native config should write");

    apply(&profile).expect("a native configuration should be adopted");
    let applied = document(&profile.config);
    assert_eq!(applied["model"].as_str(), Some(SELECTED_MODEL));
    assert_eq!(applied["model_provider"].as_str(), Some("nan_harness"));
    assert_eq!(applied["features"]["apps"].as_bool(), Some(false));
    assert_eq!(
        applied["model_providers"]["nan_harness"]["base_url"].as_str(),
        Some("http://127.0.0.1:43123/v1")
    );
    // Settings nan-harness does not own keep their native values.
    assert_eq!(
        applied["preferred_auth_method"].as_str(),
        Some("chatgpt"),
        "unrelated preferences must survive the overlay"
    );
    assert_eq!(
        applied["features"]["native_preference"].as_bool(),
        Some(true)
    );
    assert_eq!(
        applied["model_providers"]["other"]["base_url"].as_str(),
        Some("https://example.invalid/v1")
    );
    assert_eq!(read(&profile.config_backup), NATIVE_CONFIG);

    assert!(restore_session(&profile).expect("the session should restore"));
    assert_eq!(read(&profile.config), NATIVE_CONFIG);
    assert_session_state_cleared(&profile);
}

#[test]
fn an_absent_configuration_is_restored_to_absence() {
    let directory = temporary_root();
    let profile = managed_profile(&directory.path().join("profile"));

    apply(&profile).expect("an empty profile should be adopted");
    assert_eq!(read(&profile.catalog), CATALOG_JSON);
    assert_absent(&profile.config_backup);
    assert_eq!(
        selected_model_from_config(&profile, &[SELECTED_MODEL.to_owned()]),
        Some(SELECTED_MODEL.to_owned()),
        "the selected model must be readable before restoration"
    );

    assert!(restore_session(&profile).expect("the session should restore"));
    assert_absent(&profile.config);
    assert_session_state_cleared(&profile);
    assert!(
        !restore_session(&profile).expect("a second restoration should be inert"),
        "recovery must be idempotent"
    );
}

#[test]
fn settings_the_app_wrote_survive_restoration() {
    let directory = temporary_root();
    let profile = managed_profile(&directory.path().join("profile"));
    fs::write(&profile.config, NATIVE_CONFIG).expect("native config should write");

    apply(&profile).expect("a native configuration should be adopted");
    app_writes(&profile, |applied| {
        applied["model"] = toml_edit::value("gpt-picked-in-app");
        applied["preferred_auth_method"] = toml_edit::value("apikey");
        applied["onboarding_completed"] = toml_edit::value(true);
        applied["features"]["apps"] = toml_edit::value(true);
        applied["features"]["native_preference"] = toml_edit::value(false);
    });

    assert!(restore_session(&profile).expect("the session should restore"));
    let restored = document(&profile.config);
    // Owned settings return to their captured original values.
    assert_eq!(restored["model"].as_str(), Some("gpt-native"));
    assert_eq!(restored["features"]["apps"].as_bool(), Some(true));
    assert!(restored.get("model_provider").is_none());
    assert!(
        restored["model_providers"]
            .as_table()
            .expect("the unrelated provider table should remain")
            .get("nan_harness")
            .is_none()
    );
    // Everything the app changed outside them is kept.
    assert_eq!(restored["preferred_auth_method"].as_str(), Some("apikey"));
    assert_eq!(restored["onboarding_completed"].as_bool(), Some(true));
    assert_eq!(
        restored["features"]["native_preference"].as_bool(),
        Some(false)
    );
    assert_eq!(
        restored["model_providers"]["other"]["name"].as_str(),
        Some("other")
    );
    assert_session_state_cleared(&profile);
}

#[test]
fn native_settings_written_during_the_session_survive_an_absent_original() {
    let directory = temporary_root();
    let profile = managed_profile(&directory.path().join("profile"));

    apply(&profile).expect("an empty profile should be adopted");
    app_writes(&profile, |applied| {
        applied["preferred_auth_method"] = toml_edit::value("chatgpt");
        applied["features"]["native_preference"] = toml_edit::value(true);
    });

    assert!(restore_session(&profile).expect("the session should restore"));
    let restored = document(&profile.config);
    assert_eq!(restored["preferred_auth_method"].as_str(), Some("chatgpt"));
    assert_eq!(
        restored["features"]["native_preference"].as_bool(),
        Some(true)
    );
    assert!(restored.get("model").is_none());
    assert!(restored.get("model_providers").is_none());
    assert!(restored["features"].as_table().is_some_and(|features| {
        features.len() == 1 && features.contains_key("native_preference")
    }));
    assert_session_state_cleared(&profile);
}

#[test]
fn a_configuration_of_only_managed_settings_leaves_no_empty_file() {
    let directory = temporary_root();
    let profile = managed_profile(&directory.path().join("profile"));

    apply(&profile).expect("an empty profile should be adopted");
    app_writes(&profile, |applied| {
        applied["model"] = toml_edit::value("gpt-picked-in-app");
    });

    assert!(restore_session(&profile).expect("the session should restore"));
    assert_absent(&profile.config);
    assert_session_state_cleared(&profile);
}

#[test]
fn a_malformed_configuration_is_never_overwritten() {
    let directory = temporary_root();
    let profile = managed_profile(&directory.path().join("profile"));
    let malformed = "model = \n";
    fs::write(&profile.config, malformed).expect("malformed config should write");

    assert!(matches!(
        apply(&profile),
        Err(ChatGptDesktopError::MalformedConfig)
    ));
    assert_eq!(read(&profile.config), malformed);
    assert_absent(&profile.receipt);
    assert_absent(&profile.config_backup);
}

#[test]
fn a_configuration_the_app_corrupted_keeps_the_receipt_for_a_retry() {
    let directory = temporary_root();
    let profile = managed_profile(&directory.path().join("profile"));
    fs::write(&profile.config, NATIVE_CONFIG).expect("native config should write");
    apply(&profile).expect("a native configuration should be adopted");
    fs::write(&profile.config, "model = \n").expect("corrupted config should write");

    assert!(matches!(
        restore_session(&profile),
        Err(ChatGptDesktopError::MalformedConfig)
    ));
    assert_eq!(read(&profile.config_backup), NATIVE_CONFIG);
    assert!(profile.receipt.exists());

    // The user repairs the document; the retry restores the owned settings.
    fs::write(&profile.config, "model = \"repaired\"\n").expect("repaired config should write");
    assert!(restore_session(&profile).expect("the retry should restore"));
    assert_eq!(
        document(&profile.config)["model"].as_str(),
        Some("gpt-native")
    );
    assert_session_state_cleared(&profile);
}

#[test]
fn a_tampered_backup_is_never_written_back() {
    let directory = temporary_root();
    let profile = managed_profile(&directory.path().join("profile"));
    fs::write(&profile.config, NATIVE_CONFIG).expect("native config should write");
    apply(&profile).expect("a native configuration should be adopted");
    let applied = read(&profile.config);
    fs::write(&profile.config_backup, "model = \"injected\"\n").expect("backup should write");

    assert!(matches!(
        restore_session(&profile),
        Err(ChatGptDesktopError::BackupHashMismatch)
    ));
    assert_eq!(read(&profile.config), applied);
    assert!(profile.receipt.exists());
}

#[test]
fn a_receipt_that_redirects_its_backup_is_rejected() {
    let directory = temporary_root();
    let profile = managed_profile(&directory.path().join("profile"));
    fs::write(&profile.config, NATIVE_CONFIG).expect("native config should write");
    apply(&profile).expect("a native configuration should be adopted");
    let applied = read(&profile.config);
    let mut receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(&profile.receipt).expect("receipt should read"))
            .expect("receipt should parse");
    receipt["originalConfig"]["backupFile"] = serde_json::Value::String("../escape".to_owned());
    fs::write(
        &profile.receipt,
        serde_json::to_vec(&receipt).expect("receipt should serialize"),
    )
    .expect("receipt should write");

    assert!(matches!(
        restore_session(&profile),
        Err(ChatGptDesktopError::InvalidReceipt)
    ));
    assert_eq!(read(&profile.config), applied);
    assert_eq!(read(&profile.config_backup), NATIVE_CONFIG);
}

#[test]
fn a_missing_backup_is_reported_instead_of_dropping_the_configuration() {
    let directory = temporary_root();
    let profile = managed_profile(&directory.path().join("profile"));
    fs::write(&profile.config, NATIVE_CONFIG).expect("native config should write");
    apply(&profile).expect("a native configuration should be adopted");
    let applied = read(&profile.config);
    fs::remove_file(&profile.config_backup).expect("backup should be removable");

    assert!(matches!(
        restore_session(&profile),
        Err(ChatGptDesktopError::MissingBackup)
    ));
    assert_eq!(read(&profile.config), applied);
    assert!(profile.receipt.exists());
}

#[test]
fn an_interrupted_apply_that_never_wrote_the_configuration_restores_cleanly() {
    let directory = temporary_root();
    let profile = managed_profile(&directory.path().join("profile"));
    fs::write(&profile.config, NATIVE_CONFIG).expect("native config should write");
    apply(&profile).expect("a native configuration should be adopted");
    // The state a crash between the receipt and the configuration write leaves.
    fs::write(&profile.config, NATIVE_CONFIG).expect("original config should return");
    fs::remove_file(&profile.catalog).expect("catalog should be removable");

    assert!(restore_session(&profile).expect("the session should restore"));
    assert_eq!(read(&profile.config), NATIVE_CONFIG);
    assert_session_state_cleared(&profile);
}

#[test]
fn a_backup_without_a_receipt_blocks_adoption() {
    let directory = temporary_root();
    let profile = managed_profile(&directory.path().join("profile"));
    fs::write(&profile.config, NATIVE_CONFIG).expect("native config should write");
    fs::write(&profile.config_backup, NATIVE_CONFIG).expect("backup should write");

    assert!(matches!(
        apply(&profile),
        Err(ChatGptDesktopError::OrphanedSessionFiles)
    ));
    assert_eq!(read(&profile.config), NATIVE_CONFIG);
    assert_eq!(
        profile
            .config_backup
            .file_name()
            .and_then(|name| name.to_str()),
        Some(CONFIG_BACKUP_NAME)
    );
}

#[test]
fn an_obstructed_catalog_keeps_the_receipt_until_the_retry_succeeds() {
    let directory = temporary_root();
    let profile = managed_profile(&directory.path().join("profile"));
    fs::write(&profile.config, NATIVE_CONFIG).expect("native config should write");
    apply(&profile).expect("a native configuration should be adopted");
    fs::remove_file(&profile.catalog).expect("catalog should be removable");
    fs::create_dir(&profile.catalog).expect("catalog obstruction should exist");
    fs::write(profile.catalog.join("sentinel"), "sentinel\n").expect("sentinel should write");

    assert!(matches!(
        restore_session(&profile),
        Err(ChatGptDesktopError::State(_))
    ));
    // The configuration was restored first and the receipt survives the failure.
    assert_eq!(read(&profile.config), NATIVE_CONFIG);
    assert!(profile.receipt.exists());
    assert_eq!(read(&profile.config_backup), NATIVE_CONFIG);

    fs::remove_file(profile.catalog.join("sentinel")).expect("sentinel should be removable");
    fs::remove_dir(&profile.catalog).expect("obstruction should be removable");
    assert!(restore_session(&profile).expect("the retry should complete recovery"));
    assert_eq!(read(&profile.config), NATIVE_CONFIG);
    assert_session_state_cleared(&profile);
}

#[cfg(unix)]
mod unix {
    use super::{NATIVE_CONFIG, apply, managed_profile, read, temporary_root};
    use crate::commands::chatgpt_desktop::ChatGptDesktopError;
    use crate::commands::chatgpt_desktop::session::restore_session;
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;

    fn mode(path: &std::path::Path) -> u32 {
        fs::metadata(path)
            .expect("metadata should be readable")
            .permissions()
            .mode()
            & 0o777
    }

    #[test]
    fn the_original_file_mode_returns_with_the_original_bytes() {
        let directory = temporary_root();
        let profile = managed_profile(&directory.path().join("profile"));
        fs::write(&profile.config, NATIVE_CONFIG).expect("native config should write");
        fs::set_permissions(&profile.config, fs::Permissions::from_mode(0o640))
            .expect("native mode should apply");

        apply(&profile).expect("a native configuration should be adopted");
        assert_eq!(
            mode(&profile.config),
            0o600,
            "the applied session is private"
        );
        assert_eq!(mode(&profile.config_backup), 0o600);

        assert!(restore_session(&profile).expect("the session should restore"));
        assert_eq!(read(&profile.config), NATIVE_CONFIG);
        assert_eq!(mode(&profile.config), 0o640);
    }

    #[test]
    fn a_configuration_symlink_is_refused_before_any_managed_write() {
        let directory = temporary_root();
        let profile = managed_profile(&directory.path().join("profile"));
        let target = directory.path().join("outside-config.toml");
        fs::write(&target, NATIVE_CONFIG).expect("target should write");
        std::os::unix::fs::symlink(&target, &profile.config).expect("config link should exist");

        assert!(matches!(
            apply(&profile),
            Err(ChatGptDesktopError::State(_))
        ));
        assert_eq!(read(&target), NATIVE_CONFIG);
        assert!(!profile.receipt.exists());
    }

    #[test]
    fn a_backup_symlink_is_refused_during_restoration() {
        let directory = temporary_root();
        let profile = managed_profile(&directory.path().join("profile"));
        fs::write(&profile.config, NATIVE_CONFIG).expect("native config should write");
        apply(&profile).expect("a native configuration should be adopted");
        let target = directory.path().join("outside-backup.toml");
        fs::write(&target, NATIVE_CONFIG).expect("target should write");
        fs::remove_file(&profile.config_backup).expect("backup should be removable");
        std::os::unix::fs::symlink(&target, &profile.config_backup)
            .expect("backup link should exist");

        assert!(matches!(
            restore_session(&profile),
            Err(ChatGptDesktopError::State(_))
        ));
        assert_eq!(read(&target), NATIVE_CONFIG);
        assert!(profile.receipt.exists());
    }
}
