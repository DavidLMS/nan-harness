use super::super::compatibility::{
    generate_hosted_compatibility_feed, merge_hosted_compatibility_feed,
    validate_hosted_compatibility_feed, validate_versioned_compatibility_feed,
};
use serde_json::{Value, json};
use std::fs;

fn observation(day: &str, outcome: &str, digest: &str) -> Value {
    json!({"suite":"cli", "id":"codex", "platform":"linux", "architecture":"aarch64",
        "harnessVersion":"0.155.0", "model":"qwen3.6", "checkedAt":format!("2026-09-{day}T12:00:00Z"),
        "outcome":outcome, "nanHarnessSha256":"a".repeat(64), "specSha256":"b".repeat(64),
        "evidenceSha256":digest.repeat(64), "sourceRun":1234})
}

#[test]
fn hosted_merge_retains_last_success_and_separates_models_and_versions() {
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().join("base.json");
    let output = directory.path().join("output.json");
    let updates = directory.path().join("updates");
    fs::create_dir(&updates).unwrap();
    generate_hosted_compatibility_feed(&base).unwrap();
    let mut observations = vec![
        observation("10", "passed", "c"),
        observation("11", "failed", "d"),
    ];
    let mut alternate = observation("12", "passed", "e");
    alternate["model"] = json!("another-model");
    observations.push(alternate);
    let mut latest = observation("12", "blocked", "f");
    latest["harnessVersion"] = json!("0.156.0");
    observations.push(latest);
    let release = json!({"nanHarnessVersion":env!("CARGO_PKG_VERSION"), "verifications":[],
                         "hostedChecks":observations});
    fs::write(
        updates.join("checks.json"),
        serde_json::to_vec(&release).unwrap(),
    )
    .unwrap();
    merge_hosted_compatibility_feed(&base, &updates, &output).unwrap();
    validate_hosted_compatibility_feed(&output).unwrap();
    assert!(validate_versioned_compatibility_feed(&output).is_err());
    let first: Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
    let checks = first["releases"][0]["hostedChecks"].as_array().unwrap();
    assert_eq!(checks.len(), 4);
    assert!(
        checks
            .iter()
            .any(|check| check["outcome"] == "passed" && check["model"] == "qwen3.6")
    );
    assert!(
        checks
            .iter()
            .any(|check| check["outcome"] == "failed" && check["model"] == "qwen3.6")
    );
    merge_hosted_compatibility_feed(&output, &updates, &base).unwrap();
    let repeated: Value = serde_json::from_slice(&fs::read(&base).unwrap()).unwrap();
    assert_eq!(first, repeated);
}

#[test]
fn a_failed_validation_does_not_replace_the_destination() {
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().join("base.json");
    let output = directory.path().join("output.json");
    let updates = directory.path().join("updates");
    fs::create_dir(&updates).unwrap();
    generate_hosted_compatibility_feed(&base).unwrap();
    fs::write(&output, "unchanged").unwrap();
    let check = observation("12", "passed", "c");
    let release = json!({"nanHarnessVersion":env!("CARGO_PKG_VERSION"), "verifications":[],
                         "hostedChecks":[check.clone(), check]});
    fs::write(
        updates.join("checks.json"),
        serde_json::to_vec(&release).unwrap(),
    )
    .unwrap();
    assert!(merge_hosted_compatibility_feed(&base, &updates, &output).is_err());
    assert_eq!(fs::read_to_string(&output).unwrap(), "unchanged");
}

#[test]
fn authenticated_updates_reject_release_changes_and_mixed_legacy_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().join("base.json");
    let output = directory.path().join("output.json");
    let updates = directory.path().join("updates");
    fs::create_dir(&updates).unwrap();
    generate_hosted_compatibility_feed(&base).unwrap();
    let registry = super::super::validation::repository_root()
        .join("crates/nan-harness-runtime/resources/desktop-compatibility.json");
    let version = env!("CARGO_PKG_VERSION");
    let mut update = json!({"nanHarnessVersion":version, "verifications":[],
                           "hostedChecks":[observation("12", "passed", "c")]});
    let update_path = updates.join("check.json");
    fs::write(&update_path, serde_json::to_vec(&update).unwrap()).unwrap();
    super::super::merge_release_hosted_checks(&base, &updates, &registry, version, &output)
        .unwrap();
    let unchanged = fs::read(&output).unwrap();
    update["nanHarnessVersion"] = json!("999.0.0");
    fs::write(&update_path, serde_json::to_vec(&update).unwrap()).unwrap();
    assert!(
        super::super::merge_release_hosted_checks(&base, &updates, &registry, version, &output,)
            .is_err()
    );
    assert_eq!(fs::read(&output).unwrap(), unchanged);
    update["nanHarnessVersion"] = json!(version);
    update["desktopChecks"] = json!([{"id":"zed-desktop", "platform":"linux",
        "architecture":"x86_64", "appVersion":"1.0.0", "deterministicAt":"2026-09-12T00:00:00Z"}]);
    fs::write(&update_path, serde_json::to_vec(&update).unwrap()).unwrap();
    assert!(
        super::super::merge_release_hosted_checks(&base, &updates, &registry, version, &output,)
            .is_err()
    );
    assert_eq!(fs::read(&output).unwrap(), unchanged);
}
