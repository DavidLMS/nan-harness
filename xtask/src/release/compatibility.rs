use super::desktop::{
    DesktopRequirements, DesktopVerificationUpdate, bundled_desktop_verifications,
    desktop_requirements,
};
use super::validation::{repository_root, require_regular_file};
use super::verification::{
    HarnessRequirement, VerificationManifest, VerificationRelease, VerificationUpdate,
    apply_release_update, bundled_verification_release, current_release_version,
    validate_embedded_manifest, validate_manifest_header, validate_releases,
};
use nan_harness_core::{CompatibilityManifest, HarnessKind};
use std::ffi::OsStr;
use std::fs;
use std::io::Write as _;
use std::path::Path;
use tempfile::Builder as TempFileBuilder;

pub(super) const COMPATIBILITY_FILE_NAME: &str = "compatibility.json";
const COMPATIBILITY_SOURCE_PATH: &str = "crates/nan-harness-runtime/resources/compatibility.json";
/// Legacy CLI-only feed. Existing clients parse it with `deny_unknown_fields`.
const COMPATIBILITY_FEED_SCHEMA_VERSION: u8 = 2;
/// Unified feed carrying CLI and Desktop evidence for the same accepted results.
const UNIFIED_FEED_SCHEMA_VERSION: u8 = 3;
const VERSIONED_FEED_SCHEMA_VERSION: u8 = 4;
const HOSTED_FEED_SCHEMA_VERSION: u8 = 5;

pub(crate) fn generate_hosted_compatibility_feed(output: &Path) -> Result<(), String> {
    write_verification_manifest(
        output,
        &bundled_verification_manifest(HOSTED_FEED_SCHEMA_VERSION)?,
    )
}

pub(crate) fn merge_hosted_compatibility_feed(
    base: &Path,
    updates: &Path,
    output: &Path,
) -> Result<(), String> {
    merge_feed(base, updates, output, HOSTED_FEED_SCHEMA_VERSION)
}

pub(crate) fn validate_hosted_compatibility_feed(input: &Path) -> Result<(), String> {
    validate_feed(input, HOSTED_FEED_SCHEMA_VERSION)
}

pub(crate) fn generate_versioned_compatibility_feed(output: &Path) -> Result<(), String> {
    write_verification_manifest(
        output,
        &bundled_verification_manifest(VERSIONED_FEED_SCHEMA_VERSION)?,
    )
}

pub(crate) fn merge_versioned_compatibility_feed(
    base: &Path,
    updates: &Path,
    output: &Path,
) -> Result<(), String> {
    merge_feed(base, updates, output, VERSIONED_FEED_SCHEMA_VERSION)
}

pub(crate) fn validate_versioned_compatibility_feed(input: &Path) -> Result<(), String> {
    validate_feed(input, VERSIONED_FEED_SCHEMA_VERSION)
}

pub(crate) fn generate_compatibility_feed(output: &Path) -> Result<(), String> {
    let manifest = bundled_verification_manifest(COMPATIBILITY_FEED_SCHEMA_VERSION)?;
    write_verification_manifest(output, &manifest)
}

pub(crate) fn generate_unified_compatibility_feed(output: &Path) -> Result<(), String> {
    let manifest = bundled_verification_manifest(UNIFIED_FEED_SCHEMA_VERSION)?;
    write_verification_manifest(output, &manifest)
}

pub(crate) fn merge_compatibility_feed(
    base: &Path,
    updates: &Path,
    output: &Path,
) -> Result<(), String> {
    merge_feed(base, updates, output, COMPATIBILITY_FEED_SCHEMA_VERSION)
}

pub(crate) fn merge_unified_compatibility_feed(
    base: &Path,
    updates: &Path,
    output: &Path,
) -> Result<(), String> {
    merge_feed(base, updates, output, UNIFIED_FEED_SCHEMA_VERSION)
}

fn merge_feed(
    base: &Path,
    updates: &Path,
    output: &Path,
    schema_version: u8,
) -> Result<(), String> {
    if !updates.is_dir() {
        return Err(format!(
            "compatibility update directory '{}' does not exist",
            updates.display()
        ));
    }
    let desktop = desktop_requirements_for(schema_version)?;
    let desktop = desktop.as_ref();
    let source = bundled_compatibility_manifest()?;
    let requirements = source
        .harnesses
        .iter()
        .map(|entry| {
            (
                entry.id,
                HarnessRequirement {
                    minimum_version: entry.minimum_version.clone(),
                    compatible_version: entry.last_compatible_version.clone(),
                },
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let base_manifest = read_verification_manifest(base)?;
    validate_manifest_header(&base_manifest, schema_version)?;
    validate_releases(
        &base_manifest.releases,
        &requirements,
        desktop,
        "base compatibility feed",
    )?;
    let mut releases = base_manifest.releases;
    let mut updated_releases = std::collections::BTreeSet::new();
    let tally = apply_update_directory(
        updates,
        schema_version,
        &requirements,
        desktop,
        &mut releases,
        &mut updated_releases,
    )?;
    // A directory of Desktop-only results is a complete run for the unified feed and a no-op for
    // the legacy one, which is then republished unchanged.
    if tally.applied == 0 && tally.skipped_desktop == 0 {
        return Err("compatibility updates contain no verification entries".to_owned());
    }
    if let Some(desktop) = desktop {
        seed_desktop_evidence(&mut releases, &updated_releases, desktop);
    }

    let manifest = VerificationManifest {
        schema_version,
        releases,
    };
    write_verification_manifest(output, &manifest)
}

pub(crate) fn validate_compatibility_feed(input: &Path) -> Result<(), String> {
    validate_feed(input, COMPATIBILITY_FEED_SCHEMA_VERSION)
}

pub(crate) fn validate_unified_compatibility_feed(input: &Path) -> Result<(), String> {
    validate_feed(input, UNIFIED_FEED_SCHEMA_VERSION)
}

fn validate_feed(input: &Path, schema_version: u8) -> Result<(), String> {
    let desktop = desktop_requirements_for(schema_version)?;
    let source = bundled_compatibility_manifest()?;
    let requirements = source
        .harnesses
        .iter()
        .map(|entry| {
            (
                entry.id,
                HarnessRequirement {
                    minimum_version: entry.minimum_version.clone(),
                    compatible_version: entry.last_compatible_version.clone(),
                },
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let manifest = read_verification_manifest(input)?;
    validate_manifest_header(&manifest, schema_version)?;
    if manifest.releases.is_empty() {
        return Err("compatibility feed contains no release records".to_owned());
    }
    validate_releases(
        &manifest.releases,
        &requirements,
        desktop.as_ref(),
        "compatibility feed",
    )
}

/// Counts what one update directory contributed to a feed.
#[derive(Default)]
struct UpdateTally {
    applied: usize,
    /// Desktop updates the legacy feed recognized but does not carry.
    skipped_desktop: usize,
}

/// Applies every update file to the accumulating releases.
fn apply_update_directory(
    updates: &Path,
    schema_version: u8,
    requirements: &std::collections::BTreeMap<HarnessKind, HarnessRequirement>,
    desktop: Option<&DesktopRequirements>,
    releases: &mut Vec<VerificationRelease>,
    updated_releases: &mut std::collections::BTreeSet<semver::Version>,
) -> Result<UpdateTally, String> {
    let mut tally = UpdateTally::default();
    for entry in fs::read_dir(updates).map_err(|error| {
        format!(
            "could not inspect compatibility updates '{}': {error}",
            updates.display()
        )
    })? {
        let entry =
            entry.map_err(|error| format!("could not inspect compatibility update: {error}"))?;
        let path = entry.path();
        if path.extension().and_then(OsStr::to_str) != Some("json") {
            continue;
        }
        require_regular_file(&path)?;
        let contents = fs::read(&path)
            .map_err(|error| format!("could not read '{}': {error}", path.display()))?;
        let value: serde_json::Value = serde_json::from_slice(&contents)
            .map_err(|error| format!("could not parse '{}': {error}", path.display()))?;
        if value.get("releases").is_some() {
            let manifest: VerificationManifest = serde_json::from_value(value)
                .map_err(|error| format!("could not parse '{}': {error}", path.display()))?;
            validate_manifest_header(&manifest, schema_version)?;
            validate_releases(
                &manifest.releases,
                requirements,
                desktop,
                "compatibility update",
            )?;
            for release in manifest.releases {
                updated_releases.insert(release.nan_harness_version.clone());
                apply_release_update(
                    releases,
                    release,
                    requirements,
                    desktop,
                    &format!("compatibility update '{}':", path.display()),
                )?;
                tally.applied += 1;
            }
            continue;
        }
        let release = if value.get("desktopChecks").is_some() || value.get("hostedChecks").is_some()
        {
            let release: VerificationRelease = serde_json::from_value(value)
                .map_err(|error| format!("could not parse '{}': {error}", path.display()))?;
            if schema_version < VERSIONED_FEED_SCHEMA_VERSION {
                return Err("exact-version checks cannot be projected into legacy feeds".to_owned());
            }
            if schema_version != HOSTED_FEED_SCHEMA_VERSION && !release.hosted_checks.is_empty() {
                return Err("hosted checks require schema v5".to_owned());
            }
            release
        } else if value.get("platform").is_some() {
            // The same update directory feeds both assets. The shape is checked either way; the
            // legacy asset is CLI-only by contract and simply does not carry the record.
            let update: DesktopVerificationUpdate = serde_json::from_value(value)
                .map_err(|error| format!("could not parse '{}': {error}", path.display()))?;
            if desktop.is_none() {
                tally.skipped_desktop += 1;
                continue;
            }
            VerificationRelease {
                hosted_checks: Vec::new(),
                desktop_checks: Vec::new(),
                nan_harness_version: update
                    .nan_harness_version
                    .clone()
                    .unwrap_or_else(current_release_version),
                verifications: Vec::new(),
                desktop_verifications: vec![update.into_entry()],
            }
        } else {
            let update: VerificationUpdate = serde_json::from_value(value)
                .map_err(|error| format!("could not parse '{}': {error}", path.display()))?;
            VerificationRelease {
                hosted_checks: Vec::new(),
                desktop_checks: Vec::new(),
                nan_harness_version: update
                    .nan_harness_version
                    .clone()
                    .unwrap_or_else(current_release_version),
                verifications: vec![update.into_entry()],
                desktop_verifications: Vec::new(),
            }
        };
        updated_releases.insert(release.nan_harness_version.clone());
        apply_release_update(
            releases,
            release,
            requirements,
            desktop,
            &format!("canary update '{}':", path.display()),
        )?;
        tally.applied += 1;
    }
    Ok(tally)
}

/// Fills the surfaces this checkout's registry describes into the record of the release it
/// builds, and only that release: evidence from this source cannot certify a different binary.
/// Explicit updates already merged into the record are never overwritten.
fn seed_desktop_evidence(
    releases: &mut [VerificationRelease],
    updated: &std::collections::BTreeSet<semver::Version>,
    desktop: &DesktopRequirements,
) {
    let current = current_release_version();
    if !updated.contains(&current) {
        return;
    }
    let Some(release) = releases
        .iter_mut()
        .find(|release| release.nan_harness_version == current)
    else {
        return;
    };
    for entry in bundled_desktop_verifications(desktop) {
        if !release
            .desktop_verifications
            .iter()
            .any(|published| published.id == entry.id && published.platform == entry.platform)
        {
            release.desktop_verifications.push(entry);
        }
    }
    release
        .desktop_verifications
        .sort_by(|left, right| (&left.id, &left.platform).cmp(&(&right.id, &right.platform)));
}

/// Desktop requirements apply to the unified feed only; the legacy feed carries no Desktop
/// evidence at all.
fn desktop_requirements_for(schema_version: u8) -> Result<Option<DesktopRequirements>, String> {
    if matches!(
        schema_version,
        UNIFIED_FEED_SCHEMA_VERSION | VERSIONED_FEED_SCHEMA_VERSION | HOSTED_FEED_SCHEMA_VERSION
    ) {
        desktop_requirements().map(Some)
    } else {
        Ok(None)
    }
}

pub(super) fn bundled_compatibility_manifest() -> Result<CompatibilityManifest, String> {
    let source_path = repository_root().join(COMPATIBILITY_SOURCE_PATH);
    let source = fs::read(&source_path)
        .map_err(|error| format!("could not read '{}': {error}", source_path.display()))?;
    let manifest: CompatibilityManifest = serde_json::from_slice(&source).map_err(|error| {
        format!(
            "could not parse compatibility manifest '{}': {error}",
            source_path.display()
        )
    })?;
    validate_embedded_manifest(&manifest)?;
    Ok(manifest)
}

/// Builds both assets from the same accepted evidence: the unified feed adds Desktop records
/// from the embedded registry, while the legacy feed stays CLI-only.
fn bundled_verification_manifest(schema_version: u8) -> Result<VerificationManifest, String> {
    let source = bundled_compatibility_manifest()?;
    let mut release = bundled_verification_release(&source);
    if let Some(desktop) = desktop_requirements_for(schema_version)? {
        release.desktop_verifications = bundled_desktop_verifications(&desktop);
    }
    Ok(VerificationManifest {
        schema_version,
        releases: vec![release],
    })
}

pub(super) fn read_verification_manifest(path: &Path) -> Result<VerificationManifest, String> {
    let contents =
        fs::read(path).map_err(|error| format!("could not read '{}': {error}", path.display()))?;
    serde_json::from_slice(&contents)
        .map_err(|error| format!("could not parse '{}': {error}", path.display()))
}

pub(super) fn write_verification_manifest(
    path: &Path,
    manifest: &VerificationManifest,
) -> Result<(), String> {
    let mut payload = serde_json::to_vec_pretty(manifest)
        .map_err(|error| format!("could not serialize compatibility manifest: {error}"))?;
    payload.push(b'\n');
    if payload.len() > 1024 * 1024 {
        return Err("compatibility feed exceeds the clients' 1 MiB limit".to_owned());
    }
    let parent = path.parent().ok_or_else(|| {
        format!(
            "compatibility manifest path '{}' has no parent",
            path.display()
        )
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create compatibility manifest directory: {error}"))?;
    let mut temporary = TempFileBuilder::new()
        .prefix(".nan-harness-compatibility-")
        .tempfile_in(parent)
        .map_err(|error| {
            format!("could not create compatibility manifest temporary file: {error}")
        })?;
    temporary
        .write_all(&payload)
        .and_then(|()| temporary.flush())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|error| format!("could not write compatibility manifest: {error}"))?;
    temporary
        .persist(path)
        .map_err(|error| format!("could not replace compatibility manifest: {}", error.error))?;
    Ok(())
}
