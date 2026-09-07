use super::super::compatibility::bundled_compatibility_manifest;
use super::super::verification::{
    HarnessRequirement, VerificationEntry, VerificationRelease, current_release_version,
    merge_evidence_pair, merge_verification_entry, validate_releases,
};
use crate::release::{generate_compatibility_feed, merge_compatibility_feed};
use nan_harness_core::HarnessKind;
use semver::Version;
use serde_json::Value;
use std::fs;

#[test]
fn compatibility_merges_only_known_non_regressing_updates() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let base = directory.path().join("base.json");
    let updates = directory.path().join("updates");
    let output = directory.path().join("merged.json");
    fs::create_dir(&updates).expect("updates directory should exist");
    generate_compatibility_feed(&base).expect("base feed should be generated");
    fs::write(
            updates.join("fx.json"),
            r#"{"id":"fx","lastCompatibleVersion":"0.0.4","compatibleAt":"2026-08-20T08:00:00Z","lastLiveVerifiedVersion":"0.0.4","liveVerifiedAt":"2026-08-20T08:00:00Z"}"#,
        )
        .expect("fx update should exist");

    merge_compatibility_feed(&base, &updates, &output).expect("compatibility feed should merge");
    let merged: Value =
        serde_json::from_slice(&fs::read(output).expect("merged feed should be readable"))
            .expect("merged feed should be JSON");
    assert_eq!(merged["schemaVersion"], 2);
    let fx = merged["releases"][0]["verifications"]
        .as_array()
        .expect("verifications should be an array")
        .iter()
        .find(|entry| entry["id"] == "fx")
        .expect("fx should remain in the feed");

    assert_eq!(fx["lastCompatibleVersion"], "0.0.7");
    assert_eq!(fx["lastLiveVerifiedVersion"], "0.0.7");
    assert_eq!(fx["compatibleAt"], "2026-09-07T03:05:19.08398Z");
}

#[test]
fn compatibility_preserves_releases_and_merges_partial_updates_monotonically() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let base = directory.path().join("base.json");
    let updates = directory.path().join("updates");
    let output = directory.path().join("merged.json");
    fs::create_dir(&updates).expect("updates directory should exist");
    fs::write(
            &base,
            format!(
                r#"{{"schemaVersion":2,"releases":[{{"nanHarnessVersion":"0.0.5","verifications":[{{"id":"fx","lastCompatibleVersion":"0.0.5","compatibleAt":"2026-08-01T00:00:00Z"}}]}},{{"nanHarnessVersion":"{}","verifications":[]}}]}}"#,
                env!("CARGO_PKG_VERSION")
            ),
        )
        .expect("base feed should exist");
    fs::write(
        updates.join("fx.json"),
        r#"{"id":"fx","lastCompatibleVersion":"0.0.4","compatibleAt":"2026-08-01T00:00:00Z"}"#,
    )
    .expect("partial update should exist");

    merge_compatibility_feed(&base, &updates, &output)
        .expect("partial compatibility update should merge");
    let merged: Value =
        serde_json::from_slice(&fs::read(output).expect("merged feed should be readable"))
            .expect("merged feed should be JSON");
    let releases = merged["releases"]
        .as_array()
        .expect("releases should be an array");
    assert_eq!(releases.len(), 2);
    assert_eq!(releases[0]["nanHarnessVersion"], "0.0.5");
    let current = releases
        .iter()
        .find(|release| release["nanHarnessVersion"] == env!("CARGO_PKG_VERSION"))
        .expect("current release should remain in the feed");
    assert!(current["verifications"].as_array().is_some());
}

#[test]
fn compatibility_does_not_seed_a_new_release_before_an_update() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let base = directory.path().join("base.json");
    let updates = directory.path().join("updates");
    let output = directory.path().join("merged.json");
    fs::create_dir(&updates).expect("updates directory should exist");
    fs::write(
        &base,
        r#"{"schemaVersion":2,"releases":[{"nanHarnessVersion":"0.0.5","verifications":[]}]}"#,
    )
    .expect("base feed should exist");
    fs::write(
            updates.join("fx.json"),
            format!(
                r#"{{"nanHarnessVersion":"{}","id":"fx","lastCompatibleVersion":"0.0.4","compatibleAt":"2026-08-20T08:00:00Z"}}"#,
                env!("CARGO_PKG_VERSION")
            ),
        )
        .expect("fx update should exist");

    merge_compatibility_feed(&base, &updates, &output).expect("compatibility feed should merge");
    let merged: Value =
        serde_json::from_slice(&fs::read(output).expect("merged feed should be readable"))
            .expect("merged feed should be JSON");
    let current = merged["releases"]
        .as_array()
        .expect("releases should be an array")
        .iter()
        .find(|release| release["nanHarnessVersion"] == env!("CARGO_PKG_VERSION"))
        .expect("updated release should exist");
    assert_eq!(current["verifications"].as_array().unwrap().len(), 1);
    assert_eq!(current["verifications"][0]["id"], "fx");
}

#[test]
fn compatibility_merge_rejects_malformed_pairs_and_missing_evidence() {
    let requirements = requirements();
    let cases = [
        VerificationEntry {
            id: "codex".to_owned(),
            last_compatible_version: Some(Version::new(0, 147, 0)),
            compatible_at: None,
            last_live_verified_version: None,
            live_verified_at: None,
        },
        VerificationEntry {
            id: "codex".to_owned(),
            last_compatible_version: None,
            compatible_at: None,
            last_live_verified_version: None,
            live_verified_at: None,
        },
        VerificationEntry {
            id: "codex".to_owned(),
            last_compatible_version: Some(Version::new(0, 147, 0)),
            compatible_at: Some("2026-08-19".to_owned()),
            last_live_verified_version: None,
            live_verified_at: None,
        },
    ];
    for entry in cases {
        let result = validate_releases(
            &[VerificationRelease {
                nan_harness_version: current_release_version(),
                verifications: vec![entry],
                desktop_verifications: Vec::new(),
            }],
            &requirements,
            None,
            "test feed",
        );
        assert!(result.is_err());
    }
}

#[test]
fn compatibility_merge_rejects_minimum_duplicate_and_live_order_violations() {
    let requirements = requirements();
    let below_minimum = entry("codex", "0.145.0", "2026-08-19T00:00:00Z");
    assert!(validate_single(&requirements, below_minimum).is_err());

    let duplicate = entry("codex", "0.147.0", "2026-08-19T00:00:00Z");
    assert!(
        validate_releases(
            &[VerificationRelease {
                nan_harness_version: current_release_version(),
                verifications: vec![duplicate.clone(), duplicate],
                desktop_verifications: Vec::new(),
            }],
            &requirements,
            None,
            "test feed",
        )
        .is_err()
    );
    assert!(
        validate_releases(
            &[
                VerificationRelease {
                    nan_harness_version: current_release_version(),
                    verifications: Vec::new(),
                    desktop_verifications: Vec::new(),
                },
                VerificationRelease {
                    nan_harness_version: current_release_version(),
                    verifications: Vec::new(),
                    desktop_verifications: Vec::new(),
                },
            ],
            &requirements,
            None,
            "test feed",
        )
        .is_err()
    );

    let live_ahead = VerificationEntry {
        id: "codex".to_owned(),
        last_compatible_version: Some(Version::new(0, 146, 0)),
        compatible_at: Some("2026-08-19T00:00:00Z".to_owned()),
        last_live_verified_version: Some(Version::new(0, 147, 0)),
        live_verified_at: Some("2026-08-20T00:00:00Z".to_owned()),
    };
    assert!(validate_single(&requirements, live_ahead).is_err());
}

#[test]
fn compatibility_merge_preserves_unknown_ids_and_merges_pairs_atomically() {
    let requirements = requirements();
    let unknown = entry("future-harness", "99.0.0", "2026-08-19T00:00:00Z");
    assert!(validate_single(&requirements, unknown.clone()).is_ok());

    let mut current = entry("fx", "0.0.3", "2026-08-20T00:00:00Z");
    merge_verification_entry(
        &mut current,
        &entry("fx", "0.0.4", "2026-08-19T00:00:00Z"),
        "test feed",
    )
    .expect("higher version should replace the complete pair");
    assert_eq!(current.last_compatible_version, Some(Version::new(0, 0, 4)));
    assert_eq!(
        current.compatible_at.as_deref(),
        Some("2026-08-20T00:00:00Z")
    );

    merge_verification_entry(
        &mut current,
        &entry("fx", "0.0.5", "2026-08-18T00:00:00Z"),
        "test feed",
    )
    .expect("higher version should retain the newer existing timestamp");
    assert_eq!(current.last_compatible_version, Some(Version::new(0, 0, 5)));
    assert_eq!(
        current.compatible_at.as_deref(),
        Some("2026-08-20T00:00:00Z")
    );

    merge_verification_entry(
        &mut current,
        &entry("fx", "0.0.4", "2026-08-20T00:00:00Z"),
        "test feed",
    )
    .expect("equal version with later timestamp should advance the timestamp");
    assert_eq!(
        current.compatible_at.as_deref(),
        Some("2026-08-20T00:00:00Z")
    );
    let unchanged = current.clone();
    merge_verification_entry(
        &mut current,
        &entry("fx", "0.0.3", "2026-08-21T00:00:00Z"),
        "test feed",
    )
    .expect("lower version should be ignored");
    assert_eq!(current, unchanged);

    merge_verification_entry(
        &mut current,
        &entry("fx", "0.0.5", "2026-08-19T00:00:00Z"),
        "test feed",
    )
    .expect("equal version with an older timestamp should be ignored");
    assert_eq!(current, unchanged);

    let mut absent_version = None;
    let mut stray_timestamp = Some("2026-08-21T00:00:00Z".to_owned());
    merge_evidence_pair(
        &mut absent_version,
        &mut stray_timestamp,
        None,
        Some(&"2026-08-22T00:00:00Z".to_owned()),
        "fx",
        "compatible",
        "test feed",
    )
    .expect("an update without a version should be ignored");
    assert_eq!(absent_version, None);
    assert_eq!(stray_timestamp.as_deref(), Some("2026-08-21T00:00:00Z"));

    let mut incomplete_version = Some(Version::new(0, 0, 3));
    let mut incomplete_timestamp = None;
    assert!(
        merge_evidence_pair(
            &mut incomplete_version,
            &mut incomplete_timestamp,
            Some(&Version::new(0, 0, 4)),
            Some(&"2026-08-22T00:00:00Z".to_owned()),
            "fx",
            "compatible",
            "test feed",
        )
        .is_err()
    );
}

fn requirements() -> std::collections::BTreeMap<HarnessKind, HarnessRequirement> {
    let manifest = bundled_compatibility_manifest().expect("embedded manifest");
    manifest
        .harnesses
        .into_iter()
        .map(|entry| {
            (
                entry.id,
                HarnessRequirement {
                    minimum_version: entry.minimum_version,
                    compatible_version: entry.last_compatible_version,
                },
            )
        })
        .collect()
}

fn validate_single(
    requirements: &std::collections::BTreeMap<HarnessKind, HarnessRequirement>,
    entry: VerificationEntry,
) -> Result<(), String> {
    validate_releases(
        &[VerificationRelease {
            nan_harness_version: current_release_version(),
            verifications: vec![entry],
            desktop_verifications: Vec::new(),
        }],
        requirements,
        None,
        "test feed",
    )
}

fn entry(id: &str, version: &str, timestamp: &str) -> VerificationEntry {
    VerificationEntry {
        id: id.to_owned(),
        last_compatible_version: Some(Version::parse(version).expect("version")),
        compatible_at: Some(timestamp.to_owned()),
        last_live_verified_version: None,
        live_verified_at: None,
    }
}
