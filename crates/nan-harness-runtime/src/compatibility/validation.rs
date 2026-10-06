use super::desktop::{desktop_surfaces, validate_desktop_verifications};
use super::manifest::{
    HOSTED_FEED_SCHEMA_VERSION, LEGACY_FEED_SCHEMA_VERSION, UNIFIED_FEED_SCHEMA_VERSION,
    VERSIONED_FEED_SCHEMA_VERSION,
};
use super::{CompatibilityError, VerificationEntry, VerificationManifest};
use nan_harness_core::{CompatibilityManifest, HarnessKind};
use std::collections::BTreeSet;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

/// Validates a downloaded or cached feed.
///
/// The current v5 feed and older v2/v3/v4 feeds are accepted. Older feeds cannot
/// carry exact-version checks; v2 also leaves the embedded Desktop registry in effect.
pub(super) fn validate_manifest(
    manifest: &VerificationManifest,
    base: &CompatibilityManifest,
) -> Result<(), CompatibilityError> {
    let unified = match manifest.schema_version {
        UNIFIED_FEED_SCHEMA_VERSION
        | VERSIONED_FEED_SCHEMA_VERSION
        | HOSTED_FEED_SCHEMA_VERSION => true,
        LEGACY_FEED_SCHEMA_VERSION => false,
        version => return Err(CompatibilityError::UnsupportedManifestSchema(version)),
    };
    if manifest.releases.is_empty() {
        return Err(CompatibilityError::EmptyReleases);
    }
    let surfaces = if unified {
        Some(desktop_surfaces()?)
    } else {
        None
    };
    // Only the running release's records are checked against this binary's registry: records for
    // another release describe platforms and minimums this binary does not own.
    let running_version = semver::Version::parse(env!("CARGO_PKG_VERSION")).ok();
    let mut release_versions = BTreeSet::new();
    for release in &manifest.releases {
        if manifest.schema_version < VERSIONED_FEED_SCHEMA_VERSION
            && !release.desktop_checks.is_empty()
        {
            return Err(CompatibilityError::InvalidDesktopChecks(
                "checks require schema v4",
            ));
        }
        if manifest.schema_version != HOSTED_FEED_SCHEMA_VERSION
            && !release.hosted_checks.is_empty()
        {
            return Err(CompatibilityError::InvalidHostedChecks(
                "checks require schema v5",
            ));
        }
        super::hosted_checks::validate_checks(
            release,
            surfaces.as_deref().unwrap_or_default(),
            running_version.as_ref() == Some(&release.nan_harness_version),
        )?;
        super::desktop_checks::validate_checks(
            release,
            surfaces.as_deref().unwrap_or_default(),
            running_version.as_ref() == Some(&release.nan_harness_version),
        )?;
        if !release_versions.insert(release.nan_harness_version.clone()) {
            return Err(CompatibilityError::DuplicateRelease(
                release.nan_harness_version.clone(),
            ));
        }
        let mut ids = BTreeSet::new();
        for verification in &release.verifications {
            let id = validate_verification(verification, base)?;
            if let Some(id) = id
                && !ids.insert(id)
            {
                return Err(CompatibilityError::DuplicateHarness(id));
            }
        }
        match &surfaces {
            Some(surfaces) => validate_desktop_verifications(
                release,
                surfaces,
                running_version.as_ref() == Some(&release.nan_harness_version),
            )?,
            None if !release.desktop_verifications.is_empty() => {
                return Err(CompatibilityError::DesktopEvidenceInLegacyFeed);
            }
            None => {}
        }
    }
    Ok(())
}

fn validate_verification(
    verification: &VerificationEntry,
    base: &CompatibilityManifest,
) -> Result<Option<HarnessKind>, CompatibilityError> {
    let compatible_at = validate_evidence_pair(
        &verification.id,
        "compatible",
        verification.last_compatible_version.as_ref(),
        verification.compatible_at.as_ref(),
    )?;
    let live_at = validate_evidence_pair(
        &verification.id,
        "live",
        verification.last_live_verified_version.as_ref(),
        verification.live_verified_at.as_ref(),
    )?;
    if compatible_at.is_none() && live_at.is_none() {
        return Err(CompatibilityError::MissingEvidence {
            id: verification.id.clone(),
        });
    }

    let Ok(id) = verification.id.parse::<HarnessKind>() else {
        return Ok(None);
    };
    let Some(entry) = base.entry(id) else {
        return Ok(None);
    };
    if let Some(version) = &verification.last_compatible_version
        && version < &entry.minimum_version
    {
        return Err(CompatibilityError::VersionBelowMinimum {
            harness: id,
            version: version.clone(),
            minimum: entry.minimum_version.clone(),
        });
    }
    if let Some(version) = &verification.last_live_verified_version
        && version < &entry.minimum_version
    {
        return Err(CompatibilityError::LiveVersionBelowMinimum {
            harness: id,
            version: version.clone(),
            minimum: entry.minimum_version.clone(),
        });
    }
    if let Some(live_version) = &verification.last_live_verified_version {
        let compatible_version = verification
            .last_compatible_version
            .as_ref()
            .unwrap_or(&entry.last_compatible_version);
        if live_version > compatible_version {
            return Err(CompatibilityError::LiveEvidenceAhead {
                harness: id,
                live: live_version.clone(),
                compatible: compatible_version.clone(),
            });
        }
    }
    Ok(Some(id))
}

fn validate_evidence_pair(
    id: &str,
    track: &'static str,
    version: Option<&semver::Version>,
    timestamp: Option<&String>,
) -> Result<Option<OffsetDateTime>, CompatibilityError> {
    match (version, timestamp) {
        (None, None) => Ok(None),
        (Some(_), Some(timestamp)) => OffsetDateTime::parse(timestamp, &Rfc3339)
            .map(Some)
            .map_err(|_| CompatibilityError::InvalidEvidenceTimestamp {
                id: id.to_owned(),
                track,
                timestamp: timestamp.clone(),
            }),
        _ => Err(CompatibilityError::IncompleteEvidencePair {
            id: id.to_owned(),
            track,
        }),
    }
}
