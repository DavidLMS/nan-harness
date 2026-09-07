//! Validation and application of the Desktop evidence carried by the unified feed.
//!
//! Remote evidence may only refine what this binary already certifies: dates, upper version
//! bounds and the evidence classification. Minimum versions, platforms, transports and the
//! availability of a surface stay embedded, so a feed can never widen the supported matrix.

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

/// Loads the embedded Desktop registry for feed validation.
pub(super) fn desktop_surfaces() -> Result<Vec<DesktopCompatibilityEntry>, CompatibilityError> {
    embedded_desktop_surfaces()
        .map_err(|error| CompatibilityError::InvalidEmbeddedDesktopRegistry(error.to_string()))
}

/// Validates every Desktop record of one release against the embedded registry.
pub(super) fn validate_desktop_verifications(
    release: &VerificationRelease,
    surfaces: &[DesktopCompatibilityEntry],
) -> Result<(), CompatibilityError> {
    let mut seen = BTreeSet::new();
    for verification in &release.desktop_verifications {
        let Some(id) = validate_desktop_verification(verification, surfaces)? else {
            continue;
        };
        if !seen.insert((id, verification.platform.clone())) {
            return Err(CompatibilityError::DuplicateDesktopSurface {
                id,
                platform: verification.platform.clone(),
            });
        }
    }
    Ok(())
}

/// Validates one Desktop record. Unknown surface identifiers are reserved for future releases and
/// are ignored after the record's own shape has been checked.
fn validate_desktop_verification(
    verification: &DesktopVerificationEntry,
    surfaces: &[DesktopCompatibilityEntry],
) -> Result<Option<DesktopHarnessKind>, CompatibilityError> {
    let Ok(id) = verification.id.parse::<DesktopHarnessKind>() else {
        return Ok(None);
    };
    let platform = verification.platform.clone();
    let Some(embedded) = surfaces
        .iter()
        .find(|entry| entry.id == id && entry.platform == platform)
    else {
        return Err(CompatibilityError::UnknownDesktopPlatform { id, platform });
    };
    if evidence_instant(&verification.compatible_at).is_none() {
        return Err(CompatibilityError::InvalidDesktopEvidenceTimestamp {
            id,
            platform,
            timestamp: verification.compatible_at.clone(),
        });
    }
    if embedded.evidence == DesktopCompatibilityEvidence::Unavailable
        || verification.evidence == DesktopCompatibilityEvidence::Unavailable
    {
        return Err(CompatibilityError::UnavailableDesktopSurface { id, platform });
    }
    validate_version_bound(
        id,
        &platform,
        "app",
        verification.last_compatible_app_version.as_ref(),
        embedded.minimum_app_version.as_ref(),
    )?;
    validate_version_bound(
        id,
        &platform,
        "runtime",
        verification.last_compatible_runtime_version.as_ref(),
        embedded.minimum_runtime_version.as_ref(),
    )?;
    if verification.evidence == DesktopCompatibilityEvidence::LiveVerified {
        require_certified_pair(verification, embedded, id, &platform)?;
    }
    Ok(Some(id))
}

/// A record certifies the application and its bundled runtime together. Live verification is only
/// credible when the record carries every bound the embedded surface tracks, so a runtime-only or
/// application-only record can never advertise a verified pair.
fn require_certified_pair(
    verification: &DesktopVerificationEntry,
    embedded: &DesktopCompatibilityEntry,
    id: DesktopHarnessKind,
    platform: &str,
) -> Result<(), CompatibilityError> {
    let missing_app = embedded.minimum_app_version.is_some()
        && verification.last_compatible_app_version.is_none();
    let missing_runtime = embedded.minimum_runtime_version.is_some()
        && verification.last_compatible_runtime_version.is_none();
    let track = if missing_app {
        "application"
    } else if missing_runtime {
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

/// Overlays the release's evidence for one effective entry, in place.
///
/// Returns `true` when a remote record actually changed the entry, so callers can report the
/// effective source honestly. Records for another platform or another surface are ignored.
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
    if entry.evidence == DesktopCompatibilityEvidence::Unavailable
        || verification.evidence == DesktopCompatibilityEvidence::Unavailable
    {
        return false;
    }
    let Some(remote_instant) = evidence_instant(&verification.compatible_at) else {
        return false;
    };
    if !evidence_is_newer(entry, verification, remote_instant) {
        return false;
    }
    advance_bound(
        &mut entry.last_compatible_app_version,
        verification.last_compatible_app_version.as_ref(),
    );
    advance_bound(
        &mut entry.last_compatible_runtime_version,
        verification.last_compatible_runtime_version.as_ref(),
    );
    entry.evidence = verification.evidence;
    entry.compatible_at = verification.compatible_at.clone();
    entry.source = DesktopEvidenceSource::RemoteFeed;
    true
}

/// Remote evidence is only adopted when it is not older than the embedded record: either it
/// advances a version bound, or it re-states the same bounds at a later date.
fn evidence_is_newer(
    entry: &DesktopCompatibilityEntry,
    verification: &DesktopVerificationEntry,
    remote_instant: OffsetDateTime,
) -> bool {
    if bound_advances(
        entry.last_compatible_app_version.as_ref(),
        verification.last_compatible_app_version.as_ref(),
    ) || bound_advances(
        entry.last_compatible_runtime_version.as_ref(),
        verification.last_compatible_runtime_version.as_ref(),
    ) {
        return true;
    }
    evidence_instant(&entry.compatible_at)
        .is_some_and(|embedded_instant| remote_instant > embedded_instant)
}

fn bound_advances(current: Option<&Version>, update: Option<&Version>) -> bool {
    match (current, update) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(current), Some(update)) => update > current,
    }
}

fn advance_bound(current: &mut Option<Version>, update: Option<&Version>) {
    if bound_advances(current.as_ref(), update) {
        *current = update.cloned();
    }
}

/// Accepts the RFC 3339 timestamps used by the feed and the plain `YYYY-MM-DD` dates used by the
/// embedded registry, so both can be ordered against each other.
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
