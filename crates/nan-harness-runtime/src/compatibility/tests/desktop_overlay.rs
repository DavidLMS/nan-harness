use super::support::{base_manifest, desktop_feed, desktop_release, unified_feed};
use crate::compatibility::desktop::apply_desktop_verifications;
use crate::compatibility::validation::validate_manifest;
use crate::compatibility::{
    CompatibilityError, DesktopVerificationEntry, VerificationEntry, VerificationManifest,
    VerificationRelease,
};
use crate::desktop_compatibility::{
    DesktopCompatibilityEvidence, DesktopEvidenceSource, embedded_desktop_compatibility,
    embedded_desktop_surfaces,
};
use nan_harness_core::DesktopHarnessKind;
use semver::Version;

fn desktop_entry(
    id: &str,
    platform: &str,
    evidence: DesktopCompatibilityEvidence,
    app: Option<Version>,
    runtime: Option<Version>,
    compatible_at: &str,
) -> DesktopVerificationEntry {
    DesktopVerificationEntry {
        id: id.to_owned(),
        platform: platform.to_owned(),
        evidence,
        last_compatible_app_version: app,
        last_compatible_runtime_version: runtime,
        compatible_at: compatible_at.to_owned(),
    }
}

fn chatgpt_macos_record(
    app: Version,
    runtime: Version,
    compatible_at: &str,
) -> DesktopVerificationEntry {
    desktop_entry(
        "chatgpt-desktop",
        "macos",
        DesktopCompatibilityEvidence::LiveVerified,
        Some(app),
        Some(runtime),
        compatible_at,
    )
}

#[test]
fn desktop_overlay_advances_application_and_runtime_bounds_together() {
    let mut entry = embedded_desktop_compatibility(DesktopHarnessKind::ChatGpt, "macos")
        .expect("macOS ChatGPT record should exist");
    let release = desktop_release(vec![chatgpt_macos_record(
        Version::new(26, 831, 21537),
        Version::parse("0.152.0").expect("runtime version"),
        "2026-09-07T00:00:00Z",
    )]);

    assert!(apply_desktop_verifications(&mut entry, &release));

    assert_eq!(
        entry.last_compatible_app_version,
        Some(Version::new(26, 831, 21537))
    );
    assert_eq!(
        entry.last_compatible_runtime_version,
        Some(Version::parse("0.152.0").expect("runtime version"))
    );
    assert_eq!(entry.compatible_at, "2026-09-07T00:00:00Z");
    assert_eq!(entry.source, DesktopEvidenceSource::RemoteFeed);
    assert_eq!(
        entry.minimum_app_version,
        embedded_desktop_compatibility(DesktopHarnessKind::ChatGpt, "macos")
            .expect("macOS ChatGPT record should exist")
            .minimum_app_version,
        "remote evidence must not move the embedded minimum"
    );
}

#[test]
fn desktop_overlay_never_lowers_bounds_or_backdates_evidence() {
    let embedded = embedded_desktop_compatibility(DesktopHarnessKind::ChatGpt, "macos")
        .expect("macOS ChatGPT record should exist");
    let mut entry = embedded.clone();
    let release = desktop_release(vec![chatgpt_macos_record(
        Version::new(26, 800, 0),
        Version::parse("0.150.0").expect("runtime version"),
        "2026-08-01T00:00:00Z",
    )]);

    assert!(!apply_desktop_verifications(&mut entry, &release));
    assert_eq!(entry, embedded);
}

#[test]
fn desktop_overlay_ignores_other_surfaces_and_platforms() {
    let embedded = embedded_desktop_compatibility(DesktopHarnessKind::ChatGpt, "macos")
        .expect("macOS ChatGPT record should exist");
    for foreign in [
        desktop_entry(
            "chatgpt-desktop",
            "linux",
            DesktopCompatibilityEvidence::ContractOnly,
            Some(Version::new(99, 0, 0)),
            Some(Version::parse("9.0.0").expect("runtime version")),
            "2026-09-07T00:00:00Z",
        ),
        desktop_entry(
            "zed-desktop",
            "macos",
            DesktopCompatibilityEvidence::LiveVerified,
            Some(Version::new(99, 0, 0)),
            None,
            "2026-09-07T00:00:00Z",
        ),
    ] {
        let mut entry = embedded.clone();
        assert!(!apply_desktop_verifications(
            &mut entry,
            &desktop_release(vec![foreign])
        ));
        assert_eq!(entry, embedded);
    }
}

#[test]
fn every_registered_desktop_surface_and_platform_can_be_refreshed() {
    let surfaces = embedded_desktop_surfaces().expect("embedded registry should load");
    for kind in DesktopHarnessKind::ALL {
        let rows = surfaces
            .iter()
            .filter(|entry| entry.id == kind)
            .collect::<Vec<_>>();
        assert!(!rows.is_empty(), "{kind} must have platform records");
        for embedded in rows {
            if embedded.evidence == DesktopCompatibilityEvidence::Unavailable {
                continue;
            }
            let record = desktop_entry(
                &kind.to_string(),
                &embedded.platform,
                embedded.evidence,
                embedded.last_compatible_app_version.clone(),
                embedded.last_compatible_runtime_version.clone(),
                "2026-09-07T00:00:00Z",
            );
            let feed = unified_feed(Vec::new(), vec![record]);
            validate_manifest(&feed, &base_manifest())
                .unwrap_or_else(|error| panic!("{kind} on {}: {error}", embedded.platform));

            let mut entry = embedded.clone();
            assert!(
                apply_desktop_verifications(&mut entry, &feed.releases[0]),
                "{kind} on {} should accept refreshed evidence",
                embedded.platform
            );
            assert_eq!(entry.source, DesktopEvidenceSource::RemoteFeed);
        }
    }
}

#[test]
fn unknown_desktop_platforms_are_rejected() {
    let feed = desktop_feed(chatgpt_macos_record(
        Version::new(26, 831, 21537),
        Version::parse("0.152.0").expect("runtime version"),
        "2026-09-07T00:00:00Z",
    ));
    let mut invalid = feed.clone();
    invalid.releases[0].desktop_verifications[0].platform = "haiku".to_owned();

    assert!(matches!(
        validate_manifest(&invalid, &base_manifest()),
        Err(CompatibilityError::UnknownDesktopPlatform { .. })
    ));
}

#[test]
fn runtime_only_records_cannot_certify_the_application_pair() {
    let feed = desktop_feed(desktop_entry(
        "chatgpt-desktop",
        "macos",
        DesktopCompatibilityEvidence::LiveVerified,
        None,
        Some(Version::parse("0.152.0").expect("runtime version")),
        "2026-09-07T00:00:00Z",
    ));

    assert!(matches!(
        validate_manifest(&feed, &base_manifest()),
        Err(CompatibilityError::IncompleteDesktopEvidence {
            track: "application",
            ..
        })
    ));
}

#[test]
fn application_only_records_cannot_certify_the_runtime_pair() {
    let feed = desktop_feed(desktop_entry(
        "chatgpt-desktop",
        "macos",
        DesktopCompatibilityEvidence::LiveVerified,
        Some(Version::new(26, 831, 21537)),
        None,
        "2026-09-07T00:00:00Z",
    ));

    assert!(matches!(
        validate_manifest(&feed, &base_manifest()),
        Err(CompatibilityError::IncompleteDesktopEvidence {
            track: "runtime",
            ..
        })
    ));
}

#[test]
fn desktop_records_below_the_embedded_minimum_are_rejected() {
    for (app, runtime, track) in [
        (
            Some(Version::new(1, 0, 0)),
            Some(Version::parse("0.151.0-alpha.7.2").expect("runtime version")),
            "app",
        ),
        (
            Some(Version::new(26, 831, 21537)),
            Some(Version::parse("0.1.0").expect("runtime version")),
            "runtime",
        ),
    ] {
        let feed = desktop_feed(desktop_entry(
            "chatgpt-desktop",
            "macos",
            DesktopCompatibilityEvidence::LiveVerified,
            app,
            runtime,
            "2026-09-07T00:00:00Z",
        ));
        let error = validate_manifest(&feed, &base_manifest())
            .expect_err("evidence below the embedded minimum must be rejected");
        assert!(
            matches!(
                error,
                CompatibilityError::DesktopVersionBelowMinimum { track: reported, .. }
                    if reported == track
            ),
            "{track}: {error}"
        );
    }
}

#[test]
fn unavailable_surfaces_and_malformed_records_are_rejected() {
    let unavailable = desktop_feed(desktop_entry(
        "chatgpt-desktop",
        "macos",
        DesktopCompatibilityEvidence::Unavailable,
        None,
        None,
        "2026-09-07T00:00:00Z",
    ));
    assert!(matches!(
        validate_manifest(&unavailable, &base_manifest()),
        Err(CompatibilityError::UnavailableDesktopSurface { .. })
    ));

    let malformed = desktop_feed(chatgpt_macos_record(
        Version::new(26, 831, 21537),
        Version::parse("0.152.0").expect("runtime version"),
        "the seventh of September",
    ));
    assert!(matches!(
        validate_manifest(&malformed, &base_manifest()),
        Err(CompatibilityError::InvalidDesktopEvidenceTimestamp { .. })
    ));
}

#[test]
fn duplicate_desktop_surfaces_are_rejected_and_unknown_surfaces_are_ignored() {
    let record = chatgpt_macos_record(
        Version::new(26, 831, 21537),
        Version::parse("0.152.0").expect("runtime version"),
        "2026-09-07T00:00:00Z",
    );
    let duplicate = unified_feed(Vec::new(), vec![record.clone(), record]);
    assert!(matches!(
        validate_manifest(&duplicate, &base_manifest()),
        Err(CompatibilityError::DuplicateDesktopSurface { .. })
    ));

    let unknown = desktop_feed(desktop_entry(
        "future-desktop",
        "macos",
        DesktopCompatibilityEvidence::LiveVerified,
        None,
        None,
        "2026-09-07T00:00:00Z",
    ));
    assert!(validate_manifest(&unknown, &base_manifest()).is_ok());

    let mut entry = embedded_desktop_compatibility(DesktopHarnessKind::ChatGpt, "macos")
        .expect("macOS ChatGPT record should exist");
    let before = entry.clone();
    assert!(!apply_desktop_verifications(
        &mut entry,
        &unknown.releases[0]
    ));
    assert_eq!(entry, before);
}

#[test]
fn legacy_feeds_stay_cli_only_and_never_carry_desktop_evidence() {
    let cli_entry = VerificationEntry {
        id: "codex".to_owned(),
        last_compatible_version: Some(Version::new(0, 147, 0)),
        compatible_at: Some("2026-09-07T00:00:00Z".to_owned()),
        last_live_verified_version: None,
        live_verified_at: None,
    };
    let legacy = VerificationManifest {
        schema_version: 2,
        releases: vec![VerificationRelease {
            nan_harness_version: Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
            verifications: vec![cli_entry.clone()],
            desktop_verifications: Vec::new(),
        }],
    };
    assert!(validate_manifest(&legacy, &base_manifest()).is_ok());

    let mut smuggled = legacy;
    smuggled.releases[0].desktop_verifications = vec![chatgpt_macos_record(
        Version::new(26, 831, 21537),
        Version::parse("0.152.0").expect("runtime version"),
        "2026-09-07T00:00:00Z",
    )];
    assert!(matches!(
        validate_manifest(&smuggled, &base_manifest()),
        Err(CompatibilityError::DesktopEvidenceInLegacyFeed)
    ));

    let unified = unified_feed(vec![cli_entry], Vec::new());
    assert!(validate_manifest(&unified, &base_manifest()).is_ok());
}

#[test]
fn unsupported_feed_schemas_are_rejected() {
    for schema_version in [1_u8, 4] {
        let mut feed = unified_feed(Vec::new(), Vec::new());
        feed.schema_version = schema_version;
        assert!(matches!(
            validate_manifest(&feed, &base_manifest()),
            Err(CompatibilityError::UnsupportedManifestSchema(reported))
                if reported == schema_version
        ));
    }
}

/// The documented example both sides agree on. The producer validates the same file in
/// `xtask/src/release/tests/unified_compatibility.rs`, so a schema change that only one side
/// makes fails here or there.
const EXAMPLE_FEED_PATH: &str = "../../docs/examples/compatibility-v3.json";

#[test]
fn the_documented_example_feed_is_accepted_by_this_client() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(EXAMPLE_FEED_PATH);
    let contents = std::fs::read(&path).expect("the documented example feed should be readable");
    let feed: VerificationManifest =
        serde_json::from_slice(&contents).expect("the example feed should parse");

    validate_manifest(&feed, &base_manifest()).expect("the example feed should validate");

    let mut entry = embedded_desktop_compatibility(DesktopHarnessKind::ChatGpt, "macos")
        .expect("macOS ChatGPT record should exist");
    assert!(apply_desktop_verifications(&mut entry, &feed.releases[0]));
    assert_eq!(
        entry.last_compatible_app_version,
        Some(Version::new(26, 831, 21537))
    );
}
