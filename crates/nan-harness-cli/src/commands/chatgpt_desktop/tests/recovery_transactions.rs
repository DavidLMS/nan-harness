//! Recovery of interrupted `ChatGPT` Desktop sessions.
//!
//! `restore_session` performs receipt-owned ordered cleanup, not an atomic
//! transaction: a failure part-way through leaves the remaining files and the
//! receipt in place so the next `--restore` can finish the work. These tests
//! pin that contract against explicit reachable interruption states.

use super::super::profile::{ManagedProfile, ensure_managed_profile};
use super::super::session::{SessionReceiptV1, reject_orphaned_session_files, restore_session};
use super::super::{
    CONFIG_FILE_NAME, ChatGptDesktopError, MODEL_CATALOG_FILE_NAME, SESSION_SCHEMA_VERSION,
    SESSION_SCHEMA_VERSION_2, SURFACE_ID,
};
use crate::commands::desktop::DesktopStateError;
use std::fs;
use std::path::{Path, PathBuf};

const AUTH_FILE: &str = "auth.json";
const UNRELATED_FILE: &str = "history.jsonl";
const CONFIG_BYTES: &[u8] = b"model = \"qwen3.6\"\n";
/// A configuration that still carries nan-harness session routing.
const MANAGED_CONFIG_BYTES: &[u8] =
    b"model_provider = \"nan_harness\"\n\n[model_providers.nan_harness]\nname = \"nan-harness\"\n";
const CATALOG_BYTES: &[u8] = b"{\"models\":[]}\n";
const AUTH_BYTES: &[u8] = b"{\"token\":\"placeholder\"}\n";
const UNRELATED_BYTES: &[u8] = b"{\"turn\":1}\n";
const SENTINEL_BYTES: &[u8] = b"sentinel\n";
const DECOY_BYTES: &[u8] = b"decoy\n";

/// A profile whose ownership marker and persistent user state already exist.
fn established_profile(root: &Path) -> ManagedProfile {
    let profile = super::profile(root);
    ensure_managed_profile(&profile).expect("managed profile should be created");
    fs::write(root.join(AUTH_FILE), AUTH_BYTES).expect("persistent auth should write");
    fs::write(root.join(UNRELATED_FILE), UNRELATED_BYTES).expect("unrelated state should write");
    profile
}

fn valid_receipt() -> SessionReceiptV1 {
    SessionReceiptV1 {
        schema_version: SESSION_SCHEMA_VERSION,
        surface: SURFACE_ID.to_owned(),
        config_file: CONFIG_FILE_NAME.to_owned(),
        model_catalog_file: MODEL_CATALOG_FILE_NAME.to_owned(),
    }
}

/// Writes a receipt exactly as `apply_session` would and returns its bytes.
fn write_receipt(profile: &ManagedProfile, receipt: &SessionReceiptV1) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(receipt).expect("receipt should serialize");
    bytes.push(b'\n');
    fs::write(&profile.receipt, &bytes).expect("receipt should write");
    bytes
}

fn read_file(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()))
}

/// Asserts absence specifically, so an unrelated IO failure cannot pass as one.
fn assert_absent(path: &Path) {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) => panic!("{} should have been removed", path.display()),
        Err(error) => panic!("{} could not be inspected: {error}", path.display()),
    }
}

fn persistent_snapshot(profile: &ManagedProfile) -> Vec<(PathBuf, Vec<u8>)> {
    [
        profile.marker.clone(),
        profile.root.join(AUTH_FILE),
        profile.root.join(UNRELATED_FILE),
    ]
    .into_iter()
    .map(|path| {
        let bytes = read_file(&path);
        (path, bytes)
    })
    .collect()
}

fn assert_preserved(snapshot: &[(PathBuf, Vec<u8>)]) {
    for (path, bytes) in snapshot {
        assert_eq!(
            &read_file(path),
            bytes,
            "{} should keep its bytes",
            path.display()
        );
    }
}

fn temporary_root() -> tempfile::TempDir {
    tempfile::tempdir().expect("temporary directory should exist")
}

#[test]
fn every_interrupted_session_state_recovers_once() {
    for (state, config, catalog) in [
        ("receipt only", false, false),
        ("receipt and catalog", false, true),
        ("receipt and config", true, false),
        ("all session files", true, true),
    ] {
        let directory = temporary_root();
        let profile = established_profile(&directory.path().join("profile"));
        write_receipt(&profile, &valid_receipt());
        if config {
            fs::write(&profile.config, CONFIG_BYTES).expect("config should write");
        }
        if catalog {
            fs::write(&profile.catalog, CATALOG_BYTES).expect("catalog should write");
        }
        let snapshot = persistent_snapshot(&profile);

        assert!(
            restore_session(&profile).expect("recovery should succeed"),
            "{state} should be recovered"
        );
        assert_absent(&profile.config);
        assert_absent(&profile.catalog);
        assert_absent(&profile.receipt);
        assert_preserved(&snapshot);

        assert!(
            !restore_session(&profile).expect("a second recovery should be inert"),
            "{state} should report nothing left to recover"
        );
        assert_preserved(&snapshot);
    }
}

#[test]
fn unreceipted_session_files_are_preserved_and_block_adoption() {
    for (state, config, catalog) in [
        ("managed config only", true, false),
        ("catalog only", false, true),
        ("managed config and catalog", true, true),
    ] {
        let directory = temporary_root();
        let profile = established_profile(&directory.path().join("profile"));
        if config {
            fs::write(&profile.config, MANAGED_CONFIG_BYTES).expect("config should write");
        }
        if catalog {
            fs::write(&profile.catalog, CATALOG_BYTES).expect("catalog should write");
        }
        let snapshot = persistent_snapshot(&profile);

        assert!(
            !restore_session(&profile).expect("recovery should claim nothing without a receipt"),
            "{state} should not be recovered"
        );
        if config {
            assert_eq!(read_file(&profile.config), MANAGED_CONFIG_BYTES, "{state}");
        }
        if catalog {
            assert_eq!(read_file(&profile.catalog), CATALOG_BYTES, "{state}");
        }
        assert_preserved(&snapshot);
        assert!(
            matches!(
                reject_orphaned_session_files(&profile),
                Err(ChatGptDesktopError::OrphanedSessionFiles)
            ),
            "{state} should block adoption"
        );
    }

    let directory = temporary_root();
    let profile = established_profile(&directory.path().join("profile"));
    assert!(!restore_session(&profile).expect("an empty profile needs no recovery"));
    reject_orphaned_session_files(&profile).expect("an empty profile is adoptable");

    fs::write(&profile.config, CONFIG_BYTES).expect("native config should write");
    assert!(!restore_session(&profile).expect("a native configuration needs no recovery"));
    reject_orphaned_session_files(&profile)
        .expect("a native configuration without managed routing is adoptable");
    assert_eq!(read_file(&profile.config), CONFIG_BYTES);

    // A renamed provider table is still recognizable by the session token.
    fs::write(
        &profile.config,
        b"[model_providers.renamed]\nenv_key = \"NAN_HARNESS_SESSION_TOKEN\"\n",
    )
    .expect("renamed managed config should write");
    assert!(matches!(
        reject_orphaned_session_files(&profile),
        Err(ChatGptDesktopError::OrphanedSessionFiles)
    ));
}

#[test]
fn every_receipt_discriminator_is_rejected_before_any_deletion() {
    let redirected_config = "redirected-config.toml";
    let redirected_catalog = "redirected-catalog.json";
    let cases = [
        (
            "unsupported schema",
            SessionReceiptV1 {
                schema_version: SESSION_SCHEMA_VERSION_2 + 1,
                ..valid_receipt()
            },
            None,
        ),
        (
            "foreign surface",
            SessionReceiptV1 {
                surface: "codex-desktop".to_owned(),
                ..valid_receipt()
            },
            None,
        ),
        (
            "redirected config name",
            SessionReceiptV1 {
                config_file: redirected_config.to_owned(),
                ..valid_receipt()
            },
            Some(redirected_config),
        ),
        (
            "redirected catalog name",
            SessionReceiptV1 {
                model_catalog_file: redirected_catalog.to_owned(),
                ..valid_receipt()
            },
            Some(redirected_catalog),
        ),
    ];

    for (discriminator, receipt, decoy) in cases {
        let directory = temporary_root();
        let profile = established_profile(&directory.path().join("profile"));
        fs::write(&profile.config, CONFIG_BYTES).expect("config should write");
        fs::write(&profile.catalog, CATALOG_BYTES).expect("catalog should write");
        let receipt_bytes = write_receipt(&profile, &receipt);
        let decoy_path = decoy.map(|name| {
            let path = profile.root.join(name);
            fs::write(&path, DECOY_BYTES).expect("decoy should write");
            path
        });
        let snapshot = persistent_snapshot(&profile);

        assert!(
            matches!(
                restore_session(&profile),
                Err(ChatGptDesktopError::InvalidReceipt)
            ),
            "{discriminator} should be rejected"
        );
        assert_eq!(read_file(&profile.config), CONFIG_BYTES, "{discriminator}");
        assert_eq!(
            read_file(&profile.catalog),
            CATALOG_BYTES,
            "{discriminator}"
        );
        assert_eq!(
            read_file(&profile.receipt),
            receipt_bytes,
            "{discriminator}"
        );
        if let Some(path) = decoy_path {
            assert_eq!(read_file(&path), DECOY_BYTES, "{discriminator}");
        }
        assert_preserved(&snapshot);
    }
}

#[test]
fn unparseable_receipts_are_rejected_before_any_deletion() {
    let cases = [
        ("malformed JSON", b"{\"schemaVersion\": 1".to_vec()),
        (
            "unknown field",
            br#"{"schemaVersion":1,"surface":"chatgpt-desktop","configFile":"config.toml","modelCatalogFile":"nan-model-catalog.json","profileFile":"extra.json"}"#.to_vec(),
        ),
    ];

    for (shape, bytes) in cases {
        let directory = temporary_root();
        let profile = established_profile(&directory.path().join("profile"));
        fs::write(&profile.config, CONFIG_BYTES).expect("config should write");
        fs::write(&profile.catalog, CATALOG_BYTES).expect("catalog should write");
        fs::write(&profile.receipt, &bytes).expect("receipt should write");
        let snapshot = persistent_snapshot(&profile);

        assert!(
            matches!(
                restore_session(&profile),
                Err(ChatGptDesktopError::ParseReceipt(_))
            ),
            "{shape} should fail to parse"
        );
        assert_eq!(read_file(&profile.config), CONFIG_BYTES, "{shape}");
        assert_eq!(read_file(&profile.catalog), CATALOG_BYTES, "{shape}");
        assert_eq!(read_file(&profile.receipt), bytes, "{shape}");
        assert_preserved(&snapshot);
    }
}

#[test]
fn a_failed_first_deletion_preserves_every_recovery_input() {
    let directory = temporary_root();
    let profile = established_profile(&directory.path().join("profile"));
    let receipt_bytes = write_receipt(&profile, &valid_receipt());
    fs::write(&profile.catalog, CATALOG_BYTES).expect("catalog should write");
    fs::create_dir(&profile.config).expect("config obstruction should exist");
    let sentinel = profile.config.join("sentinel");
    fs::write(&sentinel, SENTINEL_BYTES).expect("sentinel should write");
    let snapshot = persistent_snapshot(&profile);

    assert!(matches!(
        restore_session(&profile),
        Err(ChatGptDesktopError::State(DesktopStateError::Io(_)))
    ));
    assert_eq!(read_file(&sentinel), SENTINEL_BYTES);
    assert_eq!(read_file(&profile.catalog), CATALOG_BYTES);
    assert_eq!(read_file(&profile.receipt), receipt_bytes);
    assert_preserved(&snapshot);

    fs::remove_file(&sentinel).expect("sentinel should be removable");
    fs::remove_dir(&profile.config).expect("obstruction should be removable");

    assert!(restore_session(&profile).expect("the retry should complete cleanup"));
    assert_absent(&profile.config);
    assert_absent(&profile.catalog);
    assert_absent(&profile.receipt);
    assert_preserved(&snapshot);
}

#[test]
fn a_failure_after_config_deletion_stays_restartable() {
    let directory = temporary_root();
    let profile = established_profile(&directory.path().join("profile"));
    let receipt_bytes = write_receipt(&profile, &valid_receipt());
    fs::write(&profile.config, CONFIG_BYTES).expect("config should write");
    fs::create_dir(&profile.catalog).expect("catalog obstruction should exist");
    let sentinel = profile.catalog.join("sentinel");
    fs::write(&sentinel, SENTINEL_BYTES).expect("sentinel should write");
    let snapshot = persistent_snapshot(&profile);

    assert!(matches!(
        restore_session(&profile),
        Err(ChatGptDesktopError::State(DesktopStateError::Io(_)))
    ));
    assert_absent(&profile.config);
    assert_eq!(read_file(&sentinel), SENTINEL_BYTES);
    assert_eq!(read_file(&profile.receipt), receipt_bytes);
    assert_preserved(&snapshot);

    fs::remove_file(&sentinel).expect("sentinel should be removable");
    fs::remove_dir(&profile.catalog).expect("obstruction should be removable");

    assert!(
        restore_session(&profile).expect("the retry should tolerate the already deleted config")
    );
    assert_absent(&profile.catalog);
    assert_absent(&profile.receipt);
    assert!(!restore_session(&profile).expect("a further retry should be inert"));
    assert_preserved(&snapshot);
}

#[cfg(unix)]
mod symlinks {
    use super::{
        CATALOG_BYTES, CONFIG_BYTES, ChatGptDesktopError, DesktopStateError, SENTINEL_BYTES,
        assert_absent, assert_preserved, established_profile, fs, persistent_snapshot, read_file,
        restore_session, temporary_root, valid_receipt, write_receipt,
    };
    use std::path::{Path, PathBuf};

    /// A target the managed profile has no authority to delete.
    fn sentinel(directory: &Path) -> PathBuf {
        let path = directory.join("outside-sentinel");
        fs::write(&path, SENTINEL_BYTES).expect("sentinel should write");
        path
    }

    fn assert_link_intact(link: &Path, target: &Path) {
        let metadata = fs::symlink_metadata(link).expect("link should still exist");
        assert!(
            metadata.file_type().is_symlink(),
            "link should be a symlink"
        );
        assert_eq!(read_file(target), SENTINEL_BYTES, "target should be intact");
    }

    #[test]
    fn a_receipt_symlink_is_refused_before_any_deletion() {
        let directory = temporary_root();
        let profile = established_profile(&directory.path().join("profile"));
        let target = sentinel(directory.path());
        std::os::unix::fs::symlink(&target, &profile.receipt).expect("receipt link should exist");
        fs::write(&profile.config, CONFIG_BYTES).expect("config should write");
        fs::write(&profile.catalog, CATALOG_BYTES).expect("catalog should write");
        let snapshot = persistent_snapshot(&profile);

        assert!(matches!(
            restore_session(&profile),
            Err(ChatGptDesktopError::State(DesktopStateError::Symlink))
        ));
        assert_link_intact(&profile.receipt, &target);
        assert_eq!(read_file(&profile.config), CONFIG_BYTES);
        assert_eq!(read_file(&profile.catalog), CATALOG_BYTES);
        assert_preserved(&snapshot);

        fs::remove_file(&profile.receipt).expect("link should be removable");
        write_receipt(&profile, &valid_receipt());
        assert!(restore_session(&profile).expect("a real receipt should authorize cleanup"));
        assert_absent(&profile.config);
        assert_absent(&profile.catalog);
        assert_absent(&profile.receipt);
        assert_eq!(read_file(&target), SENTINEL_BYTES);
        assert_preserved(&snapshot);
    }

    #[test]
    fn a_config_symlink_is_refused_with_the_catalog_and_receipt_retained() {
        let directory = temporary_root();
        let profile = established_profile(&directory.path().join("profile"));
        let target = sentinel(directory.path());
        let receipt_bytes = write_receipt(&profile, &valid_receipt());
        std::os::unix::fs::symlink(&target, &profile.config).expect("config link should exist");
        fs::write(&profile.catalog, CATALOG_BYTES).expect("catalog should write");
        let snapshot = persistent_snapshot(&profile);

        assert!(matches!(
            restore_session(&profile),
            Err(ChatGptDesktopError::State(DesktopStateError::Symlink))
        ));
        assert_link_intact(&profile.config, &target);
        assert_eq!(read_file(&profile.catalog), CATALOG_BYTES);
        assert_eq!(read_file(&profile.receipt), receipt_bytes);
        assert_preserved(&snapshot);

        fs::remove_file(&profile.config).expect("link should be removable");
        assert!(restore_session(&profile).expect("the retry should complete cleanup"));
        assert_absent(&profile.catalog);
        assert_absent(&profile.receipt);
        assert_eq!(read_file(&target), SENTINEL_BYTES);
        assert_preserved(&snapshot);
    }

    #[test]
    fn a_catalog_symlink_is_refused_after_the_config_is_deleted() {
        let directory = temporary_root();
        let profile = established_profile(&directory.path().join("profile"));
        let target = sentinel(directory.path());
        let receipt_bytes = write_receipt(&profile, &valid_receipt());
        fs::write(&profile.config, CONFIG_BYTES).expect("config should write");
        std::os::unix::fs::symlink(&target, &profile.catalog).expect("catalog link should exist");
        let snapshot = persistent_snapshot(&profile);

        assert!(matches!(
            restore_session(&profile),
            Err(ChatGptDesktopError::State(DesktopStateError::Symlink))
        ));
        assert_absent(&profile.config);
        assert_link_intact(&profile.catalog, &target);
        assert_eq!(read_file(&profile.receipt), receipt_bytes);
        assert_preserved(&snapshot);

        fs::remove_file(&profile.catalog).expect("link should be removable");
        assert!(restore_session(&profile).expect("the retry should complete cleanup"));
        assert_absent(&profile.receipt);
        assert_eq!(read_file(&target), SENTINEL_BYTES);
        assert_preserved(&snapshot);
    }
}

/// How `validate_managed_profile` must refuse a tampered marker.
#[derive(Clone, Copy)]
enum MarkerRejection {
    Invalid,
    Unparseable,
}

#[test]
fn a_tampered_ownership_marker_preserves_the_established_profile() {
    let cases = [
        (
            "unsupported schema",
            br#"{"schemaVersion":2,"surface":"chatgpt-desktop"}"#.as_slice(),
            MarkerRejection::Invalid,
        ),
        (
            "foreign surface",
            br#"{"schemaVersion":1,"surface":"codex-desktop"}"#.as_slice(),
            MarkerRejection::Invalid,
        ),
        (
            "unknown field",
            br#"{"schemaVersion":1,"surface":"chatgpt-desktop","adopted":true}"#.as_slice(),
            MarkerRejection::Unparseable,
        ),
    ];

    for (tamper, marker_bytes, expected) in cases {
        let directory = temporary_root();
        let profile = established_profile(&directory.path().join("profile"));
        fs::write(&profile.marker, marker_bytes).expect("tampered marker should write");
        let snapshot = persistent_snapshot(&profile);

        let error = ensure_managed_profile(&profile)
            .expect_err(&format!("{tamper} should not re-establish ownership"));
        let rejected = match expected {
            MarkerRejection::Invalid => matches!(error, ChatGptDesktopError::InvalidMarker),
            MarkerRejection::Unparseable => matches!(error, ChatGptDesktopError::ParseMarker(_)),
        };
        assert!(rejected, "{tamper} produced {error:?}");
        assert_eq!(read_file(&profile.marker), marker_bytes, "{tamper}");
        assert_preserved(&snapshot);
    }
}
