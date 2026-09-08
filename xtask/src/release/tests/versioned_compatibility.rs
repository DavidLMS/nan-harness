use crate::release::{
    generate_versioned_compatibility_feed, merge_versioned_compatibility_feed,
    validate_unified_compatibility_feed, validate_versioned_compatibility_feed,
};
use serde_json::{Value, json};
use std::fs;

fn update(architecture: &str, version: &str, deterministic: bool) -> Value {
    let mut check = json!({"id":"zed-desktop","platform":"macos","architecture":architecture,"appVersion":version});
    check[if deterministic {
        "deterministicAt"
    } else {
        "liveVerifiedAt"
    }] = json!("2026-09-08T12:00:00Z");
    json!({"nanHarnessVersion":env!("CARGO_PKG_VERSION"),"verifications":[],"desktopChecks":[check]})
}

#[test]
fn shared_v4_fixture_is_publishable() {
    validate_versioned_compatibility_feed(
        &crate::release::validation::repository_root()
            .join("canary/fixtures/compatibility-v4.json"),
    )
    .unwrap();
}

#[test]
fn merge_retains_independent_tracks_and_exact_targets_without_changing_legacy_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().join("base.json");
    let output = directory.path().join("output.json");
    let updates = directory.path().join("updates");
    fs::create_dir(&updates).unwrap();
    generate_versioned_compatibility_feed(&base).unwrap();
    let before: Value = serde_json::from_slice(&fs::read(&base).unwrap()).unwrap();
    for (index, check) in [
        update("aarch64", "1.19.0", true),
        update("aarch64", "1.19.0", false),
        update("x86_64", "1.19.0", true),
        update("aarch64", "1.20.0", true),
    ]
    .iter()
    .enumerate()
    {
        fs::write(
            updates.join(format!("{index}.json")),
            serde_json::to_vec(check).unwrap(),
        )
        .unwrap();
    }
    merge_versioned_compatibility_feed(&base, &updates, &output).unwrap();
    validate_versioned_compatibility_feed(&output).unwrap();
    assert!(validate_unified_compatibility_feed(&output).is_err());
    let after: Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(
        before["releases"][0]["desktopVerifications"],
        after["releases"][0]["desktopVerifications"]
    );
    let checks = after["releases"][0]["desktopChecks"].as_array().unwrap();
    assert_eq!(checks.len(), 3);
    let live = checks
        .iter()
        .find(|check| check.get("liveVerifiedAt").is_some())
        .unwrap();
    assert_eq!(live["architecture"], "aarch64");
    assert_eq!(live["appVersion"], "1.19.0");
    assert!(live.get("deterministicAt").is_some());
    merge_versioned_compatibility_feed(&output, &updates, &base).unwrap();
    assert_eq!(fs::read(&base).unwrap(), fs::read(&output).unwrap());
}

#[test]
fn historical_updates_require_their_own_registry() {
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().join("base.json");
    let output = directory.path().join("output.json");
    let updates = directory.path().join("updates");
    fs::create_dir(&updates).unwrap();
    generate_versioned_compatibility_feed(&base).unwrap();
    let mut historical = update("aarch64", "1.0.0", true);
    historical["nanHarnessVersion"] = json!("0.0.1");
    fs::write(
        updates.join("check.json"),
        serde_json::to_vec(&historical).unwrap(),
    )
    .unwrap();
    assert!(merge_versioned_compatibility_feed(&base, &updates, &output).is_err());
    let registry = directory.path().join("registry.json");
    fs::write(&registry, serde_json::to_vec(&json!({"schemaVersion":1,"surfaces":[{
        "id":"zed-desktop","platform":"macos","transport":"chat-completions-gateway","evidence":"contract-only","minimumAppVersion":"0.9.0","compatibleAt":"2026-08-01"
    }]})).unwrap()).unwrap();
    crate::release::merge_release_checks(&base, &updates, &registry, "0.0.1", &output).unwrap();
    validate_versioned_compatibility_feed(&output).unwrap();
    assert!(
        crate::release::merge_release_checks(&base, &updates, &registry, "0.0.2", &output).is_err()
    );
}

#[test]
fn incomplete_runtime_pair_and_missing_track_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().join("base.json");
    generate_versioned_compatibility_feed(&base).unwrap();
    let mut manifest: Value = serde_json::from_slice(&fs::read(&base).unwrap()).unwrap();
    manifest["releases"][0]["desktopChecks"] = json!([{"id":"chatgpt-desktop","platform":"macos","architecture":"aarch64","appVersion":"26.900.0","deterministicAt":"2026-09-08T12:00:00Z"}]);
    fs::write(&base, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(validate_versioned_compatibility_feed(&base).is_err());
    manifest["releases"][0]["desktopChecks"][0]["runtimeVersion"] = json!("0.155.0");
    manifest["releases"][0]["desktopChecks"][0]
        .as_object_mut()
        .unwrap()
        .remove("deterministicAt");
    fs::write(&base, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(validate_versioned_compatibility_feed(&base).is_err());
}
