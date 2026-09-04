// Release selection and evidence merge: only the release matching the running
// version applies, unknown harness ids stay inert, evidence pairs merge
// atomically by version then real timestamp instant, and a lower remote record
// cannot rewrite part of an embedded entry.
use super::support::base_manifest;
use crate::compatibility::evidence::{apply_verifications, merge_evidence_pair, select_release};
use crate::compatibility::{
    CompatibilityError, VerificationEntry, VerificationManifest, VerificationRelease,
};
use nan_harness_core::HarnessKind;
use semver::Version;

#[test]
fn evidence_pairs_merge_atomically_using_version_then_real_timestamp_order() {
    let mut version = Some(Version::new(0, 146, 0));
    let mut timestamp = Some("2026-08-20T00:00:00Z".to_owned());
    merge_evidence_pair(
        &mut version,
        &mut timestamp,
        Some(&Version::new(0, 147, 0)),
        Some(&"2026-08-19T00:00:00Z".to_owned()),
        "codex",
        "compatible",
    )
    .expect("higher version should merge while preserving newer evidence");
    assert_eq!(version, Some(Version::new(0, 147, 0)));
    assert_eq!(timestamp.as_deref(), Some("2026-08-20T00:00:00Z"));

    timestamp = Some("2026-08-20T00:30:00+01:00".to_owned());
    merge_evidence_pair(
        &mut version,
        &mut timestamp,
        Some(&Version::new(0, 147, 0)),
        Some(&"2026-08-20T00:00:00Z".to_owned()),
        "codex",
        "compatible",
    )
    .expect("equal version with later instant should merge");
    assert_eq!(timestamp.as_deref(), Some("2026-08-20T00:00:00Z"));

    merge_evidence_pair(
        &mut version,
        &mut timestamp,
        Some(&Version::new(0, 147, 0)),
        Some(&"2026-08-20T01:00:00+01:00".to_owned()),
        "codex",
        "compatible",
    )
    .expect("equal version with same instant should be unchanged");
    assert_eq!(timestamp.as_deref(), Some("2026-08-20T00:00:00Z"));

    merge_evidence_pair(
        &mut version,
        &mut timestamp,
        Some(&Version::new(0, 146, 0)),
        Some(&"2026-08-21T00:00:00Z".to_owned()),
        "codex",
        "compatible",
    )
    .expect("lower version should be ignored");
    assert_eq!(version, Some(Version::new(0, 147, 0)));
    assert_eq!(timestamp.as_deref(), Some("2026-08-20T00:00:00Z"));

    merge_evidence_pair(
        &mut version,
        &mut timestamp,
        Some(&Version::new(0, 147, 0)),
        Some(&"2026-08-19T00:00:00Z".to_owned()),
        "codex",
        "compatible",
    )
    .expect("equal version with an older timestamp should be ignored");
    assert_eq!(timestamp.as_deref(), Some("2026-08-20T00:00:00Z"));

    let mut absent_version = None;
    let mut stray_timestamp = Some("2026-08-21T00:00:00Z".to_owned());
    merge_evidence_pair(
        &mut absent_version,
        &mut stray_timestamp,
        None,
        Some(&"2026-08-22T00:00:00Z".to_owned()),
        "codex",
        "compatible",
    )
    .expect("an update without a version should be ignored");
    assert_eq!(absent_version, None);
    assert_eq!(stray_timestamp.as_deref(), Some("2026-08-21T00:00:00Z"));

    let mut incomplete_version = Some(Version::new(0, 146, 0));
    let mut incomplete_timestamp = None;
    assert!(matches!(
        merge_evidence_pair(
            &mut incomplete_version,
            &mut incomplete_timestamp,
            Some(&Version::new(0, 147, 0)),
            Some(&"2026-08-22T00:00:00Z".to_owned()),
            "codex",
            "compatible",
        ),
        Err(CompatibilityError::IncompleteEvidencePair { .. })
    ));
}

#[test]
fn lower_remote_evidence_preserves_the_entire_embedded_record() {
    let mut base = base_manifest();
    let before = base.clone();
    let release = VerificationRelease {
        nan_harness_version: Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
        verifications: vec![VerificationEntry {
            id: "codex".to_owned(),
            last_compatible_version: Some(Version::new(0, 145, 0)),
            compatible_at: Some("2026-08-21T00:00:00Z".to_owned()),
            last_live_verified_version: Some(Version::new(0, 145, 0)),
            live_verified_at: Some("2026-08-21T00:00:00Z".to_owned()),
        }],
    };
    apply_verifications(&mut base, &release).expect("overlay should apply");
    assert_eq!(base, before);
}

#[test]
fn release_selection_is_exact_and_unknown_harnesses_are_ignored() {
    let mut base = base_manifest();
    let current = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
    let feed = VerificationManifest {
        schema_version: 2,
        releases: vec![
            VerificationRelease {
                nan_harness_version: current.clone(),
                verifications: vec![
                    VerificationEntry {
                        id: "codex".to_owned(),
                        last_compatible_version: Some(Version::new(0, 147, 0)),
                        compatible_at: Some("2026-08-19T08:00:00Z".to_owned()),
                        last_live_verified_version: None,
                        live_verified_at: None,
                    },
                    VerificationEntry {
                        id: "unknown-harness".to_owned(),
                        last_compatible_version: Some(Version::new(99, 0, 0)),
                        compatible_at: Some("2026-08-19T08:00:00Z".to_owned()),
                        last_live_verified_version: None,
                        live_verified_at: None,
                    },
                ],
            },
            VerificationRelease {
                nan_harness_version: Version::new(99, 0, 0),
                verifications: vec![VerificationEntry {
                    id: "unknown-harness".to_owned(),
                    last_compatible_version: Some(Version::new(99, 0, 0)),
                    compatible_at: Some("2026-08-19T08:00:00Z".to_owned()),
                    last_live_verified_version: None,
                    live_verified_at: None,
                }],
            },
        ],
    };

    let release = select_release(&feed, &current).expect("current release should match");
    apply_verifications(&mut base, release).expect("overlay should apply");
    assert_eq!(
        base.entry(HarnessKind::Codex)
            .expect("Codex entry")
            .last_compatible_version,
        Version::new(0, 147, 0)
    );
    assert!(select_release(&feed, &Version::new(1, 0, 0)).is_none());
}
