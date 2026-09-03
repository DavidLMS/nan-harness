use super::evidence::{apply_verifications, merge_evidence_pair, select_release};
use super::network::{MAX_MANIFEST_SIZE, fetch_manifest, redirect_is_allowed};
use super::refresh::refresh_store;
use super::state::{
    CompatibilityState, CompatibilityStateStore, STATE_FILE_NAME, cache_is_fresh, cache_is_fresh_at,
};
use super::validation::validate_manifest;
use super::{
    CompatibilityError, RefreshOutcome, VerificationEntry, VerificationManifest,
    VerificationRelease,
};
use axum::Json;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::response::Redirect;
use axum::routing::get;
use nan_harness_core::{CompatibilityManifest, HarnessKind};
use semver::Version;
use serde_json::json;
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::net::TcpListener;
use url::Url;

#[test]
fn redirects_allow_only_expected_https_origins() {
    let github = Url::parse(
        "https://github.com/DavidLMS/nan-harness/releases/download/compatibility/compatibility.json",
    )
    .expect("GitHub URL");
    let release_asset = Url::parse(
        "https://release-assets.githubusercontent.com/github-production-release-asset/file",
    )
    .expect("release asset URL");
    let same_origin = Url::parse(
        "https://github.com/DavidLMS/nan-harness/releases/download/compatibility/feed.json",
    )
    .expect("same-origin URL");

    assert!(redirect_is_allowed(&github, &release_asset));
    assert!(redirect_is_allowed(&github, &same_origin));
    for rejected in [
        "http://release-assets.githubusercontent.com/file",
        "https://user@release-assets.githubusercontent.com/file",
        "https://release-assets.githubusercontent.com.evil.example/file",
        "https://objects.githubusercontent.com/file",
    ] {
        let rejected = Url::parse(rejected).expect("rejected URL should parse");
        assert!(!redirect_is_allowed(&github, &rejected), "{rejected}");
    }
}

#[test]
fn overlay_only_advances_known_compatible_versions() {
    let mut base = base_manifest();
    let original_policy = base.policy.clone();
    let original_minimum = base
        .entry(HarnessKind::Codex)
        .expect("Codex entry")
        .minimum_version
        .clone();
    let remote = VerificationManifest {
        schema_version: 2,
        releases: vec![VerificationRelease {
            nan_harness_version: Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
            verifications: vec![VerificationEntry {
                id: "codex".to_owned(),
                last_compatible_version: Some(Version::new(0, 147, 0)),
                compatible_at: Some("2026-08-19T08:00:00Z".to_owned()),
                last_live_verified_version: None,
                live_verified_at: None,
            }],
        }],
    };

    apply_verifications(&mut base, &remote.releases[0]).expect("overlay should apply");

    let codex = base.entry(HarnessKind::Codex).expect("Codex entry");
    assert_eq!(codex.last_compatible_version, Version::new(0, 147, 0));
    assert_eq!(codex.minimum_version, original_minimum);
    assert_eq!(base.policy, original_policy);
}

#[test]
fn overlay_never_regresses_the_embedded_compatible_version() {
    let mut base = base_manifest();
    let remote = VerificationRelease {
        nan_harness_version: Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
        verifications: vec![VerificationEntry {
            id: "codex".to_owned(),
            last_compatible_version: Some(Version::new(0, 145, 0)),
            compatible_at: Some("2026-08-19T08:00:00Z".to_owned()),
            last_live_verified_version: None,
            live_verified_at: None,
        }],
    };

    apply_verifications(&mut base, &remote).expect("overlay should apply");

    assert_eq!(
        base.entry(HarnessKind::Codex)
            .expect("Codex entry")
            .last_compatible_version,
        Version::new(0, 146, 0)
    );
}

#[test]
fn malformed_and_missing_evidence_pairs_are_rejected() {
    let base = base_manifest();
    let cases = [
        (
            VerificationEntry {
                id: "codex".to_owned(),
                last_compatible_version: Some(Version::new(0, 147, 0)),
                compatible_at: None,
                last_live_verified_version: None,
                live_verified_at: None,
            },
            "incomplete compatible pair",
        ),
        (
            VerificationEntry {
                id: "codex".to_owned(),
                last_compatible_version: None,
                compatible_at: None,
                last_live_verified_version: Some(Version::new(0, 147, 0)),
                live_verified_at: None,
            },
            "incomplete live pair",
        ),
        (
            VerificationEntry {
                id: "codex".to_owned(),
                last_compatible_version: None,
                compatible_at: None,
                last_live_verified_version: None,
                live_verified_at: None,
            },
            "missing evidence",
        ),
        (
            VerificationEntry {
                id: "codex".to_owned(),
                last_compatible_version: Some(Version::new(0, 147, 0)),
                compatible_at: Some("2026-08-19".to_owned()),
                last_live_verified_version: None,
                live_verified_at: None,
            },
            "malformed timestamp",
        ),
    ];

    for (entry, label) in cases {
        let manifest = feed_for(entry);
        assert!(validate_manifest(&manifest, &base).is_err(), "{label}");
    }
}

#[test]
fn evidence_versions_below_minimum_are_rejected() {
    let base = base_manifest();
    for entry in [
        VerificationEntry {
            id: "codex".to_owned(),
            last_compatible_version: Some(Version::new(0, 145, 0)),
            compatible_at: Some("2026-08-19T08:00:00Z".to_owned()),
            last_live_verified_version: None,
            live_verified_at: None,
        },
        VerificationEntry {
            id: "codex".to_owned(),
            last_compatible_version: None,
            compatible_at: None,
            last_live_verified_version: Some(Version::new(0, 145, 0)),
            live_verified_at: Some("2026-08-19T08:00:00Z".to_owned()),
        },
    ] {
        assert!(matches!(
            validate_manifest(&feed_for(entry), &base),
            Err(CompatibilityError::VersionBelowMinimum { .. }
                | CompatibilityError::LiveVersionBelowMinimum { .. },)
        ));
    }
}

#[test]
fn duplicate_releases_and_known_harnesses_are_rejected() {
    let current = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
    let valid = VerificationEntry {
        id: "codex".to_owned(),
        last_compatible_version: Some(Version::new(0, 147, 0)),
        compatible_at: Some("2026-08-19T08:00:00Z".to_owned()),
        last_live_verified_version: None,
        live_verified_at: None,
    };
    let duplicate_release = VerificationManifest {
        schema_version: 2,
        releases: vec![
            VerificationRelease {
                nan_harness_version: current.clone(),
                verifications: vec![valid.clone()],
            },
            VerificationRelease {
                nan_harness_version: current,
                verifications: vec![valid.clone()],
            },
        ],
    };
    assert!(matches!(
        validate_manifest(&duplicate_release, &base_manifest()),
        Err(CompatibilityError::DuplicateRelease(_))
    ));

    let duplicate_harness = feed_for_entries(vec![valid.clone(), valid]);
    assert!(matches!(
        validate_manifest(&duplicate_harness, &base_manifest()),
        Err(CompatibilityError::DuplicateHarness(HarnessKind::Codex))
    ));
}

#[test]
fn live_evidence_cannot_outpace_compatible_evidence_without_the_same_update() {
    let base = base_manifest();
    let ahead = VerificationEntry {
        id: "codex".to_owned(),
        last_compatible_version: Some(Version::new(0, 146, 0)),
        compatible_at: Some("2026-08-19T08:00:00Z".to_owned()),
        last_live_verified_version: Some(Version::new(0, 147, 0)),
        live_verified_at: Some("2026-08-20T08:00:00Z".to_owned()),
    };
    assert!(matches!(
        validate_manifest(&feed_for(ahead), &base),
        Err(CompatibilityError::LiveEvidenceAhead { .. })
    ));

    let advanced_together = VerificationEntry {
        id: "codex".to_owned(),
        last_compatible_version: Some(Version::new(0, 147, 0)),
        compatible_at: Some("2026-08-19T08:00:00Z".to_owned()),
        last_live_verified_version: Some(Version::new(0, 147, 0)),
        live_verified_at: Some("2026-08-20T08:00:00Z".to_owned()),
    };
    assert!(validate_manifest(&feed_for(advanced_together), &base).is_ok());
}

#[test]
fn unknown_future_harness_ids_are_validated_then_ignored() {
    let mut base = base_manifest();
    let before = base.clone();
    let unknown = VerificationEntry {
        id: "future-harness".to_owned(),
        last_compatible_version: Some(Version::new(99, 0, 0)),
        compatible_at: Some("2026-08-20T00:00:00Z".to_owned()),
        last_live_verified_version: None,
        live_verified_at: None,
    };
    let feed = feed_for(unknown);
    assert!(validate_manifest(&feed, &base).is_ok());
    apply_verifications(&mut base, &feed.releases[0]).expect("unknown IDs are ignored");
    assert_eq!(base, before);
}

#[test]
fn evidence_pairs_merge_atomically_using_version_then_real_timestamp_order() {
    let mut version = Some(Version::new(0, 146, 0));
    let mut timestamp = Some("2026-08-20T00:00:00Z".to_owned());
    merge_evidence_pair(
        &mut version,
        &mut timestamp,
        Some(&Version::new(0, 147, 0)),
        Some(&"2026-08-19T00:00:00Z".to_owned()),
        "codex",
        "compatible",
    )
    .expect("higher version should merge while preserving newer evidence");
    assert_eq!(version, Some(Version::new(0, 147, 0)));
    assert_eq!(timestamp.as_deref(), Some("2026-08-20T00:00:00Z"));

    timestamp = Some("2026-08-20T00:30:00+01:00".to_owned());
    merge_evidence_pair(
        &mut version,
        &mut timestamp,
        Some(&Version::new(0, 147, 0)),
        Some(&"2026-08-20T00:00:00Z".to_owned()),
        "codex",
        "compatible",
    )
    .expect("equal version with later instant should merge");
    assert_eq!(timestamp.as_deref(), Some("2026-08-20T00:00:00Z"));

    merge_evidence_pair(
        &mut version,
        &mut timestamp,
        Some(&Version::new(0, 147, 0)),
        Some(&"2026-08-20T01:00:00+01:00".to_owned()),
        "codex",
        "compatible",
    )
    .expect("equal version with same instant should be unchanged");
    assert_eq!(timestamp.as_deref(), Some("2026-08-20T00:00:00Z"));

    merge_evidence_pair(
        &mut version,
        &mut timestamp,
        Some(&Version::new(0, 146, 0)),
        Some(&"2026-08-21T00:00:00Z".to_owned()),
        "codex",
        "compatible",
    )
    .expect("lower version should be ignored");
    assert_eq!(version, Some(Version::new(0, 147, 0)));
    assert_eq!(timestamp.as_deref(), Some("2026-08-20T00:00:00Z"));

    merge_evidence_pair(
        &mut version,
        &mut timestamp,
        Some(&Version::new(0, 147, 0)),
        Some(&"2026-08-19T00:00:00Z".to_owned()),
        "codex",
        "compatible",
    )
    .expect("equal version with an older timestamp should be ignored");
    assert_eq!(timestamp.as_deref(), Some("2026-08-20T00:00:00Z"));

    let mut absent_version = None;
    let mut stray_timestamp = Some("2026-08-21T00:00:00Z".to_owned());
    merge_evidence_pair(
        &mut absent_version,
        &mut stray_timestamp,
        None,
        Some(&"2026-08-22T00:00:00Z".to_owned()),
        "codex",
        "compatible",
    )
    .expect("an update without a version should be ignored");
    assert_eq!(absent_version, None);
    assert_eq!(stray_timestamp.as_deref(), Some("2026-08-21T00:00:00Z"));

    let mut incomplete_version = Some(Version::new(0, 146, 0));
    let mut incomplete_timestamp = None;
    assert!(matches!(
        merge_evidence_pair(
            &mut incomplete_version,
            &mut incomplete_timestamp,
            Some(&Version::new(0, 147, 0)),
            Some(&"2026-08-22T00:00:00Z".to_owned()),
            "codex",
            "compatible",
        ),
        Err(CompatibilityError::IncompleteEvidencePair { .. })
    ));
}

#[test]
fn lower_remote_evidence_preserves_the_entire_embedded_record() {
    let mut base = base_manifest();
    let before = base.clone();
    let release = VerificationRelease {
        nan_harness_version: Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
        verifications: vec![VerificationEntry {
            id: "codex".to_owned(),
            last_compatible_version: Some(Version::new(0, 145, 0)),
            compatible_at: Some("2026-08-21T00:00:00Z".to_owned()),
            last_live_verified_version: Some(Version::new(0, 145, 0)),
            live_verified_at: Some("2026-08-21T00:00:00Z".to_owned()),
        }],
    };
    apply_verifications(&mut base, &release).expect("overlay should apply");
    assert_eq!(base, before);
}

#[test]
fn release_selection_is_exact_and_unknown_harnesses_are_ignored() {
    let mut base = base_manifest();
    let current = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
    let feed = VerificationManifest {
        schema_version: 2,
        releases: vec![
            VerificationRelease {
                nan_harness_version: current.clone(),
                verifications: vec![
                    VerificationEntry {
                        id: "codex".to_owned(),
                        last_compatible_version: Some(Version::new(0, 147, 0)),
                        compatible_at: Some("2026-08-19T08:00:00Z".to_owned()),
                        last_live_verified_version: None,
                        live_verified_at: None,
                    },
                    VerificationEntry {
                        id: "unknown-harness".to_owned(),
                        last_compatible_version: Some(Version::new(99, 0, 0)),
                        compatible_at: Some("2026-08-19T08:00:00Z".to_owned()),
                        last_live_verified_version: None,
                        live_verified_at: None,
                    },
                ],
            },
            VerificationRelease {
                nan_harness_version: Version::new(99, 0, 0),
                verifications: vec![VerificationEntry {
                    id: "unknown-harness".to_owned(),
                    last_compatible_version: Some(Version::new(99, 0, 0)),
                    compatible_at: Some("2026-08-19T08:00:00Z".to_owned()),
                    last_live_verified_version: None,
                    live_verified_at: None,
                }],
            },
        ],
    };

    let release = select_release(&feed, &current).expect("current release should match");
    apply_verifications(&mut base, release).expect("overlay should apply");
    assert_eq!(
        base.entry(HarnessKind::Codex)
            .expect("Codex entry")
            .last_compatible_version,
        Version::new(0, 147, 0)
    );
    assert!(select_release(&feed, &Version::new(1, 0, 0)).is_none());
}

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
fn empty_release_lists_are_rejected() {
    let manifest = VerificationManifest {
        schema_version: 2,
        releases: Vec::new(),
    };

    assert!(matches!(
        validate_manifest(&manifest, &base_manifest()),
        Err(CompatibilityError::EmptyReleases)
    ));
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

#[tokio::test]
async fn remote_manifest_is_validated_and_downloaded() {
    let payload = json!({
        "schemaVersion": 2,
        "releases": [{
            "nanHarnessVersion": env!("CARGO_PKG_VERSION"),
            "verifications": [{
                "id": "codex",
                "lastCompatibleVersion": "0.147.0",
                "compatibleAt": "2026-08-19T08:00:00Z"
            }]
        }]
    });
    let app = Router::new().route(
        "/compatibility.json",
        get({
            let payload = Arc::new(payload);
            move || {
                let payload = Arc::clone(&payload);
                async move { Json((*payload).clone()) }
            }
        }),
    );
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("listener should bind");
    let address = listener.local_addr().expect("listener address");
    tokio::spawn(axum::serve(listener, app).into_future());

    let url = format!("http://{address}/compatibility.json");
    let base = base_manifest();
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = CompatibilityStateStore::new(directory.path());
    let outcome = refresh_store(&url, &store, &base)
        .await
        .expect("remote manifest should validate and cache");
    assert_eq!(outcome, RefreshOutcome::Updated);
    let state = store.load().expect("cached state should load");
    assert_eq!(
        state.cached_manifest.expect("cached manifest").releases[0].verifications[0]
            .last_compatible_version,
        Some(Version::new(0, 147, 0))
    );
    assert_eq!(
        refresh_store(&url, &store, &base)
            .await
            .expect("fresh cache should be reused"),
        RefreshOutcome::Cached
    );
}

#[tokio::test]
async fn empty_remote_release_lists_are_rejected_without_caching() {
    let app = Router::new().route(
        "/compatibility.json",
        get(|| async {
            Json(json!({
                "schemaVersion": 2,
                "releases": []
            }))
        }),
    );
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("listener should bind");
    let address = listener.local_addr().expect("listener address");
    tokio::spawn(axum::serve(listener, app).into_future());
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = CompatibilityStateStore::new(directory.path());

    let error = refresh_store(
        &format!("http://{address}/compatibility.json"),
        &store,
        &base_manifest(),
    )
    .await
    .expect_err("an empty remote feed should be rejected");

    assert!(matches!(error, CompatibilityError::EmptyReleases));
    let state = store.load().expect("state should remain readable");
    assert!(state.cached_manifest.is_none());
    assert!(state.last_checked_unix_seconds.is_none());
}

#[tokio::test]
async fn remote_manifest_redirects_are_not_followed() {
    let target_reached = Arc::new(AtomicBool::new(false));
    let app = Router::new()
        .route(
            "/redirect",
            get(|| async { Redirect::temporary("/compatibility.json") }),
        )
        .route(
            "/compatibility.json",
            get({
                let target_reached = Arc::clone(&target_reached);
                move || {
                    let target_reached = Arc::clone(&target_reached);
                    async move {
                        target_reached.store(true, Ordering::SeqCst);
                        Json(json!({
                            "schemaVersion": 2,
                            "releases": []
                        }))
                    }
                }
            }),
        );
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("listener should bind");
    let address = listener.local_addr().expect("listener address");
    tokio::spawn(axum::serve(listener, app).into_future());
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = CompatibilityStateStore::new(directory.path());

    let error = refresh_store(
        &format!("http://{address}/redirect"),
        &store,
        &base_manifest(),
    )
    .await
    .expect_err("redirect should be rejected");

    assert!(matches!(error, CompatibilityError::ManifestStatus(307)));
    assert!(!target_reached.load(Ordering::SeqCst));
}

#[tokio::test]
async fn remote_manifest_stream_is_bounded_while_downloading() {
    let app = Router::new().route(
        "/compatibility.json",
        get(|| async {
            let chunks = vec![
                Ok::<_, Infallible>(Bytes::from(vec![b' '; MAX_MANIFEST_SIZE / 2])),
                Ok(Bytes::from(vec![b' '; MAX_MANIFEST_SIZE / 2])),
                Ok(Bytes::from_static(b" ")),
            ];
            axum::response::Response::new(Body::from_stream(futures_util::stream::iter(chunks)))
        }),
    );
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("listener should bind");
    let address = listener.local_addr().expect("listener address");
    tokio::spawn(axum::serve(listener, app).into_future());
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = CompatibilityStateStore::new(directory.path());

    let error = refresh_store(
        &format!("http://{address}/compatibility.json"),
        &store,
        &base_manifest(),
    )
    .await
    .expect_err("oversized stream should be rejected");

    assert!(matches!(error, CompatibilityError::ManifestTooLarge));
}

#[tokio::test]
async fn remote_manifest_errors_do_not_retain_the_request_url() {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("listener should bind");
    let address = listener.local_addr().expect("listener address");
    drop(listener);

    let error = fetch_manifest(
        &format!("http://{address}/compatibility.json?token=nan-secret"),
        &base_manifest(),
    )
    .await
    .expect_err("closed endpoint should fail");

    let CompatibilityError::FetchManifest(source) = &error else {
        panic!("expected a fetch error, received {error}");
    };
    assert!(source.url().is_none());
    assert!(!error.to_string().contains("nan-secret"));
}

fn base_manifest() -> CompatibilityManifest {
    crate::discovery::bundled_compatibility_manifest().expect("embedded manifest")
}

fn feed_for(entry: VerificationEntry) -> VerificationManifest {
    feed_for_entries(vec![entry])
}

fn feed_for_entries(entries: Vec<VerificationEntry>) -> VerificationManifest {
    VerificationManifest {
        schema_version: 2,
        releases: vec![VerificationRelease {
            nan_harness_version: Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
            verifications: entries,
        }],
    }
}
