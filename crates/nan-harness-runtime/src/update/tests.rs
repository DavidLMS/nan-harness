use super::candidate::{hex_digest, verify_candidate};
use super::manifest::{MAX_MANIFEST_SIZE, current_target};
use super::state::UpdateStateStore;
use super::{ReleaseArtifact, ReleaseManifest, UpdateManager};
use axum::Router;
use axum::body::Body;
use axum::http::{Response, StatusCode};
use axum::routing::get;
use semver::Version;
use sha2::{Digest as _, Sha256};
use std::sync::Arc;

#[tokio::test]
async fn skipped_release_returns_only_when_a_newer_version_exists() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let manifest = manifest("0.2.0", "https://example.com/nan");
    let server = manifest_server(manifest.clone()).await;
    let manager = UpdateManager::new(
        "0.1.0",
        Some(format!("{server}/manifest.json")),
        None,
        UpdateStateStore::new(directory.path()),
    )
    .expect("manager should build");

    let available = manager
        .recommended_release(true, true)
        .await
        .expect("release should load")
        .expect("release should be newer");
    assert_eq!(available.version, Version::new(0, 2, 0));

    manager
        .skip(available.version.clone())
        .expect("skip should persist");
    assert!(
        manager
            .recommended_release(false, true)
            .await
            .expect("cached release should load")
            .is_none()
    );
    assert!(
        manager
            .recommended_release(false, false)
            .await
            .expect("manual check should load")
            .is_some()
    );
}

#[tokio::test]
async fn current_release_is_not_offered() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let server = manifest_server(manifest("0.1.0", "https://example.com/nan")).await;
    let manager = UpdateManager::new(
        "0.1.0",
        Some(format!("{server}/manifest.json")),
        None,
        UpdateStateStore::new(directory.path()),
    )
    .expect("manager should build");

    assert!(
        manager
            .recommended_release(true, true)
            .await
            .expect("release should load")
            .is_none()
    );
}

#[tokio::test]
async fn state_read_errors_are_returned_instead_of_resetting_state() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    std::fs::create_dir(directory.path().join("update.json"))
        .expect("state path fixture should be created");
    let manager = UpdateManager::new(
        "0.1.0",
        Some("https://example.com/manifest.json".to_owned()),
        None,
        UpdateStateStore::new(directory.path()),
    )
    .expect("manager should build");

    assert!(matches!(
        manager.recommended_release(true, true).await,
        Err(super::UpdateError::ReadState(_))
    ));
    assert!(matches!(
        manager.skip(Version::new(0, 2, 0)),
        Err(super::UpdateError::ReadState(_))
    ));
}

#[tokio::test]
async fn oversized_release_manifests_are_rejected() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let server = serve(Router::new().route(
        "/manifest.json",
        get(|| async { vec![b'x'; MAX_MANIFEST_SIZE + 1] }),
    ))
    .await;
    let manager = UpdateManager::new(
        "0.1.0",
        Some(format!("{server}/manifest.json")),
        None,
        UpdateStateStore::new(directory.path()),
    )
    .expect("manager should build");

    let error = manager
        .recommended_release(true, true)
        .await
        .expect_err("oversized manifests must be rejected");
    assert!(matches!(error, super::UpdateError::ManifestTooLarge));
}

#[cfg(unix)]
#[tokio::test]
async fn downloads_and_verifies_an_executable_candidate() {
    use std::os::unix::fs::PermissionsExt as _;

    let binary = b"#!/bin/sh\nprintf '%s\\n' 'nan-harness 0.2.0'\n".to_vec();
    let checksum = hex_digest(Sha256::digest(&binary));
    let binary = Arc::new(binary);
    let binary_server = {
        let binary = Arc::clone(&binary);
        serve(Router::new().route(
            "/nan",
            get(move || {
                let binary = Arc::clone(&binary);
                async move {
                    Response::builder()
                        .status(StatusCode::OK)
                        .body(Body::from(binary.as_ref().clone()))
                        .expect("response should build")
                }
            }),
        ))
        .await
    };
    let release = ReleaseManifest {
        schema_version: 1,
        version: Version::new(0, 2, 0),
        notes_url: "https://example.com/notes".to_owned(),
        artifacts: vec![ReleaseArtifact {
            target: current_target().to_owned(),
            url: format!("{binary_server}/nan"),
            sha256: checksum,
        }],
    };
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let manager = UpdateManager::new(
        "0.1.0",
        Some("https://example.com/manifest.json".to_owned()),
        None,
        UpdateStateStore::new(directory.path()),
    )
    .expect("manager should build");

    let candidate = super::artifact::download(&manager.client, &release.artifacts[0])
        .await
        .expect("candidate should download");
    let candidate_path: &std::path::Path = candidate.as_ref();
    assert_eq!(
        std::fs::metadata(candidate_path)
            .expect("metadata should exist")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    verify_candidate(candidate_path, &release.version)
        .expect("candidate should report the expected version");
}

#[tokio::test]
async fn explicit_updates_see_a_published_release_that_startup_discovery_must_not_offer() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let recommended = manifest_server(manifest("0.2.0", "https://example.com/nan")).await;
    let published = manifest_server(manifest("0.3.0", "https://example.com/nan")).await;
    let manager = UpdateManager::new(
        "0.1.0",
        Some(format!("{recommended}/manifest.json")),
        Some(format!("{published}/manifest.json")),
        UpdateStateStore::new(directory.path()),
    )
    .expect("manager should build");

    let startup = manager
        .recommended_release(true, true)
        .await
        .expect("recommended release should load")
        .expect("recommended release should be newer");
    let manual = manager
        .available_release()
        .await
        .expect("published release should load")
        .expect("published release should be newer");

    assert_eq!(startup.version, Version::new(0, 2, 0));
    assert_eq!(manual.version, Version::new(0, 3, 0));
}

#[tokio::test]
async fn an_explicit_update_neither_reads_nor_writes_startup_state() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let published = manifest_server(manifest("0.3.0", "https://example.com/nan")).await;
    let manager = UpdateManager::new(
        "0.1.0",
        Some("https://example.com/manifest.json".to_owned()),
        Some(format!("{published}/manifest.json")),
        UpdateStateStore::new(directory.path()),
    )
    .expect("manager should build");
    manager
        .skip(Version::new(0, 2, 0))
        .expect("skip should persist");

    let manual = manager
        .available_release()
        .await
        .expect("published release should load")
        .expect("published release should be newer");

    let state = std::fs::read_to_string(directory.path().join("update.json"))
        .expect("updater state should exist");
    assert_eq!(manual.version, Version::new(0, 3, 0));
    assert!(state.contains("\"schemaVersion\": 1"));
    assert!(state.contains("\"skippedVersion\": \"0.2.0\""));
    assert!(state.contains("\"cachedRelease\": null"));
}

#[tokio::test]
async fn a_client_installed_ahead_of_both_sources_is_never_downgraded() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let recommended = manifest_server(manifest("0.2.0", "https://example.com/nan")).await;
    let published = manifest_server(manifest("0.3.0", "https://example.com/nan")).await;
    let manager = UpdateManager::new(
        "0.4.0",
        Some(format!("{recommended}/manifest.json")),
        Some(format!("{published}/manifest.json")),
        UpdateStateStore::new(directory.path()),
    )
    .expect("manager should build");

    assert!(
        manager
            .recommended_release(true, true)
            .await
            .expect("recommended release should load")
            .is_none()
    );
    assert!(
        manager
            .available_release()
            .await
            .expect("published release should load")
            .is_none()
    );
}

#[tokio::test]
async fn recommending_a_published_release_makes_startup_discovery_offer_it() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let recommendation = Arc::new(std::sync::Mutex::new(manifest(
        "0.2.0",
        "https://example.com/nan",
    )));
    let server = {
        let recommendation = Arc::clone(&recommendation);
        serve(Router::new().route(
            "/manifest.json",
            get(move || {
                let recommendation = Arc::clone(&recommendation);
                async move {
                    let release = recommendation
                        .lock()
                        .expect("recommendation should not be poisoned")
                        .clone();
                    axum::Json(release)
                }
            }),
        ))
        .await
    };
    let manager = UpdateManager::new(
        "0.1.0",
        Some(format!("{server}/manifest.json")),
        Some("https://example.com/available.json".to_owned()),
        UpdateStateStore::new(directory.path()),
    )
    .expect("manager should build");

    let before = manager
        .recommended_release(true, true)
        .await
        .expect("recommended release should load")
        .expect("recommended release should be newer");
    *recommendation
        .lock()
        .expect("recommendation should not be poisoned") =
        manifest("0.3.0", "https://example.com/nan");
    let cached = manager
        .recommended_release(false, true)
        .await
        .expect("cached release should load")
        .expect("cached release should be newer");
    let after = manager
        .recommended_release(true, true)
        .await
        .expect("recommended release should load")
        .expect("recommended release should be newer");

    assert_eq!(before.version, Version::new(0, 2, 0));
    assert_eq!(cached.version, Version::new(0, 2, 0));
    assert_eq!(after.version, Version::new(0, 3, 0));
}

#[tokio::test]
async fn an_unpublished_feed_falls_back_to_the_recommended_release() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let recommended = manifest_server(manifest("0.2.0", "https://example.com/nan")).await;
    let empty =
        serve(Router::new().route("/available.json", get(|| async { StatusCode::NOT_FOUND })))
            .await;
    let manager = UpdateManager::new(
        "0.1.0",
        Some(format!("{recommended}/manifest.json")),
        Some(format!("{empty}/available.json")),
        UpdateStateStore::new(directory.path()),
    )
    .expect("manager should build");

    let manual = manager
        .available_release()
        .await
        .expect("fallback release should load")
        .expect("fallback release should be newer");
    assert_eq!(manual.version, Version::new(0, 2, 0));
}

#[tokio::test]
async fn explicit_fallback_preserves_startup_state_even_when_it_is_malformed() {
    let recommended = manifest_server(manifest("0.2.0", "https://example.com/nan")).await;
    let missing =
        serve(Router::new().route("/available.json", get(|| async { StatusCode::NOT_FOUND })))
            .await;
    let gone =
        serve(Router::new().route("/available.json", get(|| async { StatusCode::GONE }))).await;
    for available_url in [
        None,
        Some(format!("{missing}/available.json")),
        Some(format!("{gone}/available.json")),
    ] {
        for initial_state in [
            None,
            Some("not valid JSON"),
            Some(
                r#"{"schemaVersion":1,"lastCheckedUnixSeconds":123,"skippedVersion":"0.2.0","cachedRelease":null}"#,
            ),
        ] {
            let directory = tempfile::tempdir().expect("temporary directory should exist");
            let state_path = directory.path().join("update.json");
            if let Some(contents) = initial_state {
                std::fs::write(&state_path, contents).expect("startup state should be seeded");
            }
            let manager = UpdateManager::new(
                "0.1.0",
                Some(format!("{recommended}/manifest.json")),
                available_url.clone(),
                UpdateStateStore::new(directory.path()),
            )
            .expect("manager should build");

            let release = manager
                .available_release()
                .await
                .expect("manual fallback must not depend on startup state")
                .expect("the skipped recommended release should still be offered manually");

            assert_eq!(release.version, Version::new(0, 2, 0));
            if let Some(contents) = initial_state {
                assert_eq!(
                    std::fs::read_to_string(&state_path).expect("startup state should remain"),
                    contents
                );
            } else {
                assert!(
                    !state_path.exists(),
                    "manual fallback must not create startup state"
                );
            }
        }
    }
}

#[tokio::test]
async fn explicit_fallback_does_not_offer_the_current_or_an_older_release() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let recommended = manifest_server(manifest("0.2.0", "https://example.com/nan")).await;
    for current_version in ["0.2.0", "0.3.0"] {
        let manager = UpdateManager::new(
            current_version,
            Some(format!("{recommended}/manifest.json")),
            None,
            UpdateStateStore::new(directory.path()),
        )
        .expect("manager should build");

        assert!(
            manager
                .available_release()
                .await
                .expect("manual fallback should load")
                .is_none()
        );
        assert!(!directory.path().join("update.json").exists());
    }
}

#[tokio::test]
async fn a_failing_feed_is_reported_instead_of_silently_falling_back() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let recommended = manifest_server(manifest("0.2.0", "https://example.com/nan")).await;
    let broken = serve(Router::new().route(
        "/available.json",
        get(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
    ))
    .await;
    let manager = UpdateManager::new(
        "0.1.0",
        Some(format!("{recommended}/manifest.json")),
        Some(format!("{broken}/available.json")),
        UpdateStateStore::new(directory.path()),
    )
    .expect("manager should build");

    assert!(matches!(
        manager.available_release().await,
        Err(super::UpdateError::ManifestStatus(500))
    ));
}

#[tokio::test]
async fn a_client_without_an_available_source_keeps_using_the_recommended_release() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let recommended = manifest_server(manifest("0.2.0", "https://example.com/nan")).await;
    let manager = UpdateManager::new(
        "0.1.0",
        Some(format!("{recommended}/manifest.json")),
        None,
        UpdateStateStore::new(directory.path()),
    )
    .expect("manager should build");

    let manual = manager
        .available_release()
        .await
        .expect("recommended release should load")
        .expect("recommended release should be newer");
    assert_eq!(manual.version, Version::new(0, 2, 0));
}

pub(super) fn manifest(version: &str, artifact_url: &str) -> ReleaseManifest {
    ReleaseManifest {
        schema_version: 1,
        version: Version::parse(version).expect("version should parse"),
        notes_url: "https://example.com/notes".to_owned(),
        artifacts: vec![ReleaseArtifact {
            target: current_target().to_owned(),
            url: artifact_url.to_owned(),
            sha256: "0".repeat(64),
        }],
    }
}

async fn manifest_server(manifest: ReleaseManifest) -> String {
    serve(Router::new().route(
        "/manifest.json",
        get(move || {
            let manifest = manifest.clone();
            async move { axum::Json(manifest) }
        }),
    ))
    .await
}

async fn serve(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let address = listener.local_addr().expect("address should resolve");
    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("server should run");
    });
    format!("http://{address}")
}
