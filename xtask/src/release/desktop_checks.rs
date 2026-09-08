use super::desktop::{
    DesktopRequirements, DesktopVerificationEntry, validate_desktop_verifications,
};
use super::verification::{VerificationRelease, current_release_version};
use nan_harness_core::DesktopCheck;
use std::path::Path;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

/// Merge only Desktop check data using the registry from the tested official release.
/// The caller authenticates the registry and binary identity before invoking this command.
pub(crate) fn merge_release_checks(
    base: &Path,
    updates: &Path,
    registry: &Path,
    version: &str,
    output: &Path,
) -> Result<(), String> {
    let version = semver::Version::parse(version)
        .map_err(|_| "tested release version is invalid".to_owned())?;
    let requirements = super::desktop::desktop_requirements_from_path(registry)?;
    let mut manifest = super::compatibility::read_verification_manifest(base)?;
    super::verification::validate_manifest_header(&manifest, 4)?;
    super::compatibility::validate_versioned_compatibility_feed(base)?;
    let mut count = 0;
    let mut paths = std::fs::read_dir(updates)
        .map_err(|error| error.to_string())?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    paths.sort();
    for path in paths {
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        super::validation::require_regular_file(&path)?;
        let contents = std::fs::read(&path).map_err(|error| error.to_string())?;
        let update: VerificationRelease =
            serde_json::from_slice(&contents).map_err(|error| error.to_string())?;
        if update.nan_harness_version != version
            || !update.verifications.is_empty()
            || !update.desktop_verifications.is_empty()
            || update.desktop_checks.is_empty()
        {
            return Err(
                "Desktop updates must name the authenticated release and contain only exact checks"
                    .to_owned(),
            );
        }
        validate_checks_for_release(&update, Some(&requirements), &version)?;
        if !manifest
            .releases
            .iter()
            .any(|release| release.nan_harness_version == version)
        {
            manifest.releases.push(VerificationRelease {
                nan_harness_version: version.clone(),
                verifications: Vec::new(),
                desktop_verifications: Vec::new(),
                desktop_checks: Vec::new(),
            });
        }
        let release = manifest
            .releases
            .iter_mut()
            .find(|release| release.nan_harness_version == version)
            .expect("release inserted above");
        for check in update.desktop_checks {
            merge_check(&mut release.desktop_checks, check);
            count += 1;
        }
    }
    if count == 0 {
        return Err("Desktop updates contain no checks".to_owned());
    }
    super::compatibility::write_verification_manifest(output, &manifest)
}

pub(super) fn validate_checks(
    release: &VerificationRelease,
    requirements: Option<&DesktopRequirements>,
) -> Result<(), String> {
    validate_checks_for_release(release, requirements, &current_release_version())
}

pub(super) fn validate_checks_for_release(
    release: &VerificationRelease,
    requirements: Option<&DesktopRequirements>,
    registry_version: &semver::Version,
) -> Result<(), String> {
    for (index, check) in release.desktop_checks.iter().enumerate() {
        if !matches!(check.platform.as_str(), "macos" | "linux" | "windows")
            || !matches!(check.architecture.as_str(), "aarch64" | "x86_64")
        {
            return Err("Desktop check has an unknown platform or architecture".to_owned());
        }
        if check.deterministic_at.is_none() && check.live_verified_at.is_none() {
            return Err("Desktop check has no successful track".to_owned());
        }
        for timestamp in [&check.deterministic_at, &check.live_verified_at]
            .into_iter()
            .flatten()
        {
            OffsetDateTime::parse(timestamp, &Rfc3339)
                .map_err(|_| "Desktop check has an invalid timestamp".to_owned())?;
        }
        if release.desktop_checks[..index]
            .iter()
            .any(|other| check.same_target(other))
        {
            return Err("Desktop checks contain a duplicate exact target".to_owned());
        }
        let requirements =
            requirements.ok_or_else(|| "legacy feed cannot carry Desktop checks".to_owned())?;
        let entry = DesktopVerificationEntry {
            id: check.id.to_string(),
            platform: check.platform.clone(),
            evidence: "live-verified".to_owned(),
            last_compatible_app_version: Some(check.app_version.clone()),
            last_compatible_runtime_version: check.runtime_version.clone(),
            compatible_at: check
                .deterministic_at
                .as_ref()
                .or(check.live_verified_at.as_ref())
                .expect("track checked above")
                .clone(),
        };
        validate_desktop_verifications(
            &[entry],
            requirements,
            release.nan_harness_version == *registry_version,
            "exact Desktop checks",
        )?;
    }
    Ok(())
}

pub(super) fn merge_check(checks: &mut Vec<DesktopCheck>, update: DesktopCheck) {
    if let Some(existing) = checks.iter_mut().find(|check| check.same_target(&update)) {
        merge_timestamp(&mut existing.deterministic_at, update.deterministic_at);
        merge_timestamp(&mut existing.live_verified_at, update.live_verified_at);
    } else {
        checks.push(update);
    }
}

fn merge_timestamp(current: &mut Option<String>, update: Option<String>) {
    let Some(update) = update else {
        return;
    };
    let newer = current.as_ref().is_none_or(|current| {
        OffsetDateTime::parse(&update, &Rfc3339).ok()
            > OffsetDateTime::parse(current, &Rfc3339).ok()
    });
    if newer {
        *current = Some(update);
    }
}
