//! Hosted live checks stay model-scoped and never become legacy global live claims.

use super::{CompatibilityError, VerificationRelease};
use nan_harness_core::{CompatibilityManifest, HostedCheck, HostedOutcome, HostedSuite};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub(super) fn validate_checks(
    release: &VerificationRelease,
    surfaces: &[crate::desktop_compatibility::DesktopCompatibilityEntry],
    embedded_release: bool,
) -> Result<(), CompatibilityError> {
    for (index, check) in release.hosted_checks.iter().enumerate() {
        check
            .validate_identity()
            .map_err(CompatibilityError::InvalidHostedChecks)?;
        if OffsetDateTime::parse(&check.checked_at, &Rfc3339).is_err() {
            return Err(CompatibilityError::InvalidHostedChecks("invalid timestamp"));
        }
        if release.hosted_checks[..index]
            .iter()
            .any(|other| check.same_target(other) && check.evidence_sha256 == other.evidence_sha256)
        {
            return Err(CompatibilityError::InvalidHostedChecks(
                "duplicate observation",
            ));
        }
        if embedded_release
            && check.suite == HostedSuite::Desktop
            && check.outcome == HostedOutcome::Passed
        {
            let id = check
                .id
                .parse()
                .map_err(|_| CompatibilityError::InvalidHostedChecks("unknown desktop app"))?;
            let pair = super::DesktopVerificationEntry {
                id: check.id.clone(),
                platform: check.platform.clone(),
                evidence: crate::desktop_compatibility::DesktopCompatibilityEvidence::LiveVerified,
                last_compatible_app_version: Some(check.harness_version.clone()),
                last_compatible_runtime_version: check.runtime_version.clone(),
                compatible_at: check.checked_at.clone(),
            };
            super::desktop::validate_against_embedded(&pair, id, &check.platform, surfaces)?;
        }
    }
    Ok(())
}

fn native_deterministic_success(check: &HostedCheck, suite: HostedSuite) -> bool {
    let system = if cfg!(target_os = "macos") {
        "macos"
    } else {
        std::env::consts::OS
    };
    check.suite == suite
        && check.outcome == HostedOutcome::Passed
        && check.model.is_none()
        && check.platform == system
        && check.architecture == std::env::consts::ARCH
}

pub(super) fn apply_cli(
    manifest: &mut CompatibilityManifest,
    release: &VerificationRelease,
) -> Result<(), CompatibilityError> {
    for check in &release.hosted_checks {
        if !native_deterministic_success(check, HostedSuite::Cli) {
            continue;
        }
        let Ok(id) = check.id.parse::<nan_harness_core::HarnessKind>() else {
            continue;
        };
        let Some(entry) = manifest.harnesses.iter_mut().find(|entry| entry.id == id) else {
            continue;
        };
        if check.harness_version < entry.minimum_version {
            continue;
        }
        let mut version = Some(entry.last_compatible_version.clone());
        let mut timestamp = Some(entry.compatible_at.clone());
        super::evidence::merge_evidence_pair(
            &mut version,
            &mut timestamp,
            Some(&check.harness_version),
            Some(&check.checked_at),
            &check.id,
            "compatible",
        )?;
        if let (Some(version), Some(timestamp)) = (version, timestamp) {
            entry.last_compatible_version = version;
            entry.compatible_at = timestamp;
        }
    }
    Ok(())
}

pub(super) fn apply_desktop(
    entry: &mut crate::desktop_compatibility::DesktopCompatibilityEntry,
    release: &VerificationRelease,
) {
    for check in &release.hosted_checks {
        if !native_deterministic_success(check, HostedSuite::Desktop)
            || check.id.parse().ok() != Some(entry.id)
        {
            continue;
        }
        let observation = nan_harness_core::DesktopCheck {
            id: entry.id,
            platform: check.platform.clone(),
            architecture: check.architecture.clone(),
            app_version: check.harness_version.clone(),
            runtime_version: check.runtime_version.clone(),
            deterministic_at: Some(check.checked_at.clone()),
            live_verified_at: None,
        };
        if !entry
            .checks
            .iter()
            .any(|current| current.same_target(&observation))
        {
            entry.checks.push(observation);
        }
    }
}
