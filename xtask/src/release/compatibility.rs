use super::validation::{repository_root, require_regular_file};
use super::verification::{
    HarnessRequirement, VerificationManifest, VerificationRelease, VerificationUpdate,
    apply_release_update, bundled_verification_release, current_release_version,
    validate_embedded_manifest, validate_manifest_header, validate_releases,
};
use nan_harness_core::CompatibilityManifest;
use std::ffi::OsStr;
use std::fs;
use std::io::Write as _;
use std::path::Path;
use tempfile::Builder as TempFileBuilder;

pub(super) const COMPATIBILITY_FILE_NAME: &str = "compatibility.json";
const COMPATIBILITY_SOURCE_PATH: &str = "crates/nan-harness-runtime/resources/compatibility.json";
const COMPATIBILITY_FEED_SCHEMA_VERSION: u8 = 2;

pub(crate) fn generate_compatibility_feed(output: &Path) -> Result<(), String> {
    let manifest = bundled_verification_manifest()?;
    write_verification_manifest(output, &manifest)
}

pub(crate) fn merge_compatibility_feed(
    base: &Path,
    updates: &Path,
    output: &Path,
) -> Result<(), String> {
    if !updates.is_dir() {
        return Err(format!(
            "compatibility update directory '{}' does not exist",
            updates.display()
        ));
    }
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
    validate_manifest_header(&base_manifest, COMPATIBILITY_FEED_SCHEMA_VERSION)?;
    validate_releases(
        &base_manifest.releases,
        &requirements,
        "base compatibility feed",
    )?;
    let mut releases = base_manifest.releases;

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
            validate_manifest_header(&manifest, COMPATIBILITY_FEED_SCHEMA_VERSION)?;
            validate_releases(&manifest.releases, &requirements, "compatibility update")?;
            for release in manifest.releases {
                apply_release_update(
                    &mut releases,
                    release,
                    &requirements,
                    &format!("compatibility update '{}':", path.display()),
                )?;
                update_count += 1;
            }
            continue;
        }
        let update: VerificationUpdate = serde_json::from_value(value)
            .map_err(|error| format!("could not parse '{}': {error}", path.display()))?;
        let release = VerificationRelease {
            nan_harness_version: update
                .nan_harness_version
                .clone()
                .unwrap_or_else(current_release_version),
            verifications: vec![update.into_entry()],
        };
        apply_release_update(
            &mut releases,
            release,
            &requirements,
            &format!("canary update '{}':", path.display()),
        )?;
        update_count += 1;
    }
    if update_count == 0 {
        return Err("compatibility updates contain no verification entries".to_owned());
    }

    let manifest = VerificationManifest {
        schema_version: COMPATIBILITY_FEED_SCHEMA_VERSION,
        releases,
    };
    write_verification_manifest(output, &manifest)
}

pub(crate) fn validate_compatibility_feed(input: &Path) -> Result<(), String> {
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
    validate_manifest_header(&manifest, COMPATIBILITY_FEED_SCHEMA_VERSION)?;
    if manifest.releases.is_empty() {
        return Err("compatibility feed contains no release records".to_owned());
    }
    validate_releases(&manifest.releases, &requirements, "compatibility feed")
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

fn bundled_verification_manifest() -> Result<VerificationManifest, String> {
    let source = bundled_compatibility_manifest()?;
    let release = bundled_verification_release(&source);
    Ok(VerificationManifest {
        schema_version: COMPATIBILITY_FEED_SCHEMA_VERSION,
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
