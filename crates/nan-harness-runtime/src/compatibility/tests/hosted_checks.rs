use super::support::{base_manifest, feed_for_entries as release_manifest};
use crate::compatibility::hosted_checks::{apply_cli, validate_checks};
use crate::compatibility::{HOSTED_FEED_SCHEMA_VERSION, VerificationManifest};
use nan_harness_core::{HostedCheck, HostedOutcome};
use semver::Version;
use serde_json::json;

fn observation() -> HostedCheck {
    serde_json::from_value(json!({
        "suite": "cli", "id": "codex",
        "platform": if cfg!(target_os = "macos") { "macos" } else { std::env::consts::OS },
        "architecture": std::env::consts::ARCH,
        "harnessVersion": "99.0.0", "checkedAt": "2026-09-12T12:00:00Z", "outcome": "passed",
        "nanHarnessSha256": "a".repeat(64), "specSha256": "b".repeat(64),
        "evidenceSha256": "c".repeat(64), "sourceRun": 1234
    }))
    .unwrap()
}

#[test]
fn hosted_evidence_is_native_and_does_not_extend_live_claims() {
    let mut manifest = release_manifest(Vec::new());
    manifest.schema_version = HOSTED_FEED_SCHEMA_VERSION;
    manifest.releases[0].hosted_checks.push(observation());
    let original = base_manifest();
    let mut result = original.clone();
    apply_cli(&mut result, &manifest.releases[0]).unwrap();
    let id = nan_harness_core::HarnessKind::Codex;
    assert_eq!(
        result.entry(id).unwrap().last_compatible_version,
        Version::new(99, 0, 0)
    );
    assert_eq!(
        result.entry(id).unwrap().last_live_verified_version,
        original.entry(id).unwrap().last_live_verified_version
    );

    for mutation in 0..4 {
        let mut release = manifest.releases[0].clone();
        match mutation {
            0 => release.hosted_checks[0].model = Some("another-model".into()),
            1 => release.hosted_checks[0].outcome = HostedOutcome::Failed,
            2 => release.hosted_checks[0].platform = "other".into(),
            _ => release.hosted_checks[0].architecture = "other".into(),
        }
        let mut result = original.clone();
        apply_cli(&mut result, &release).unwrap();
        assert_eq!(result, original);
    }
}

#[test]
fn historical_feeds_stay_readable_but_cannot_carry_hosted_fields() {
    let base = base_manifest();
    for schema in 2..=5 {
        let mut manifest = release_manifest(Vec::new());
        manifest.schema_version = schema;
        let raw = serde_json::to_value(&manifest).unwrap();
        assert!(raw["releases"][0].get("hostedChecks").is_none());
        let decoded: VerificationManifest = serde_json::from_value(raw).unwrap();
        super::super::validation::validate_manifest(&decoded, &base).unwrap();
        manifest.releases[0].hosted_checks.push(observation());
        assert_eq!(
            super::super::validation::validate_manifest(&manifest, &base).is_ok(),
            schema == 5
        );
    }
}

#[test]
fn hosted_checks_reject_unbounded_or_duplicate_evidence() {
    let mut manifest = release_manifest(Vec::new());
    manifest.releases[0].hosted_checks.push(observation());
    validate_checks(&manifest.releases[0], &[], false).unwrap();
    for mutation in 0..5 {
        let mut release = manifest.releases[0].clone();
        match mutation {
            0 => release.hosted_checks[0].model = Some("private\npayload".into()),
            1 => release.hosted_checks[0].checked_at = "not-a-date".into(),
            2 => release.hosted_checks[0].source_run = 0,
            3 => release.hosted_checks[0].evidence_sha256 = "bad".into(),
            _ => release.hosted_checks.push(observation()),
        }
        assert!(validate_checks(&release, &[], false).is_err());
    }
}
