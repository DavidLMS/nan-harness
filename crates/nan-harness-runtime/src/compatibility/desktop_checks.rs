//! Exact-version checks never replace the older platform-wide evidence.
use super::{CompatibilityError, DesktopVerificationEntry, VerificationRelease};
use crate::desktop_compatibility::{DesktopCompatibilityEntry, DesktopCompatibilityEvidence};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub(super) fn validate_checks(
    release: &VerificationRelease,
    surfaces: &[DesktopCompatibilityEntry],
    embedded_release: bool,
) -> Result<(), CompatibilityError> {
    for (index, check) in release.desktop_checks.iter().enumerate() {
        if !matches!(check.platform.as_str(), "macos" | "linux" | "windows")
            || !matches!(check.architecture.as_str(), "aarch64" | "x86_64")
        {
            return Err(CompatibilityError::InvalidDesktopChecks(
                "unknown platform or architecture",
            ));
        }
        if check.deterministic_at.is_none() && check.live_verified_at.is_none() {
            return Err(CompatibilityError::InvalidDesktopChecks(
                "missing success evidence",
            ));
        }
        for timestamp in [&check.deterministic_at, &check.live_verified_at]
            .into_iter()
            .flatten()
        {
            if OffsetDateTime::parse(timestamp, &Rfc3339).is_err() {
                return Err(CompatibilityError::InvalidDesktopChecks(
                    "invalid timestamp",
                ));
            }
        }
        if release.desktop_checks[..index]
            .iter()
            .any(|other| check.same_target(other))
        {
            return Err(CompatibilityError::InvalidDesktopChecks(
                "duplicate exact target",
            ));
        }
        if embedded_release {
            let pair = DesktopVerificationEntry {
                id: check.id.to_string(),
                platform: check.platform.clone(),
                // Both functional tracks require the complete certified pair. This temporary
                // validation view is never stored or used to claim a live check.
                evidence: DesktopCompatibilityEvidence::LiveVerified,
                last_compatible_app_version: Some(check.app_version.clone()),
                last_compatible_runtime_version: check.runtime_version.clone(),
                compatible_at: String::new(),
            };
            super::desktop::validate_against_embedded(&pair, check.id, &check.platform, surfaces)?;
        }
    }
    Ok(())
}

pub(super) fn apply_checks(entry: &mut DesktopCompatibilityEntry, release: &VerificationRelease) {
    entry.checks = release
        .desktop_checks
        .iter()
        .filter(|check| {
            check.id == entry.id
                && check.platform == entry.platform
                && check.architecture == std::env::consts::ARCH
        })
        .cloned()
        .collect();
}
