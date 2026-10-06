use super::support::base_manifest;
use crate::compatibility::{VerificationManifest, validation::validate_manifest};
use crate::desktop_compatibility::{
    DesktopCompatibilityStatus, classify_desktop_version, embedded_desktop_compatibility,
};
use nan_harness_core::DesktopHarnessKind;
use serde_json::json;

fn feed() -> VerificationManifest {
    serde_json::from_value(json!({"schemaVersion":4,"releases":[{
        "nanHarnessVersion":env!("CARGO_PKG_VERSION"), "verifications":[],
        "desktopChecks":[{"id":"zed-desktop","platform":"macos","architecture":std::env::consts::ARCH,
            "appVersion":"1.19.0","deterministicAt":"2026-09-08T12:00:00Z"}]
    }]})).unwrap()
}

#[test]
fn shared_v4_fixture_is_readable() {
    let manifest: VerificationManifest = serde_json::from_str(include_str!(
        "../../../../../canary/fixtures/compatibility-v4.json"
    ))
    .unwrap();
    validate_manifest(&manifest, &base_manifest()).unwrap();
}

#[test]
fn independent_exact_checks_do_not_fabricate_live_or_change_legacy_bounds() {
    let manifest = feed();
    validate_manifest(&manifest, &base_manifest()).unwrap();
    let mut entry = embedded_desktop_compatibility(DesktopHarnessKind::Zed, "macos").unwrap();
    let old = entry.clone();
    super::super::desktop_checks::apply_checks(&mut entry, &manifest.releases[0]);
    assert_eq!(entry.evidence, old.evidence);
    assert_eq!(
        entry.last_compatible_app_version,
        old.last_compatible_app_version
    );
    assert_eq!(entry.checks[0].live_verified_at, None);
    assert_eq!(
        classify_desktop_version(&entry, Some(&"1.19.0".parse().unwrap())),
        DesktopCompatibilityStatus::Tested
    );
}

#[test]
fn another_architecture_is_not_adopted() {
    let mut manifest = feed();
    manifest.releases[0].desktop_checks[0].architecture = if std::env::consts::ARCH == "aarch64" {
        "x86_64"
    } else {
        "aarch64"
    }
    .to_owned();
    let mut entry = embedded_desktop_compatibility(DesktopHarnessKind::Zed, "macos").unwrap();
    super::super::desktop_checks::apply_checks(&mut entry, &manifest.releases[0]);
    assert!(entry.checks.is_empty());
}

#[test]
fn rejects_missing_success_duplicate_target_and_old_schema() {
    let mut manifest = feed();
    manifest.releases[0].desktop_checks[0].deterministic_at = None;
    assert!(validate_manifest(&manifest, &base_manifest()).is_err());
    let mut manifest = feed();
    let duplicate = manifest.releases[0].desktop_checks[0].clone();
    manifest.releases[0].desktop_checks.push(duplicate);
    assert!(validate_manifest(&manifest, &base_manifest()).is_err());
    let mut manifest = feed();
    manifest.schema_version = 3;
    assert!(validate_manifest(&manifest, &base_manifest()).is_err());
}

#[test]
fn historical_checks_are_shape_checked_without_current_minimums() {
    let mut manifest = feed();
    manifest.releases[0].nan_harness_version = "0.0.1".parse().unwrap();
    manifest.releases[0].desktop_checks[0].app_version = "0.0.1".parse().unwrap();
    validate_manifest(&manifest, &base_manifest()).unwrap();
    manifest.releases[0].desktop_checks[0].architecture = "unknown".to_owned();
    assert!(validate_manifest(&manifest, &base_manifest()).is_err());
}
