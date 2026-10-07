use super::support::{base_manifest, spawn_manifest_server};
use crate::compatibility::{
    CompatibilityError, RefreshOutcome, VerificationManifest,
    evidence::{apply_verifications, select_release},
    refresh::refresh_store,
    state::CompatibilityStateStore,
    validation::validate_manifest,
};
use axum::{Json, Router, routing::get};
use nan_harness_core::HarnessKind;
use semver::Version;
use serde_json::{Value, json};

fn mixed_feed(schema: u8) -> Value {
    let record = |release: &str, harness: &str| {
        json!({"nanHarnessVersion":release,"verifications":[{
            "id":"deepseek-harness","lastCompatibleVersion":harness,
            "compatibleAt":"2026-10-07T00:00:00Z",
            "lastLiveVerifiedVersion":harness,"liveVerifiedAt":"2026-10-07T00:00:00Z"
        }]})
    };
    json!({"schemaVersion":schema,"releases":[
        record("0.0.7", "0.1.0-rc.7"),
        record(env!("CARGO_PKG_VERSION"), "0.2.0-rc.2"),
        record("99.0.0", "99.0.0")
    ]})
}

#[tokio::test]
async fn mixed_history_downloads_caches_and_selects_only_the_running_release() {
    for schema in 2..=5 {
        let payload = mixed_feed(schema);
        let app = Router::new().route(
            "/feed",
            get(move || {
                let payload = payload.clone();
                async move { Json(payload) }
            }),
        );
        let address = spawn_manifest_server(app).await;
        let directory = tempfile::tempdir().unwrap();
        let store = CompatibilityStateStore::new(directory.path());
        let url = format!("http://{address}/feed");
        let mut base = base_manifest();
        assert_eq!(
            refresh_store(&url, &store, &base).await.unwrap(),
            RefreshOutcome::Updated
        );
        assert_eq!(
            refresh_store(&url, &store, &base).await.unwrap(),
            RefreshOutcome::Cached
        );
        let cached = store.load().unwrap().cached_manifest.unwrap();
        assert_eq!(cached.releases.len(), 3);
        let current = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
        let selected = select_release(&cached, &current).unwrap();
        apply_verifications(&mut base, selected).unwrap();
        assert_eq!(
            base.entry(HarnessKind::DeepSeekHarness)
                .unwrap()
                .last_live_verified_version,
            Some(Version::parse("0.2.0-rc.2").unwrap())
        );
    }
}

#[test]
fn mixed_history_still_enforces_current_minimums_and_all_evidence_integrity() {
    for schema in 2..=5 {
        for field in ["lastCompatibleVersion", "lastLiveVerifiedVersion"] {
            let mut value = mixed_feed(schema);
            value["releases"][1]["verifications"][0][field] = json!("0.1.0-rc.7");
            let feed: VerificationManifest = serde_json::from_value(value).unwrap();
            assert!(matches!(
                validate_manifest(&feed, &base_manifest()),
                Err(CompatibilityError::VersionBelowMinimum { .. }
                    | CompatibilityError::LiveVersionBelowMinimum { .. })
            ));
        }
        for index in [0, 2] {
            let mut value = mixed_feed(schema);
            value["releases"][index]["verifications"][0]["compatibleAt"] = json!("invalid");
            let feed = serde_json::from_value(value).unwrap();
            assert!(matches!(
                validate_manifest(&feed, &base_manifest()),
                Err(CompatibilityError::InvalidEvidenceTimestamp { .. })
            ));

            let mut value = mixed_feed(schema);
            value["releases"][index]["verifications"][0]["lastLiveVerifiedVersion"] =
                json!("100.0.0");
            let feed = serde_json::from_value(value).unwrap();
            assert!(matches!(
                validate_manifest(&feed, &base_manifest()),
                Err(CompatibilityError::LiveEvidenceAhead { .. })
            ));
        }
    }
}

#[test]
fn another_releases_live_only_evidence_does_not_use_current_embedded_fallback() {
    let mut value = mixed_feed(5);
    let future = value["releases"][2]["verifications"][0]
        .as_object_mut()
        .unwrap();
    future.remove("lastCompatibleVersion");
    future.remove("compatibleAt");
    let feed = serde_json::from_value(value).unwrap();
    validate_manifest(&feed, &base_manifest()).unwrap();
}
