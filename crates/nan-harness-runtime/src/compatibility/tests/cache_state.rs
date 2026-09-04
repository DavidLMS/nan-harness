// Cached state and expiry: the on-disk state round-trips, an unreadable state
// file is reported instead of being reset, and a cached feed stops being fresh
// after one hour or when its timestamp is in the future.
use super::support::base_manifest;
use crate::compatibility::refresh::refresh_store;
use crate::compatibility::state::{
    CompatibilityState, CompatibilityStateStore, STATE_FILE_NAME, cache_is_fresh, cache_is_fresh_at,
};
use crate::compatibility::{
    CompatibilityError, VerificationEntry, VerificationManifest, VerificationRelease,
};
use semver::Version;

#[test]
fn cache_state_round_trips() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = CompatibilityStateStore::new(directory.path());
    let state = CompatibilityState {
        schema_version: 2,
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

    let error = refresh_store(
        "https://example.com/compatibility.json",
        &store,
        &base_manifest(),
    )
    .await
    .expect_err("state read errors must be returned");
    assert!(matches!(error, CompatibilityError::ReadState(_)));
}

#[test]
fn future_cache_timestamps_are_not_fresh() {
    let state = CompatibilityState {
        schema_version: 2,
        last_checked_unix_seconds: Some(u64::MAX),
        cached_manifest: Some(VerificationManifest {
            schema_version: 2,
            releases: Vec::new(),
        }),
    };

    assert!(!cache_is_fresh(&state));
}

#[test]
fn compatibility_cache_expires_after_one_hour() {
    let state = CompatibilityState {
        schema_version: 2,
        last_checked_unix_seconds: Some(1_000),
        cached_manifest: Some(VerificationManifest {
            schema_version: 2,
            releases: vec![VerificationRelease {
                nan_harness_version: Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
                verifications: vec![],
            }],
        }),
    };

    assert!(cache_is_fresh_at(&state, 4_599));
    assert!(!cache_is_fresh_at(&state, 4_600));
}
