use super::{CompatibilityError, VerificationManifest, VerificationRelease};
use nan_harness_core::{CompatibilityManifest, HarnessKind};
use semver::Version;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub(super) fn select_release<'a>(
    manifest: &'a VerificationManifest,
    version: &Version,
) -> Option<&'a VerificationRelease> {
    manifest
        .releases
        .iter()
        .find(|release| &release.nan_harness_version == version)
}

pub(super) fn apply_verifications(
    manifest: &mut CompatibilityManifest,
    release: &VerificationRelease,
) -> Result<(), CompatibilityError> {
    for verification in &release.verifications {
        let Ok(id) = verification.id.parse::<HarnessKind>() else {
            continue;
        };
        let Some(entry) = manifest.harnesses.iter_mut().find(|entry| entry.id == id) else {
            continue;
        };
        let mut compatible_version = Some(entry.last_compatible_version.clone());
        let mut compatible_at = Some(entry.compatible_at.clone());
        merge_evidence_pair(
            &mut compatible_version,
            &mut compatible_at,
            verification.last_compatible_version.as_ref(),
            verification.compatible_at.as_ref(),
            &verification.id,
            "compatible",
        )?;
        let mut live_version = entry.last_live_verified_version.clone();
        let mut live_at = entry.live_verified_at.clone();
        merge_evidence_pair(
            &mut live_version,
            &mut live_at,
            verification.last_live_verified_version.as_ref(),
            verification.live_verified_at.as_ref(),
            &verification.id,
            "live",
        )?;
        if let Some(version) = compatible_version {
            entry.last_compatible_version = version;
        }
        if let Some(timestamp) = compatible_at {
            entry.compatible_at = timestamp;
        }
        entry.last_live_verified_version = live_version;
        entry.live_verified_at = live_at;
    }
    Ok(())
}

pub(super) fn merge_evidence_pair(
    current_version: &mut Option<Version>,
    current_at: &mut Option<String>,
    update_version: Option<&Version>,
    update_at: Option<&String>,
    id: &str,
    track: &'static str,
) -> Result<(), CompatibilityError> {
    let Some((update_version, update_at, update_instant)) =
        validate_update_evidence(update_version, update_at, id, track)?
    else {
        return Ok(());
    };

    let Some((current_version_value, current_at_value)) =
        current_evidence_pair(current_version.as_ref(), current_at.as_ref(), id, track)?
    else {
        *current_version = Some(update_version.clone());
        *current_at = Some(update_at.clone());
        return Ok(());
    };
    merge_existing_evidence(
        current_version,
        current_at,
        &current_version_value,
        &current_at_value,
        update_version,
        update_at,
        update_instant,
        id,
        track,
    )
}

fn validate_update_evidence<'a>(
    version: Option<&'a Version>,
    timestamp: Option<&'a String>,
    id: &str,
    track: &'static str,
) -> Result<Option<(&'a Version, &'a String, OffsetDateTime)>, CompatibilityError> {
    let Some(version) = version else {
        return Ok(None);
    };
    let Some(timestamp) = timestamp else {
        return Err(CompatibilityError::IncompleteEvidencePair {
            id: id.to_owned(),
            track,
        });
    };
    let instant = OffsetDateTime::parse(timestamp, &Rfc3339).map_err(|_| {
        CompatibilityError::InvalidEvidenceTimestamp {
            id: id.to_owned(),
            track,
            timestamp: timestamp.clone(),
        }
    })?;
    Ok(Some((version, timestamp, instant)))
}

fn current_evidence_pair(
    version: Option<&Version>,
    timestamp: Option<&String>,
    id: &str,
    track: &'static str,
) -> Result<Option<(Version, String)>, CompatibilityError> {
    match (version, timestamp) {
        (None, None) => Ok(None),
        (Some(version), Some(timestamp)) => Ok(Some((version.clone(), timestamp.clone()))),
        _ => Err(CompatibilityError::IncompleteEvidencePair {
            id: id.to_owned(),
            track,
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn merge_existing_evidence(
    current_version: &mut Option<Version>,
    current_at: &mut Option<String>,
    current_version_value: &Version,
    current_at_value: &str,
    update_version: &Version,
    update_at: &str,
    update_instant: OffsetDateTime,
    id: &str,
    track: &'static str,
) -> Result<(), CompatibilityError> {
    match update_version.cmp(current_version_value) {
        std::cmp::Ordering::Greater => {
            *current_version = Some(update_version.clone());
            *current_at = Some(newer_timestamp(
                current_at_value,
                update_at,
                update_instant,
                id,
                track,
            )?);
        }
        std::cmp::Ordering::Equal => {
            if timestamp_is_newer(current_at_value, update_instant, id, track)? {
                *current_at = Some(update_at.to_owned());
            }
        }
        std::cmp::Ordering::Less => {}
    }
    Ok(())
}

fn newer_timestamp(
    current_at: &str,
    update_at: &str,
    update_instant: OffsetDateTime,
    id: &str,
    track: &'static str,
) -> Result<String, CompatibilityError> {
    if timestamp_is_newer(current_at, update_instant, id, track)? {
        Ok(update_at.to_owned())
    } else {
        Ok(current_at.to_owned())
    }
}

fn timestamp_is_newer(
    current_at: &str,
    update_instant: OffsetDateTime,
    id: &str,
    track: &'static str,
) -> Result<bool, CompatibilityError> {
    let current_instant = OffsetDateTime::parse(current_at, &Rfc3339).map_err(|_| {
        CompatibilityError::InvalidEvidenceTimestamp {
            id: id.to_owned(),
            track,
            timestamp: current_at.to_owned(),
        }
    })?;
    Ok(update_instant > current_instant)
}
