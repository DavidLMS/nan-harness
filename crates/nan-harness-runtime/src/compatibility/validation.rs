use super::{CompatibilityError, VerificationEntry, VerificationManifest};
use nan_harness_core::{CompatibilityManifest, HarnessKind};
use std::collections::BTreeSet;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const MANIFEST_SCHEMA_VERSION: u8 = 2;

pub(super) fn validate_manifest(
    manifest: &VerificationManifest,
    base: &CompatibilityManifest,
) -> Result<(), CompatibilityError> {
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(CompatibilityError::UnsupportedManifestSchema(
            manifest.schema_version,
        ));
    }
    if manifest.releases.is_empty() {
        return Err(CompatibilityError::EmptyReleases);
    }
    let mut release_versions = BTreeSet::new();
    for release in &manifest.releases {
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
