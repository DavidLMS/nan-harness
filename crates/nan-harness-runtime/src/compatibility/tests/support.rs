// Shared private fixtures and helpers for the compatibility suite: the embedded
// base manifest, the release-feed builders the validation and merge coverage
// uses, and the loopback fake manifest server the remote coverage drives.
use crate::compatibility::{VerificationEntry, VerificationManifest, VerificationRelease};
use axum::Router;
use nan_harness_core::CompatibilityManifest;
use semver::Version;
use std::net::SocketAddr;
use tokio::net::TcpListener;

pub(super) fn base_manifest() -> CompatibilityManifest {
    crate::discovery::bundled_compatibility_manifest().expect("embedded manifest")
}

pub(super) fn feed_for(entry: VerificationEntry) -> VerificationManifest {
    feed_for_entries(vec![entry])
}

pub(super) fn feed_for_entries(entries: Vec<VerificationEntry>) -> VerificationManifest {
    VerificationManifest {
        schema_version: 2,
        releases: vec![VerificationRelease {
            nan_harness_version: Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
            verifications: entries,
        }],
    }
}

/// Serves a fake compatibility feed on an ephemeral loopback port.
///
/// The spawned server belongs to the current test runtime, which stops it once
/// the test finishes, so every test keeps an isolated endpoint and store.
pub(super) async fn spawn_manifest_server(app: Router) -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("listener should bind");
    let address = listener.local_addr().expect("listener address");
    tokio::spawn(axum::serve(listener, app).into_future());
    address
}
