//! Desktop evidence carried by the unified feed.
//!
//! Remote evidence refines what this binary already certifies: dates, version bounds and the
//! evidence classification. Minimum versions, platforms, transports and availability stay
//! embedded. A record is adopted whole or not at all, so published bounds can never be combined
//! into a pair that was never verified together.

use super::manifest::DesktopVerificationEntry;
use super::{CompatibilityError, VerificationRelease};
use crate::desktop_compatibility::{
    DesktopCompatibilityEntry, DesktopCompatibilityEvidence, DesktopEvidenceSource,
    embedded_desktop_surfaces,
};
use nan_harness_core::DesktopHarnessKind;
use semver::Version;
use std::collections::BTreeSet;
use time::{Date, Month, OffsetDateTime, Time, UtcOffset, format_description::well_known::Rfc3339};

pub(super) fn desktop_surfaces() -> Result<Vec<DesktopCompatibilityEntry>, CompatibilityError> {
    embedded_desktop_surfaces()
        .map_err(|error| CompatibilityError::InvalidEmbeddedDesktopRegistry(error.to_string()))
}

/// Validates the Desktop records of one release.
///
/// `embedded_release` is true only for the record set this binary can actually consume. Records
/// published for another release are checked for shape alone: their platforms and minimums belong
/// to a different binary, and rejecting them would discard the feed's history.
pub(super) fn validate_desktop_verifications(
    release: &VerificationRelease,
    surfaces: &[DesktopCompatibilityEntry],
    embedded_release: bool,
) -> Result<(), CompatibilityError> {
    let mut seen = BTreeSet::new();
    for verification in &release.desktop_verifications {
        let Ok(id) = verification.id.parse::<DesktopHarnessKind>() else {
            continue;
        };
        let platform = verification.platform.clone();
        validate_structure(verification, id, &platform)?;
        if embedded_release {
            validate_against_embedded(verification, id, &platform, surfaces)?;
        }
        if !seen.insert((id, platform.clone())) {
            return Err(CompatibilityError::DuplicateDesktopSurface { id, platform });
        }
    }
    Ok(())
}

fn validate_structure(
    verification: &DesktopVerificationEntry,
    id: DesktopHarnessKind,
    platform: &str,
) -> Result<(), CompatibilityError> {
    if evidence_instant(&verification.compatible_at).is_none() {
        return Err(CompatibilityError::InvalidDesktopEvidenceTimestamp {
            id,
            platform: platform.to_owned(),
            timestamp: verification.compatible_at.clone(),
        });
    }
    // `unavailable` describes a surface this binary does not offer; it is never published.
    if verification.evidence == DesktopCompatibilityEvidence::Unavailable {
        return Err(CompatibilityError::UnavailableDesktopSurface {
            id,
            platform: platform.to_owned(),
        });
    }
    // Live verification names the application version that was run, so a surface with no
    // application bound can only ever be contract evidence.
    if verification.evidence == DesktopCompatibilityEvidence::LiveVerified
        && verification.last_compatible_app_version.is_none()
    {
        return Err(CompatibilityError::IncompleteDesktopEvidence {
            id,
            platform: platform.to_owned(),
            track: "application",
        });
    }
    Ok(())
}

fn validate_against_embedded(
    verification: &DesktopVerificationEntry,
    id: DesktopHarnessKind,
    platform: &str,
    surfaces: &[DesktopCompatibilityEntry],
) -> Result<(), CompatibilityError> {
    let Some(embedded) = surfaces
        .iter()
        .find(|entry| entry.id == id && entry.platform == platform)
    else {
        return Err(CompatibilityError::UnknownDesktopPlatform {
            id,
            platform: platform.to_owned(),
        });
    };
    if embedded.evidence == DesktopCompatibilityEvidence::Unavailable {
        return Err(CompatibilityError::UnavailableDesktopSurface {
            id,
            platform: platform.to_owned(),
        });
    }
    validate_version_bound(
        id,
        platform,
        "app",
        verification.last_compatible_app_version.as_ref(),
        embedded.minimum_app_version.as_ref(),
    )?;
    validate_version_bound(
        id,
        platform,
        "runtime",
        verification.last_compatible_runtime_version.as_ref(),
        embedded.minimum_runtime_version.as_ref(),
    )?;
    if verification.evidence == DesktopCompatibilityEvidence::LiveVerified {
        require_certified_pair(verification, embedded, id, platform)?;
    }
    Ok(())
}

/// Live verification certifies the application and its bundled runtime together: a record must
/// carry every bound the embedded surface tracks.
fn require_certified_pair(
    verification: &DesktopVerificationEntry,
    embedded: &DesktopCompatibilityEntry,
    id: DesktopHarnessKind,
    platform: &str,
) -> Result<(), CompatibilityError> {
    let track = if embedded.minimum_app_version.is_some()
        && verification.last_compatible_app_version.is_none()
    {
        "application"
    } else if embedded.minimum_runtime_version.is_some()
        && verification.last_compatible_runtime_version.is_none()
    {
        "runtime"
    } else {
        return Ok(());
    };
    Err(CompatibilityError::IncompleteDesktopEvidence {
        id,
        platform: platform.to_owned(),
        track,
    })
}

fn validate_version_bound(
    id: DesktopHarnessKind,
    platform: &str,
    track: &'static str,
    version: Option<&Version>,
    minimum: Option<&Version>,
) -> Result<(), CompatibilityError> {
    if let (Some(version), Some(minimum)) = (version, minimum)
        && version < minimum
    {
        return Err(CompatibilityError::DesktopVersionBelowMinimum {
            id,
            platform: platform.to_owned(),
            track,
            version: version.clone(),
            minimum: minimum.clone(),
        });
    }
    Ok(())
}

/// Overlays the release's evidence onto one effective entry, in place.
///
/// Returns whether a record was adopted, so callers can report the effective source honestly.
pub(super) fn apply_desktop_verifications(
    entry: &mut DesktopCompatibilityEntry,
    release: &VerificationRelease,
) -> bool {
    let Some(verification) = release.desktop_verifications.iter().find(|verification| {
        verification.id.parse::<DesktopHarnessKind>() == Ok(entry.id)
            && verification.platform == entry.platform
    }) else {
        return false;
    };
    if !adopts(entry, verification) {
        return false;
    }
    // Adoption is atomic: the record's own pair replaces the current one, never a per-track
    // maximum of two separately verified results.
    entry.last_compatible_app_version = verification.last_compatible_app_version.clone();
    entry.last_compatible_runtime_version = verification.last_compatible_runtime_version.clone();
    entry.evidence = verification.evidence;
    entry.compatible_at = verification.compatible_at.clone();
    entry.source = DesktopEvidenceSource::RemoteFeed;
    true
}

fn adopts(entry: &DesktopCompatibilityEntry, verification: &DesktopVerificationEntry) -> bool {
    let (Some(current_instant), Some(remote_instant)) = (
        evidence_instant(&entry.compatible_at),
        evidence_instant(&verification.compatible_at),
    ) else {
        return false;
    };
    if remote_instant < current_instant {
        return false;
    }
    // Live evidence names the application version that was run.
    if verification.evidence == DesktopCompatibilityEvidence::LiveVerified
        && verification.last_compatible_app_version.is_none()
    {
        return false;
    }
    match (entry.evidence, verification.evidence) {
        (_, DesktopCompatibilityEvidence::Unavailable)
        | (DesktopCompatibilityEvidence::Unavailable, _)
        // Live verification is never traded for a contract-only claim, whatever its date.
        | (
            DesktopCompatibilityEvidence::LiveVerified,
            DesktopCompatibilityEvidence::ContractOnly,
        ) => false,
        // A promotion replaces placeholder bounds with the pair that was actually verified, so it
        // is adopted even when that pair is lower than the contract-only bounds it replaces.
        (
            DesktopCompatibilityEvidence::ContractOnly,
            DesktopCompatibilityEvidence::LiveVerified,
        ) => true,
        _ => {
            pair_does_not_regress(entry, verification)
                && (pair_advances(entry, verification) || remote_instant > current_instant)
        }
    }
}

fn pair_does_not_regress(
    entry: &DesktopCompatibilityEntry,
    verification: &DesktopVerificationEntry,
) -> bool {
    bound_does_not_regress(
        entry.last_compatible_app_version.as_ref(),
        verification.last_compatible_app_version.as_ref(),
    ) && bound_does_not_regress(
        entry.last_compatible_runtime_version.as_ref(),
        verification.last_compatible_runtime_version.as_ref(),
    )
}

fn pair_advances(
    entry: &DesktopCompatibilityEntry,
    verification: &DesktopVerificationEntry,
) -> bool {
    bound_advances(
        entry.last_compatible_app_version.as_ref(),
        verification.last_compatible_app_version.as_ref(),
    ) || bound_advances(
        entry.last_compatible_runtime_version.as_ref(),
        verification.last_compatible_runtime_version.as_ref(),
    )
}

fn bound_does_not_regress(current: Option<&Version>, update: Option<&Version>) -> bool {
    match (current, update) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(current), Some(update)) => update >= current,
    }
}

fn bound_advances(current: Option<&Version>, update: Option<&Version>) -> bool {
    match (current, update) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(current), Some(update)) => update > current,
    }
}

/// Accepts feed timestamps (RFC 3339) and embedded registry dates (`YYYY-MM-DD`).
fn evidence_instant(value: &str) -> Option<OffsetDateTime> {
    if let Ok(instant) = OffsetDateTime::parse(value, &Rfc3339) {
        return Some(instant);
    }
    let (year, remainder) = value.split_once('-')?;
    let (month, day) = remainder.split_once('-')?;
    let date = Date::from_calendar_date(
        year.parse().ok()?,
        Month::try_from(month.parse::<u8>().ok()?).ok()?,
        day.parse().ok()?,
    )
    .ok()?;
    Some(OffsetDateTime::new_in_offset(
        date,
        Time::MIDNIGHT,
        UtcOffset::UTC,
    ))
}
