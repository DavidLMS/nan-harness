use crate::release::compatibility::{
    merge_compatibility_feed, merge_hosted_compatibility_feed, merge_unified_compatibility_feed,
    merge_versioned_compatibility_feed,
};
use serde_json::{Value, json};
use std::{fs, path::Path};

type MergeFeed = fn(&Path, &Path, &Path) -> Result<(), String>;

fn feed_mergers() -> [(u8, MergeFeed); 4] {
    [
        (2, merge_compatibility_feed),
        (3, merge_unified_compatibility_feed),
        (4, merge_versioned_compatibility_feed),
        (5, merge_hosted_compatibility_feed),
    ]
}

fn evidence(version: &str) -> Value {
    json!({
        "id": "deepseek-harness",
        "lastCompatibleVersion": version,
        "compatibleAt": "2026-10-06T00:00:00Z",
        "lastLiveVerifiedVersion": version,
        "liveVerifiedAt": "2026-10-06T00:00:00Z"
    })
}

#[test]
fn all_feed_schemas_preserve_historical_evidence_below_current_minimum() {
    for (schema, merge) in feed_mergers() {
        let directory = tempfile::tempdir().unwrap();
        let base = directory.path().join("base.json");
        let updates = directory.path().join("updates");
        let output = directory.path().join("merged.json");
        fs::create_dir(&updates).unwrap();
        let historical = json!({
            "nanHarnessVersion": "0.1.14",
            "verifications": [evidence("0.1.0-rc.7")]
        });
        fs::write(
            &base,
            json!({
                "schemaVersion": schema, "releases": [historical.clone()]
            })
            .to_string(),
        )
        .unwrap();
        let mut update = evidence("0.2.0-rc.2");
        update["nanHarnessVersion"] = json!(env!("CARGO_PKG_VERSION"));
        fs::write(updates.join("deepseek.json"), update.to_string()).unwrap();

        merge(&base, &updates, &output).unwrap();
        let merged: Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
        let releases = merged["releases"].as_array().unwrap();
        assert_eq!(releases.len(), 2);
        let preserved = releases
            .iter()
            .find(|item| item["nanHarnessVersion"] == "0.1.14")
            .unwrap();
        assert_eq!(preserved["verifications"], historical["verifications"]);
        let current = releases
            .iter()
            .find(|item| item["nanHarnessVersion"] == env!("CARGO_PKG_VERSION"))
            .unwrap();
        assert_eq!(current["verifications"][0], evidence("0.2.0-rc.2"));
    }
}

#[test]
fn all_feed_schemas_reject_current_release_updates_below_minimum() {
    for (schema, merge) in feed_mergers() {
        let directory = tempfile::tempdir().unwrap();
        let base = directory.path().join("base.json");
        let updates = directory.path().join("updates");
        let output = directory.path().join("merged.json");
        fs::create_dir(&updates).unwrap();
        fs::write(
            &base,
            json!({"schemaVersion": schema, "releases": []}).to_string(),
        )
        .unwrap();
        let mut update = evidence("0.1.0-rc.7");
        update["nanHarnessVersion"] = json!(env!("CARGO_PKG_VERSION"));
        fs::write(updates.join("deepseek.json"), update.to_string()).unwrap();
        assert!(
            merge(&base, &updates, &output)
                .unwrap_err()
                .contains("below minimum")
        );
        assert!(!output.exists());
    }
}
