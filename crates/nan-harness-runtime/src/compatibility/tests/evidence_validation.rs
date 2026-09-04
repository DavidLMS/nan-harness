use super::support::{base_manifest, feed_for, feed_for_entries};
use crate::compatibility::evidence::apply_verifications;
use crate::compatibility::validation::validate_manifest;
use crate::compatibility::{
    CompatibilityError, VerificationEntry, VerificationManifest, VerificationRelease,
};
use nan_harness_core::HarnessKind;
use semver::Version;

#[test]
fn overlay_only_advances_known_compatible_versions() {
    let mut base = base_manifest();
    let original_policy = base.policy.clone();
    let original_minimum = base
        .entry(HarnessKind::Codex)
        .expect("Codex entry")
        .minimum_version
        .clone();
    let remote = VerificationManifest {
        schema_version: 2,
        releases: vec![VerificationRelease {
            nan_harness_version: Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
            verifications: vec![VerificationEntry {
                id: "codex".to_owned(),
                last_compatible_version: Some(Version::new(0, 147, 0)),
                compatible_at: Some("2026-08-19T08:00:00Z".to_owned()),
                last_live_verified_version: None,
                live_verified_at: None,
            }],
        }],
    };

    apply_verifications(&mut base, &remote.releases[0]).expect("overlay should apply");

    let codex = base.entry(HarnessKind::Codex).expect("Codex entry");
    assert_eq!(codex.last_compatible_version, Version::new(0, 147, 0));
    assert_eq!(codex.minimum_version, original_minimum);
    assert_eq!(base.policy, original_policy);
}

#[test]
fn overlay_never_regresses_the_embedded_compatible_version() {
    let mut base = base_manifest();
    let remote = VerificationRelease {
        nan_harness_version: Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
        verifications: vec![VerificationEntry {
            id: "codex".to_owned(),
            last_compatible_version: Some(Version::new(0, 145, 0)),
            compatible_at: Some("2026-08-19T08:00:00Z".to_owned()),
            last_live_verified_version: None,
            live_verified_at: None,
        }],
    };

    apply_verifications(&mut base, &remote).expect("overlay should apply");

    assert_eq!(
        base.entry(HarnessKind::Codex)
            .expect("Codex entry")
            .last_compatible_version,
        Version::new(0, 146, 0)
    );
}

#[test]
fn malformed_and_missing_evidence_pairs_are_rejected() {
    let base = base_manifest();
    let cases = [
        (
            VerificationEntry {
                id: "codex".to_owned(),
                last_compatible_version: Some(Version::new(0, 147, 0)),
                compatible_at: None,
                last_live_verified_version: None,
                live_verified_at: None,
            },
            "incomplete compatible pair",
        ),
        (
            VerificationEntry {
                id: "codex".to_owned(),
                last_compatible_version: None,
                compatible_at: None,
                last_live_verified_version: Some(Version::new(0, 147, 0)),
                live_verified_at: None,
            },
            "incomplete live pair",
        ),
        (
            VerificationEntry {
                id: "codex".to_owned(),
                last_compatible_version: None,
                compatible_at: None,
                last_live_verified_version: None,
                live_verified_at: None,
            },
            "missing evidence",
        ),
        (
            VerificationEntry {
                id: "codex".to_owned(),
                last_compatible_version: Some(Version::new(0, 147, 0)),
                compatible_at: Some("2026-08-19".to_owned()),
                last_live_verified_version: None,
                live_verified_at: None,
            },
            "malformed timestamp",
        ),
    ];

    for (entry, label) in cases {
        let manifest = feed_for(entry);
        assert!(validate_manifest(&manifest, &base).is_err(), "{label}");
    }
}

#[test]
fn evidence_versions_below_minimum_are_rejected() {
    let base = base_manifest();
    for entry in [
        VerificationEntry {
            id: "codex".to_owned(),
            last_compatible_version: Some(Version::new(0, 145, 0)),
            compatible_at: Some("2026-08-19T08:00:00Z".to_owned()),
            last_live_verified_version: None,
            live_verified_at: None,
        },
        VerificationEntry {
            id: "codex".to_owned(),
            last_compatible_version: None,
            compatible_at: None,
            last_live_verified_version: Some(Version::new(0, 145, 0)),
            live_verified_at: Some("2026-08-19T08:00:00Z".to_owned()),
        },
    ] {
        assert!(matches!(
            validate_manifest(&feed_for(entry), &base),
            Err(CompatibilityError::VersionBelowMinimum { .. }
                | CompatibilityError::LiveVersionBelowMinimum { .. },)
        ));
    }
}

#[test]
fn duplicate_releases_and_known_harnesses_are_rejected() {
    let current = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
    let valid = VerificationEntry {
        id: "codex".to_owned(),
        last_compatible_version: Some(Version::new(0, 147, 0)),
        compatible_at: Some("2026-08-19T08:00:00Z".to_owned()),
        last_live_verified_version: None,
        live_verified_at: None,
    };
    let duplicate_release = VerificationManifest {
        schema_version: 2,
        releases: vec![
            VerificationRelease {
                nan_harness_version: current.clone(),
                verifications: vec![valid.clone()],
            },
            VerificationRelease {
                nan_harness_version: current,
                verifications: vec![valid.clone()],
            },
        ],
    };
    assert!(matches!(
        validate_manifest(&duplicate_release, &base_manifest()),
        Err(CompatibilityError::DuplicateRelease(_))
    ));

    let duplicate_harness = feed_for_entries(vec![valid.clone(), valid]);
    assert!(matches!(
        validate_manifest(&duplicate_harness, &base_manifest()),
        Err(CompatibilityError::DuplicateHarness(HarnessKind::Codex))
    ));
}

#[test]
fn live_evidence_cannot_outpace_compatible_evidence_without_the_same_update() {
    let base = base_manifest();
    let ahead = VerificationEntry {
        id: "codex".to_owned(),
        last_compatible_version: Some(Version::new(0, 146, 0)),
        compatible_at: Some("2026-08-19T08:00:00Z".to_owned()),
        last_live_verified_version: Some(Version::new(0, 147, 0)),
        live_verified_at: Some("2026-08-20T08:00:00Z".to_owned()),
    };
    assert!(matches!(
        validate_manifest(&feed_for(ahead), &base),
        Err(CompatibilityError::LiveEvidenceAhead { .. })
    ));

    let advanced_together = VerificationEntry {
        id: "codex".to_owned(),
        last_compatible_version: Some(Version::new(0, 147, 0)),
        compatible_at: Some("2026-08-19T08:00:00Z".to_owned()),
        last_live_verified_version: Some(Version::new(0, 147, 0)),
        live_verified_at: Some("2026-08-20T08:00:00Z".to_owned()),
    };
    assert!(validate_manifest(&feed_for(advanced_together), &base).is_ok());
}

#[test]
fn unknown_future_harness_ids_are_validated_then_ignored() {
    let mut base = base_manifest();
    let before = base.clone();
    let unknown = VerificationEntry {
        id: "future-harness".to_owned(),
        last_compatible_version: Some(Version::new(99, 0, 0)),
        compatible_at: Some("2026-08-20T00:00:00Z".to_owned()),
        last_live_verified_version: None,
        live_verified_at: None,
    };
    let feed = feed_for(unknown);
    assert!(validate_manifest(&feed, &base).is_ok());
    apply_verifications(&mut base, &feed.releases[0]).expect("unknown IDs are ignored");
    assert_eq!(base, before);
}

#[test]
fn empty_release_lists_are_rejected() {
    let manifest = VerificationManifest {
        schema_version: 2,
        releases: Vec::new(),
    };

    assert!(matches!(
        validate_manifest(&manifest, &base_manifest()),
        Err(CompatibilityError::EmptyReleases)
    ));
}
