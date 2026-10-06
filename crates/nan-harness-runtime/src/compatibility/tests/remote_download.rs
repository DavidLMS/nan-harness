use super::support::{base_manifest, spawn_manifest_server};
use crate::compatibility::network::{MAX_MANIFEST_SIZE, fetch_manifest, redirect_is_allowed};
use crate::compatibility::refresh::refresh_store;
use crate::compatibility::state::{
    CompatibilityState, CompatibilityStateStore, source_fingerprint,
};
use crate::compatibility::{CompatibilityError, RefreshOutcome};
use axum::Json;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::response::Redirect;
use axum::routing::get;
use semver::Version;
use serde_json::json;
use std::convert::Infallible;
use std::net::SocketAddr;
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
    let address = spawn_manifest_server(app).await;

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
    let address = spawn_manifest_server(app).await;
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
    let address = spawn_manifest_server(app).await;
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
    let address = spawn_manifest_server(app).await;
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

fn unified_payload() -> serde_json::Value {
    json!({
        "schemaVersion": 3,
        "releases": [{
            "nanHarnessVersion": env!("CARGO_PKG_VERSION"),
            "verifications": [{
                "id": "codex",
                "lastCompatibleVersion": "0.147.0",
                "compatibleAt": "2026-09-07T08:00:00Z"
            }],
            "desktopVerifications": [{
                "id": "chatgpt-desktop",
                "platform": "macos",
                "evidence": "live-verified",
                "lastCompatibleAppVersion": "26.831.21537",
                "lastCompatibleRuntimeVersion": "0.152.0",
                "compatibleAt": "2026-09-07T08:00:00Z"
            }]
        }]
    })
}

async fn serve(payload: serde_json::Value) -> SocketAddr {
    let app = Router::new().route(
        "/compatibility-v3.json",
        get({
            let payload = Arc::new(payload);
            move || {
                let payload = Arc::clone(&payload);
                async move { Json((*payload).clone()) }
            }
        }),
    );
    spawn_manifest_server(app).await
}

/// Stores a cached feed that is old enough to require another download.
fn store_stale_cache(store: &CompatibilityStateStore, url: &str) {
    let mut state = store.load().expect("state should load");
    state.source_fingerprint = Some(source_fingerprint(url));
    state.last_checked_unix_seconds = Some(1_000);
    store.save(&state).expect("stale state should save");
}

#[tokio::test]
async fn unified_desktop_evidence_is_validated_downloaded_and_bound_to_its_source() {
    let address = serve(unified_payload()).await;
    let url = format!("http://{address}/compatibility-v3.json");
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = CompatibilityStateStore::new(directory.path());

    let outcome = refresh_store(&url, &store, &base_manifest())
        .await
        .expect("unified feed should validate and cache");

    assert_eq!(outcome, RefreshOutcome::Updated);
    let state = store.load().expect("cached state should load");
    assert!(state.matches_source(&url));
    assert!(
        !serde_json::to_string(&state)
            .expect("state should serialize")
            .contains("compatibility-v3.json"),
        "the feed URL must not be persisted"
    );
    let release = &state.cached_manifest.expect("cached manifest").releases[0];
    assert_eq!(
        release.desktop_verifications[0].last_compatible_app_version,
        Some(Version::new(26, 831, 21537))
    );
}

#[tokio::test]
async fn a_new_feed_source_is_never_answered_from_the_previous_cache() {
    let first = serve(unified_payload()).await;
    let second = serve(unified_payload()).await;
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = CompatibilityStateStore::new(directory.path());
    let first_url = format!("http://{first}/compatibility-v3.json");
    let second_url = format!("http://{second}/compatibility-v3.json");

    refresh_store(&first_url, &store, &base_manifest())
        .await
        .expect("first feed should cache");
    let outcome = refresh_store(&second_url, &store, &base_manifest())
        .await
        .expect("a changed source should be downloaded again");

    assert_eq!(outcome, RefreshOutcome::Updated);
    assert!(
        store
            .load()
            .expect("state should load")
            .matches_source(&second_url)
    );
}

#[tokio::test]
async fn invalid_downloads_never_replace_a_known_good_cache() {
    let good = serve(unified_payload()).await;
    let good_url = format!("http://{good}/compatibility-v3.json");
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = CompatibilityStateStore::new(directory.path());
    refresh_store(&good_url, &store, &base_manifest())
        .await
        .expect("known-good feed should cache");
    let cached = store.load().expect("state should load").cached_manifest;
    store_stale_cache(&store, &good_url);

    let mut broken = unified_payload();
    broken["releases"][0]["desktopVerifications"][0]["platform"] =
        serde_json::Value::String("haiku".to_owned());
    let broken_address = serve(broken).await;
    let error = refresh_store(
        &format!("http://{broken_address}/compatibility-v3.json"),
        &store,
        &base_manifest(),
    )
    .await
    .expect_err("an invalid feed must be rejected");

    assert!(matches!(
        error,
        CompatibilityError::UnknownDesktopPlatform { .. }
    ));
    assert_eq!(
        store.load().expect("state should load").cached_manifest,
        cached
    );
}

#[tokio::test]
async fn offline_refreshes_retain_the_cached_evidence() {
    let good = serve(unified_payload()).await;
    let good_url = format!("http://{good}/compatibility-v3.json");
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = CompatibilityStateStore::new(directory.path());
    refresh_store(&good_url, &store, &base_manifest())
        .await
        .expect("known-good feed should cache");
    let cached = store.load().expect("state should load").cached_manifest;
    store_stale_cache(&store, &good_url);

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("listener should bind");
    let closed = listener.local_addr().expect("listener address");
    drop(listener);
    let error = refresh_store(
        &format!("http://{closed}/compatibility-v3.json"),
        &store,
        &base_manifest(),
    )
    .await
    .expect_err("an offline refresh must fail");

    assert!(matches!(error, CompatibilityError::FetchManifest(_)));
    assert_eq!(
        store.load().expect("state should load").cached_manifest,
        cached
    );
}

#[test]
fn a_cache_from_another_source_is_never_considered_fresh() {
    let state = CompatibilityState {
        schema_version: 4,
        source_fingerprint: Some(source_fingerprint(
            "https://example.com/compatibility-v3.json",
        )),
        last_checked_unix_seconds: Some(1_000),
        cached_manifest: Some(crate::compatibility::VerificationManifest {
            schema_version: 3,
            releases: Vec::new(),
        }),
    };

    assert!(!crate::compatibility::state::cache_is_fresh_at(
        &state,
        "https://example.com/other-feed.json",
        1_100
    ));
}

#[tokio::test]
async fn an_unreadable_cache_is_replaced_by_one_download_instead_of_blocking_refresh() {
    let address = serve(unified_payload()).await;
    let url = format!("http://{address}/compatibility-v3.json");
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = CompatibilityStateStore::new(directory.path());
    for corrupt in [
        b"{ this is not json".to_vec(),
        br#"{"schemaVersion":99,"lastCheckedUnixSeconds":null,"cachedManifest":null}"#.to_vec(),
    ] {
        std::fs::write(
            directory
                .path()
                .join(crate::compatibility::state::STATE_FILE_NAME),
            corrupt,
        )
        .expect("corrupt cache should be written");

        let outcome = refresh_store(&url, &store, &base_manifest())
            .await
            .expect("an unreadable cache must not block the refresh");

        assert_eq!(outcome, RefreshOutcome::Updated);
        assert!(
            store
                .load()
                .expect("state should load")
                .matches_source(&url)
        );
    }
}

#[tokio::test]
async fn an_unreadable_cache_is_preserved_when_the_download_fails() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = CompatibilityStateStore::new(directory.path());
    let path = directory
        .path()
        .join(crate::compatibility::state::STATE_FILE_NAME);
    std::fs::write(&path, b"{ this is not json").expect("corrupt cache should be written");
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("listener should bind");
    let closed = listener.local_addr().expect("listener address");
    drop(listener);

    let error = refresh_store(
        &format!("http://{closed}/compatibility-v3.json"),
        &store,
        &base_manifest(),
    )
    .await
    .expect_err("an offline refresh must fail");

    assert!(matches!(error, CompatibilityError::FetchManifest(_)));
    assert_eq!(
        std::fs::read(&path).expect("the cache file should remain"),
        b"{ this is not json"
    );
}
