//! A later failure must not erase the last success for an exact native target.

use super::verification::VerificationRelease;
use nan_harness_core::{HostedCheck, HostedOutcome};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

fn checked_at(check: &HostedCheck) -> Result<OffsetDateTime, String> {
    OffsetDateTime::parse(&check.checked_at, &Rfc3339)
        .map_err(|_| "hosted check has an invalid timestamp".to_owned())
}

pub(super) fn validate_checks(
    release: &VerificationRelease,
    requirements: Option<&super::desktop::DesktopRequirements>,
) -> Result<(), String> {
    validate_checks_for_release(
        release,
        requirements,
        &super::verification::current_release_version(),
    )
}

fn validate_checks_for_release(
    release: &VerificationRelease,
    requirements: Option<&super::desktop::DesktopRequirements>,
    registry_version: &semver::Version,
) -> Result<(), String> {
    for (index, check) in release.hosted_checks.iter().enumerate() {
        check.validate_identity().map_err(str::to_owned)?;
        checked_at(check)?;
        if release.hosted_checks[..index]
            .iter()
            .any(|other| check.same_target(other) && check.evidence_sha256 == other.evidence_sha256)
        {
            return Err("duplicate hosted observation".to_owned());
        }
        if check.suite == nan_harness_core::HostedSuite::Desktop
            && check.outcome == HostedOutcome::Passed
        {
            let entry = super::desktop::DesktopVerificationEntry {
                id: check.id.clone(),
                platform: check.platform.clone(),
                evidence: "live-verified".into(),
                last_compatible_app_version: Some(check.harness_version.clone()),
                last_compatible_runtime_version: check.runtime_version.clone(),
                compatible_at: check.checked_at.clone(),
            };
            super::desktop::validate_desktop_verifications(
                &[entry],
                requirements.ok_or_else(|| {
                    "hosted desktop checks require the trusted registry".to_owned()
                })?,
                release.nan_harness_version == *registry_version,
                "hosted checks",
            )?;
        }
    }
    Ok(())
}

/// The caller authenticates this registry at the tested release's attested commit.
pub(crate) fn merge_release_hosted_checks(
    base: &std::path::Path,
    updates: &std::path::Path,
    registry: &std::path::Path,
    version: &str,
    output: &std::path::Path,
) -> Result<(), String> {
    let version = semver::Version::parse(version)
        .map_err(|_| "tested release version is invalid".to_owned())?;
    let requirements = super::desktop::desktop_requirements_from_path(registry)?;
    for entry in std::fs::read_dir(updates).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        super::validation::require_regular_file(&path)?;
        let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
        let update: VerificationRelease =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if update.nan_harness_version != version
            || !update.verifications.is_empty()
            || !update.desktop_verifications.is_empty()
            || !update.desktop_checks.is_empty()
            || update.hosted_checks.is_empty()
        {
            return Err(
                "hosted updates must contain only checks for the authenticated release".into(),
            );
        }
        validate_checks_for_release(&update, Some(&requirements), &version)?;
    }
    super::compatibility::merge_hosted_compatibility_feed(base, updates, output)
}

pub(super) fn merge_check(
    checks: &mut Vec<HostedCheck>,
    update: &HostedCheck,
) -> Result<(), String> {
    let mut latest = update.clone();
    let mut success = (update.outcome == HostedOutcome::Passed).then(|| update.clone());
    for previous in checks.iter().filter(|check| check.same_target(update)) {
        if previous.evidence_sha256 == update.evidence_sha256 && previous != update {
            return Err("the same evidence digest cannot change its observation".to_owned());
        }
        if checked_at(previous)? > checked_at(&latest)? {
            latest = previous.clone();
        }
        if previous.outcome == HostedOutcome::Passed
            && success.as_ref().is_none_or(|passed| {
                // Every observation was validated before the merge begins.
                checked_at(previous).ok() > checked_at(passed).ok()
            })
        {
            success = Some(previous.clone());
        }
    }
    checks.retain(|check| !check.same_target(update));
    if let Some(success) = success
        && success.evidence_sha256 != latest.evidence_sha256
    {
        checks.push(success);
    }
    checks.push(latest);
    Ok(())
}
