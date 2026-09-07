use crate::release::{
    generate_compatibility_feed, generate_unified_compatibility_feed, merge_compatibility_feed,
    merge_unified_compatibility_feed, validate_compatibility_feed,
    validate_unified_compatibility_feed,
};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

struct Feeds {
    _directory: tempfile::TempDir,
    updates: PathBuf,
    base: PathBuf,
    output: PathBuf,
}

fn feeds() -> Feeds {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let updates = directory.path().join("updates");
    fs::create_dir(&updates).expect("updates directory should exist");
    Feeds {
        base: directory.path().join("base.json"),
        output: directory.path().join("merged.json"),
        updates,
        _directory: directory,
    }
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("feed should be readable"))
        .expect("feed should be JSON")
}

fn desktop_records(feed: &Value) -> &Vec<Value> {
    feed["releases"][0]["desktopVerifications"]
        .as_array()
        .expect("desktop evidence should be an array")
}

fn desktop_record<'a>(feed: &'a Value, id: &str, platform: &str) -> &'a Value {
    desktop_records(feed)
        .iter()
        .find(|entry| entry["id"] == id && entry["platform"] == platform)
        .unwrap_or_else(|| panic!("{id} on {platform} should be published"))
}

#[test]
fn both_assets_are_generated_from_the_same_evidence() {
    let feeds = feeds();
    let unified = feeds.base.with_file_name("compatibility-v3.json");
    generate_compatibility_feed(&feeds.base).expect("legacy feed should be generated");
    generate_unified_compatibility_feed(&unified).expect("unified feed should be generated");
    validate_compatibility_feed(&feeds.base).expect("legacy feed should validate");
    validate_unified_compatibility_feed(&unified).expect("unified feed should validate");

    let legacy = read(&feeds.base);
    let unified = read(&unified);

    assert_eq!(legacy["schemaVersion"], 2);
    assert_eq!(unified["schemaVersion"], 3);
    assert!(
        legacy["releases"][0].get("desktopVerifications").is_none(),
        "the legacy asset must stay CLI-only for clients that reject unknown fields"
    );
    assert_eq!(
        legacy["releases"][0]["verifications"], unified["releases"][0]["verifications"],
        "both assets must carry the same CLI evidence"
    );
    let chatgpt = desktop_record(&unified, "chatgpt-desktop", "macos");
    assert_eq!(chatgpt["evidence"], "live-verified");
    assert_eq!(chatgpt["lastCompatibleAppVersion"], "26.825.51511");
    assert_eq!(chatgpt["lastCompatibleRuntimeVersion"], "0.151.0-alpha.7.2");
}

#[test]
fn every_registered_desktop_surface_is_published() {
    let feeds = feeds();
    generate_unified_compatibility_feed(&feeds.base).expect("unified feed should be generated");
    let feed = read(&feeds.base);

    for id in [
        "chatgpt-desktop",
        "claude-desktop",
        "hermes-desktop",
        "pen-desktop",
        "zed-desktop",
    ] {
        for platform in ["macos", "windows", "linux"] {
            let record = desktop_record(&feed, id, platform);
            assert!(
                matches!(
                    record["evidence"].as_str(),
                    Some("live-verified" | "contract-only")
                ),
                "{id} on {platform} has unexpected evidence {}",
                record["evidence"]
            );
        }
    }
}

#[test]
fn desktop_updates_advance_the_unified_feed_only() {
    let feeds = feeds();
    generate_unified_compatibility_feed(&feeds.base).expect("unified feed should be generated");
    fs::write(
        feeds.updates.join("desktop-chatgpt.json"),
        r#"{"id":"chatgpt-desktop","platform":"macos","evidence":"live-verified","lastCompatibleAppVersion":"26.831.21537","lastCompatibleRuntimeVersion":"0.152.0","compatibleAt":"2026-09-07T00:00:00Z"}"#,
    )
    .expect("desktop update should be written");

    merge_unified_compatibility_feed(&feeds.base, &feeds.updates, &feeds.output)
        .expect("unified feed should merge");
    validate_unified_compatibility_feed(&feeds.output).expect("merged feed should validate");

    let merged = read(&feeds.output);
    let chatgpt = desktop_record(&merged, "chatgpt-desktop", "macos");
    assert_eq!(chatgpt["lastCompatibleAppVersion"], "26.831.21537");
    assert_eq!(chatgpt["lastCompatibleRuntimeVersion"], "0.152.0");
    assert_eq!(chatgpt["compatibleAt"], "2026-09-07T00:00:00Z");

    let legacy_base = feeds.base.with_file_name("legacy.json");
    let legacy_merged = feeds.base.with_file_name("legacy-merged.json");
    fs::write(
        feeds.updates.join("fx.json"),
        r#"{"id":"fx","lastCompatibleVersion":"0.0.8","compatibleAt":"2026-09-07T00:00:00Z"}"#,
    )
    .expect("CLI update should be written");
    generate_compatibility_feed(&legacy_base).expect("legacy feed should be generated");
    merge_compatibility_feed(&legacy_base, &feeds.updates, &legacy_merged)
        .expect("the legacy asset ignores Desktop updates instead of failing the run");
    validate_compatibility_feed(&legacy_merged).expect("legacy feed should validate");

    let legacy = read(&legacy_merged);
    assert!(
        legacy["releases"]
            .as_array()
            .expect("releases should be an array")
            .iter()
            .all(|release| release.get("desktopVerifications").is_none()),
        "Desktop evidence must never reach the legacy asset"
    );
}

#[test]
fn desktop_updates_never_regress_published_evidence() {
    let feeds = feeds();
    generate_unified_compatibility_feed(&feeds.base).expect("unified feed should be generated");
    fs::write(
        feeds.updates.join("desktop-chatgpt.json"),
        r#"{"id":"chatgpt-desktop","platform":"macos","evidence":"contract-only","lastCompatibleAppVersion":"26.825.51511","lastCompatibleRuntimeVersion":"0.151.0-alpha.7.2","compatibleAt":"2026-08-01T00:00:00Z"}"#,
    )
    .expect("desktop update should be written");

    merge_unified_compatibility_feed(&feeds.base, &feeds.updates, &feeds.output)
        .expect("unified feed should merge");

    let published = read(&feeds.base);
    let merged = read(&feeds.output);
    assert_eq!(
        desktop_record(&merged, "chatgpt-desktop", "macos"),
        desktop_record(&published, "chatgpt-desktop", "macos")
    );
}

#[test]
fn unpublishable_desktop_evidence_is_rejected() {
    let cases = [
        (
            "unknown surface",
            r#"{"id":"chatgpt-desktop","platform":"haiku","evidence":"contract-only","compatibleAt":"2026-09-07T00:00:00Z"}"#,
            "unknown Desktop surface",
        ),
        (
            "runtime-only live claim",
            r#"{"id":"chatgpt-desktop","platform":"macos","evidence":"live-verified","lastCompatibleRuntimeVersion":"0.152.0","compatibleAt":"2026-09-07T00:00:00Z"}"#,
            "without application evidence",
        ),
        (
            "application-only live claim",
            r#"{"id":"chatgpt-desktop","platform":"macos","evidence":"live-verified","lastCompatibleAppVersion":"26.831.21537","compatibleAt":"2026-09-07T00:00:00Z"}"#,
            "without runtime evidence",
        ),
        (
            "evidence below the embedded minimum",
            r#"{"id":"chatgpt-desktop","platform":"macos","evidence":"contract-only","lastCompatibleAppVersion":"1.0.0","compatibleAt":"2026-09-07T00:00:00Z"}"#,
            "below minimum",
        ),
        (
            "malformed timestamp",
            r#"{"id":"chatgpt-desktop","platform":"macos","evidence":"contract-only","lastCompatibleAppVersion":"26.831.21537","compatibleAt":"the seventh"}"#,
            "invalid compatibleAt timestamp",
        ),
        (
            "unavailable claim",
            r#"{"id":"chatgpt-desktop","platform":"macos","evidence":"unavailable","compatibleAt":"2026-09-07T00:00:00Z"}"#,
            "unpublishable evidence",
        ),
    ];

    for (label, payload, expected) in cases {
        let feeds = feeds();
        generate_unified_compatibility_feed(&feeds.base).expect("unified feed should be generated");
        fs::write(feeds.updates.join("desktop-update.json"), payload)
            .expect("desktop update should be written");

        let error = merge_unified_compatibility_feed(&feeds.base, &feeds.updates, &feeds.output)
            .expect_err(label);

        assert!(error.contains(expected), "{label}: {error}");
        assert!(
            !feeds.output.exists(),
            "{label}: a rejected update must not publish a candidate"
        );
    }
}

#[test]
fn feeds_are_validated_against_their_own_schema() {
    let feeds = feeds();
    let unified = feeds.base.with_file_name("compatibility-v3.json");
    generate_compatibility_feed(&feeds.base).expect("legacy feed should be generated");
    generate_unified_compatibility_feed(&unified).expect("unified feed should be generated");

    assert!(validate_unified_compatibility_feed(&feeds.base).is_err());
    assert!(validate_compatibility_feed(&unified).is_err());
}

/// The documented example both sides agree on. The client validates the same file in
/// `crates/nan-harness-runtime/src/compatibility/tests/desktop_overlay.rs`.
#[test]
fn the_documented_example_feed_is_published_shaped() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask manifest has a repository parent")
        .join("docs/examples/compatibility-v3.json");

    validate_unified_compatibility_feed(&path).expect("the example feed should validate");
    assert!(
        validate_compatibility_feed(&path).is_err(),
        "the example is a unified feed and must not pass as a legacy one"
    );
}
