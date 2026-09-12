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
                release.nan_harness_version == super::verification::current_release_version(),
                "hosted checks",
            )?;
        }
    }
    Ok(())
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
