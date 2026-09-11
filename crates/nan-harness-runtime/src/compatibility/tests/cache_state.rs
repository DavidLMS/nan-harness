use super::support::base_manifest;
use crate::compatibility::refresh::refresh_store;
use crate::compatibility::state::{
    CompatibilityState, CompatibilityStateStore, STATE_FILE_NAME, cache_is_fresh,
    cache_is_fresh_at, source_fingerprint,
};
use crate::compatibility::{
    CompatibilityError, VerificationEntry, VerificationManifest, VerificationRelease,
};
use semver::Version;

const FEED_URL: &str = "https://example.com/compatibility-v3.json";

#[test]
fn cache_replacement_is_private_and_preserves_an_open_reader() {
    use nan_harness_private_fs::{PrivateFileReadStatus, open_private_read};
    use std::io::Read as _;

    let directory = tempfile::tempdir().expect("temporary directory");
    let store = CompatibilityStateStore::new(directory.path());
    let mut state = CompatibilityState::default();
    store
        .save(&state)
        .expect("first cache write should succeed");
    let path = directory.path().join(STATE_FILE_NAME);
    let previous = std::fs::read(&path).expect("original cache should be readable");
    let (mut reader, privacy) = open_private_read(&path).expect("cache should open privately");
    assert_eq!(privacy, PrivateFileReadStatus::AlreadyPrivate);

    state.last_checked_unix_seconds = Some(42);
    store
        .save(&state)
        .expect("cache should replace an open destination");
    assert_eq!(store.load().expect("new cache should load"), state);
    let (_, privacy) = open_private_read(&path).expect("replacement should open privately");
    assert_eq!(privacy, PrivateFileReadStatus::AlreadyPrivate);
    let mut snapshot = Vec::new();
    reader
        .read_to_end(&mut snapshot)
        .expect("old reader should retain its snapshot");
    assert_eq!(snapshot, previous);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn failed_cache_replacement_preserves_the_destination_and_cleans_staging() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let destination = directory.path().join(STATE_FILE_NAME);
    std::fs::create_dir(&destination).expect("obstruction should exist");
    let sentinel = destination.join("user-owned");
    std::fs::write(&sentinel, b"preserve me").unwrap();
    let store = CompatibilityStateStore::new(directory.path());
    assert!(matches!(
        store.save(&CompatibilityState::default()),
        Err(CompatibilityError::WriteState(_))
    ));
    assert_eq!(std::fs::read(sentinel).unwrap(), b"preserve me");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn cache_state_round_trips() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = CompatibilityStateStore::new(directory.path());
    let state = CompatibilityState {
        schema_version: 3,
        source_fingerprint: Some(source_fingerprint(FEED_URL)),
        last_checked_unix_seconds: Some(42),
        cached_manifest: Some(VerificationManifest {
            schema_version: 2,
            releases: vec![VerificationRelease {
                nan_harness_version: Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
                verifications: vec![VerificationEntry {
                    id: "claude-code".to_owned(),
                    last_compatible_version: Some(Version::new(2, 1, 234)),
                    compatible_at: Some("2026-08-19T00:00:00Z".to_owned()),
                    last_live_verified_version: None,
                    live_verified_at: None,
                }],
                desktop_verifications: Vec::new(),
            }],
        }),
    };
    store.save(&state).expect("state should save");
    assert_eq!(store.load().expect("state should load"), state);
}

#[tokio::test]
async fn state_read_errors_are_returned_instead_of_resetting_state() {
    let directory = tempfile::tempdir().expect("temporary directory");
    std::fs::create_dir(directory.path().join(STATE_FILE_NAME))
        .expect("state path fixture should be created");
    let store = CompatibilityStateStore::new(directory.path());

    let error = refresh_store(FEED_URL, &store, &base_manifest())
        .await
        .expect_err("state read errors must be returned");
    assert!(matches!(error, CompatibilityError::ReadState(_)));
}

#[test]
fn future_cache_timestamps_are_not_fresh() {
    let state = CompatibilityState {
        schema_version: 3,
        source_fingerprint: Some(source_fingerprint(FEED_URL)),
        last_checked_unix_seconds: Some(u64::MAX),
        cached_manifest: Some(VerificationManifest {
            schema_version: 2,
            releases: Vec::new(),
        }),
    };

    assert!(!cache_is_fresh(&state, FEED_URL));
}

#[test]
fn compatibility_cache_expires_after_one_hour() {
    let state = CompatibilityState {
        schema_version: 3,
        source_fingerprint: Some(source_fingerprint(FEED_URL)),
        last_checked_unix_seconds: Some(1_000),
        cached_manifest: Some(VerificationManifest {
            schema_version: 2,
            releases: vec![VerificationRelease {
                nan_harness_version: Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
                verifications: vec![],
                desktop_verifications: Vec::new(),
            }],
        }),
    };

    assert!(cache_is_fresh_at(&state, FEED_URL, 4_599));
    assert!(!cache_is_fresh_at(&state, FEED_URL, 4_600));
}
