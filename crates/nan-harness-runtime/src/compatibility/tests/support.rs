use crate::compatibility::{
    DesktopVerificationEntry, UNIFIED_FEED_SCHEMA_VERSION, VerificationEntry, VerificationManifest,
    VerificationRelease,
};
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
            desktop_checks: Vec::new(),
            nan_harness_version: Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
            verifications: entries,
            desktop_verifications: Vec::new(),
        }],
    }
}

pub(super) async fn spawn_manifest_server(app: Router) -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("listener should bind");
    let address = listener.local_addr().expect("listener address");
    tokio::spawn(axum::serve(listener, app).into_future());
    address
}

pub(super) fn desktop_release(entries: Vec<DesktopVerificationEntry>) -> VerificationRelease {
    VerificationRelease {
        desktop_checks: Vec::new(),
        nan_harness_version: Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
        verifications: Vec::new(),
        desktop_verifications: entries,
    }
}

pub(super) fn unified_feed(
    verifications: Vec<VerificationEntry>,
    desktop_verifications: Vec<DesktopVerificationEntry>,
) -> VerificationManifest {
    VerificationManifest {
        schema_version: UNIFIED_FEED_SCHEMA_VERSION,
        releases: vec![VerificationRelease {
            desktop_checks: Vec::new(),
            nan_harness_version: Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
            verifications,
            desktop_verifications,
        }],
    }
}

pub(super) fn desktop_feed(entry: DesktopVerificationEntry) -> VerificationManifest {
    unified_feed(Vec::new(), vec![entry])
}
