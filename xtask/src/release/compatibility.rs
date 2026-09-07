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
    let update_count = apply_update_directory(
        updates,
        schema_version,
        &requirements,
        desktop,
        &mut releases,
        &mut updated_releases,
    )?;
    if update_count == 0 {
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

/// Applies every update file to the accumulating releases, reporting how many were applied.
fn apply_update_directory(
    updates: &Path,
    schema_version: u8,
    requirements: &std::collections::BTreeMap<HarnessKind, HarnessRequirement>,
    desktop: Option<&DesktopRequirements>,
    releases: &mut Vec<VerificationRelease>,
    updated_releases: &mut std::collections::BTreeSet<semver::Version>,
) -> Result<usize, String> {
    let mut update_count = 0_usize;
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
                update_count += 1;
            }
            continue;
        }
        let release = if value.get("platform").is_some() {
            // The legacy asset is CLI-only by contract: the same update directory feeds both
            // assets, and Desktop evidence simply does not belong in the legacy one.
            let Some(_) = desktop else {
                continue;
            };
            let update: DesktopVerificationUpdate = serde_json::from_value(value)
                .map_err(|error| format!("could not parse '{}': {error}", path.display()))?;
            VerificationRelease {
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
        update_count += 1;
    }
    Ok(update_count)
}

/// Publishes the embedded Desktop evidence for every release this run updated, when the feed
/// does not describe it yet. Desktop updates from this run have already been merged on top;
/// releases this run did not touch keep the evidence they were published with.
fn seed_desktop_evidence(
    releases: &mut [VerificationRelease],
    updated: &std::collections::BTreeSet<semver::Version>,
    desktop: &DesktopRequirements,
) {
    for release in releases
        .iter_mut()
        .filter(|release| updated.contains(&release.nan_harness_version))
    {
        if release.desktop_verifications.is_empty() {
            release.desktop_verifications = bundled_desktop_verifications(desktop);
        }
    }
}

/// Desktop requirements apply to the unified feed only; the legacy feed carries no Desktop
/// evidence at all.
fn desktop_requirements_for(schema_version: u8) -> Result<Option<DesktopRequirements>, String> {
    if schema_version == UNIFIED_FEED_SCHEMA_VERSION {
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

fn read_verification_manifest(path: &Path) -> Result<VerificationManifest, String> {
    let contents =
        fs::read(path).map_err(|error| format!("could not read '{}': {error}", path.display()))?;
    serde_json::from_slice(&contents)
        .map_err(|error| format!("could not parse '{}': {error}", path.display()))
}

fn write_verification_manifest(path: &Path, manifest: &VerificationManifest) -> Result<(), String> {
    let mut payload = serde_json::to_vec_pretty(manifest)
        .map_err(|error| format!("could not serialize compatibility manifest: {error}"))?;
    payload.push(b'\n');
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
