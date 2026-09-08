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
fn cache_state_round_trips() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = CompatibilityStateStore::new(directory.path());
    let state = CompatibilityState {
        schema_version: 4,
        source_fingerprint: Some(source_fingerprint(FEED_URL)),
        last_checked_unix_seconds: Some(42),
        cached_manifest: Some(VerificationManifest {
            schema_version: 2,
            releases: vec![VerificationRelease {
                desktop_checks: Vec::new(),
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

#[test]
fn exact_version_cache_does_not_overwrite_older_client_state() {
    let directory = tempfile::tempdir().unwrap();
    let old = directory.path().join("compatibility-v3.json");
    std::fs::write(&old, b"synthetic old cache").unwrap();
    let store = CompatibilityStateStore::new(directory.path());
    store.save(&CompatibilityState::default()).unwrap();
    assert_eq!(std::fs::read(old).unwrap(), b"synthetic old cache");
    assert!(directory.path().join("compatibility-v4.json").is_file());
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
        schema_version: 4,
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
        schema_version: 4,
        source_fingerprint: Some(source_fingerprint(FEED_URL)),
        last_checked_unix_seconds: Some(1_000),
        cached_manifest: Some(VerificationManifest {
            schema_version: 2,
            releases: vec![VerificationRelease {
                desktop_checks: Vec::new(),
                nan_harness_version: Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
                verifications: vec![],
                desktop_verifications: Vec::new(),
            }],
        }),
    };

    assert!(cache_is_fresh_at(&state, FEED_URL, 4_599));
    assert!(!cache_is_fresh_at(&state, FEED_URL, 4_600));
}
