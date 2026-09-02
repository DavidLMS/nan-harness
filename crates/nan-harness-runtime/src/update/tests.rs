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
        UpdateStateStore::new(directory.path()),
    )
    .expect("manager should build");

    let available = manager
        .available_release(true, true)
        .await
        .expect("release should load")
        .expect("release should be newer");
    assert_eq!(available.version, Version::new(0, 2, 0));

    manager
        .skip(available.version.clone())
        .expect("skip should persist");
    assert!(
        manager
            .available_release(false, true)
            .await
            .expect("cached release should load")
            .is_none()
    );
    assert!(
        manager
            .available_release(false, false)
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
        UpdateStateStore::new(directory.path()),
    )
    .expect("manager should build");

    assert!(
        manager
            .available_release(true, true)
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
        UpdateStateStore::new(directory.path()),
    )
    .expect("manager should build");

    assert!(matches!(
        manager.available_release(true, true).await,
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
        UpdateStateStore::new(directory.path()),
    )
    .expect("manager should build");

    let error = manager
        .available_release(true, true)
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
