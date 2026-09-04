// Remote download: a valid feed is validated then cached, the redirect policy
// accepts only the expected https origins, a disallowed redirect is never
// followed, the response stream is bounded, a rejected feed is never cached,
// and failures keep the request URL out of diagnostics.
use super::support::{base_manifest, spawn_manifest_server};
use crate::compatibility::network::{MAX_MANIFEST_SIZE, fetch_manifest, redirect_is_allowed};
use crate::compatibility::refresh::refresh_store;
use crate::compatibility::state::CompatibilityStateStore;
use crate::compatibility::{CompatibilityError, RefreshOutcome};
use axum::Json;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::response::Redirect;
use axum::routing::get;
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
