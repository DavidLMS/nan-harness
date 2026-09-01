use crate::changelog;
use nan_harness_core::{CompatibilityManifest, HarnessKind};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::Path;
use tempfile::Builder as TempFileBuilder;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const MAX_ARTIFACT_SIZE: u64 = 128 * 1024 * 1024;
const INSTALLER_FILES: [&str; 2] = ["install.sh", "install.ps1"];
const CITATION_FILE_NAME: &str = "CITATION.cff";
const COMPATIBILITY_FILE_NAME: &str = "compatibility.json";
const COMPATIBILITY_SOURCE_PATH: &str = "crates/nan-harness-runtime/resources/compatibility.json";
const COMPATIBILITY_FEED_SCHEMA_VERSION: u8 = 2;
const DISTRIBUTION_FILES: [&str; 3] = [CITATION_FILE_NAME, "LICENSE", "NOTICE.md"];
const CARGO_MANIFEST_FILES: [&str; 12] = [
    "Cargo.toml",
    "crates/nan-harness-adapters/Cargo.toml",
    "crates/nan-harness-bridge/Cargo.toml",
    "crates/nan-harness-canary/Cargo.toml",
    "crates/nan-harness-cli/Cargo.toml",
    "crates/nan-harness-core/Cargo.toml",
    "crates/nan-harness-diagnostics/Cargo.toml",
    "crates/nan-harness-private-fs/Cargo.toml",
    "crates/nan-harness-runtime/Cargo.toml",
    "crates/nan-harness-telemetry/Cargo.toml",
    "crates/nan-harness-test-support/Cargo.toml",
    "xtask/Cargo.toml",
];
const LOCAL_PACKAGE_NAMES: [&str; 11] = [
    "nan-harness-adapters",
    "nan-harness-bridge",
    "nan-harness-canary",
    "nan-harness-cli",
    "nan-harness-core",
    "nan-harness-diagnostics",
    "nan-harness-private-fs",
    "nan-harness-runtime",
    "nan-harness-telemetry",
    "nan-harness-test-support",
    "xtask",
];
const RELEASE_TARGETS: [&str; 5] = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-musl",
    "x86_64-unknown-linux-musl",
    "x86_64-pc-windows-msvc",
];
const AUXILIARY_ARTIFACTS: [&str; 2] = [
    "nan-harness-canary-aarch64-unknown-linux-musl",
    "nan-harness-canary-aarch64-apple-darwin",
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseManifest {
    schema_version: u8,
    version: String,
    notes_url: String,
    artifacts: Vec<ReleaseArtifact>,
}

#[derive(Serialize)]
struct ReleaseArtifact {
    target: String,
    url: String,
    sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VerificationManifest {
    schema_version: u8,
    releases: Vec<VerificationRelease>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VerificationEntry {
    id: String,
    #[serde(default)]
    last_compatible_version: Option<Version>,
    #[serde(default)]
    compatible_at: Option<String>,
    #[serde(default)]
    last_live_verified_version: Option<Version>,
    #[serde(default)]
    live_verified_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VerificationRelease {
    nan_harness_version: Version,
    #[serde(alias = "harnesses")]
    verifications: Vec<VerificationEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VerificationUpdate {
    #[serde(default)]
    nan_harness_version: Option<Version>,
    id: String,
    #[serde(default)]
    last_compatible_version: Option<Version>,
    #[serde(default)]
    compatible_at: Option<String>,
    #[serde(default)]
    last_live_verified_version: Option<Version>,
    #[serde(default)]
    live_verified_at: Option<String>,
}

#[derive(Clone)]
struct HarnessRequirement {
    minimum_version: Version,
    compatible_version: Version,
}

impl VerificationUpdate {
    fn into_entry(self) -> VerificationEntry {
        VerificationEntry {
            id: self.id,
            last_compatible_version: self.last_compatible_version,
            compatible_at: self.compatible_at,
            last_live_verified_version: self.last_live_verified_version,
            live_verified_at: self.live_verified_at,
        }
    }
}

pub(crate) fn set_version(raw_version: &str) -> Result<(), String> {
    let next_version = raw_version.strip_prefix('v').unwrap_or(raw_version);
    let next_version = Version::parse(next_version)
        .map_err(|error| format!("release version '{raw_version}' is invalid: {error}"))?;
    let current_version = Version::parse(env!("CARGO_PKG_VERSION"))
        .expect("workspace package version should be valid semver");
    if next_version <= current_version {
        return Err(format!(
            "release version {next_version} must be newer than workspace version {current_version}"
        ));
    }
    let current_version = current_version.to_string();
    let next_version = next_version.to_string();
    let root = repository_root();
    let changelog_path = root.join(changelog::FILE_NAME);
    let release_date = OffsetDateTime::now_utc().date().to_string();
    let updated_changelog = changelog::prepare(
        &changelog_path,
        &current_version,
        &next_version,
        &release_date,
    )?;

    for manifest in CARGO_MANIFEST_FILES {
        replace_manifest_version(&root.join(manifest), &current_version, &next_version)?;
    }
    replace_lockfile_version(&root.join("Cargo.lock"), &current_version, &next_version)?;
    replace_citation_version(&root.join(CITATION_FILE_NAME), &next_version)?;
    changelog::write(&changelog_path, updated_changelog)
}

pub(crate) fn validate_changelog() -> Result<(), String> {
    changelog::validate(
        &repository_root().join(changelog::FILE_NAME),
        env!("CARGO_PKG_VERSION"),
    )
}

pub(crate) fn write_changelog_notes(version: &str, output: &Path) -> Result<(), String> {
    let notes = changelog::release_notes(&repository_root().join(changelog::FILE_NAME), version)?;
    fs::write(output, notes).map_err(|error| {
        format!(
            "could not write release notes '{}': {error}",
            output.display()
        )
    })
}

pub(crate) fn validate_tag(tag: &str) -> Result<(), String> {
    let expected = format!("v{}", env!("CARGO_PKG_VERSION"));
    if tag != expected {
        return Err(format!(
            "release tag '{tag}' does not match workspace version {}; expected '{expected}'",
            env!("CARGO_PKG_VERSION")
        ));
    }

    validate_citation_version()?;
    validate_changelog()
}

pub(crate) fn generate_metadata(
    tag: &str,
    repository: &str,
    directory: &Path,
) -> Result<(), String> {
    validate_tag(tag)?;
    validate_repository(repository)?;
    if !directory.is_dir() {
        return Err(format!(
            "release directory '{}' does not exist",
            directory.display()
        ));
    }

    for installer in INSTALLER_FILES {
        require_regular_file(&directory.join(installer))?;
    }

    for file_name in DISTRIBUTION_FILES {
        copy_distribution_file(file_name, directory)?;
    }

    let mut artifacts = Vec::with_capacity(RELEASE_TARGETS.len());
    for target in RELEASE_TARGETS {
        let file_name = artifact_file_name(target);
        let path = directory.join(&file_name);
        let sha256 = checksum(&path)?;
        fs::write(
            directory.join(format!("{file_name}.sha256")),
            format!("{sha256}  {file_name}\n"),
        )
        .map_err(|error| format!("could not write checksum for '{file_name}': {error}"))?;
        artifacts.push(ReleaseArtifact {
            target: target.to_owned(),
            url: format!("https://github.com/{repository}/releases/download/{tag}/{file_name}"),
            sha256,
        });
    }
    for file_name in AUXILIARY_ARTIFACTS {
        let path = directory.join(file_name);
        let sha256 = checksum(&path)?;
        fs::write(
            directory.join(format!("{file_name}.sha256")),
            format!("{sha256}  {file_name}\n"),
        )
        .map_err(|error| format!("could not write checksum for '{file_name}': {error}"))?;
    }

    let version = env!("CARGO_PKG_VERSION");
    fs::write(
        directory.join("release-version.txt"),
        format!("{version}\n"),
    )
    .map_err(|error| format!("could not write release version: {error}"))?;
    let manifest = ReleaseManifest {
        schema_version: 1,
        version: version.to_owned(),
        notes_url: format!("https://github.com/{repository}/releases/tag/{tag}"),
        artifacts,
    };
    let mut manifest_json = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("could not serialize update manifest: {error}"))?;
    manifest_json.push(b'\n');
    fs::write(directory.join("update-manifest.json"), manifest_json)
        .map_err(|error| format!("could not write update manifest: {error}"))?;
    generate_compatibility_feed(&directory.join(COMPATIBILITY_FILE_NAME))?;

    let expected_files = expected_release_files();
    reject_unexpected_files(directory, &expected_files)?;
    write_combined_checksums(directory, &expected_files)
}

fn validate_repository(repository: &str) -> Result<(), String> {
    let Some((owner, name)) = repository.split_once('/') else {
        return Err("repository must use the 'owner/name' format".to_owned());
    };
    if owner.is_empty()
        || name.is_empty()
        || name.contains('/')
        || !owner.chars().all(repository_character)
        || !name.chars().all(repository_character)
    {
        return Err(format!(
            "repository '{repository}' is not a valid GitHub name"
        ));
    }
    Ok(())
}

fn repository_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
}

fn artifact_file_name(target: &str) -> String {
    let extension = if target.ends_with("windows-msvc") {
        ".exe"
    } else {
        ""
    };
    format!("nan-harness-{target}{extension}")
}

fn checksum(path: &Path) -> Result<String, String> {
    require_regular_file(path)?;
    let metadata = path
        .metadata()
        .map_err(|error| format!("could not inspect '{}': {error}", path.display()))?;
    if metadata.len() > MAX_ARTIFACT_SIZE {
        return Err(format!(
            "release artifact '{}' exceeds the 128 MiB safety limit",
            path.display()
        ));
    }
    let contents =
        fs::read(path).map_err(|error| format!("could not read '{}': {error}", path.display()))?;
    Ok(hex_digest(Sha256::digest(contents)))
}

fn require_regular_file(path: &Path) -> Result<(), String> {
    let metadata = path.symlink_metadata().map_err(|error| {
        format!(
            "required release file '{}' is unavailable: {error}",
            path.display()
        )
    })?;
    if metadata.file_type().is_file() {
        Ok(())
    } else {
        Err(format!(
            "required release file '{}' is not a regular file",
            path.display()
        ))
    }
}

fn expected_release_files() -> BTreeSet<String> {
    let mut files = BTreeSet::from([
        CITATION_FILE_NAME.to_owned(),
        COMPATIBILITY_FILE_NAME.to_owned(),
        "LICENSE".to_owned(),
        "NOTICE.md".to_owned(),
        "install.ps1".to_owned(),
        "install.sh".to_owned(),
        "release-version.txt".to_owned(),
        "update-manifest.json".to_owned(),
    ]);
    for target in RELEASE_TARGETS {
        let file_name = artifact_file_name(target);
        files.insert(format!("{file_name}.sha256"));
        files.insert(file_name);
    }
    for file_name in AUXILIARY_ARTIFACTS {
        files.insert(file_name.to_owned());
        files.insert(format!("{file_name}.sha256"));
    }
    files
}

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
    validate_manifest_header(&base_manifest)?;
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
            validate_manifest_header(&manifest)?;
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
    validate_manifest_header(&manifest)?;
    if manifest.releases.is_empty() {
        return Err("compatibility feed contains no release records".to_owned());
    }
    validate_releases(&manifest.releases, &requirements, "compatibility feed")
}

fn bundled_compatibility_manifest() -> Result<CompatibilityManifest, String> {
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

fn validate_embedded_manifest(manifest: &CompatibilityManifest) -> Result<(), String> {
    if manifest.schema_version != CompatibilityManifest::SCHEMA_VERSION {
        return Err(format!(
            "embedded compatibility schema {} is not supported",
            manifest.schema_version
        ));
    }
    parse_timestamp(&manifest.tested_at, "testedAt")?;
    let mut ids = BTreeSet::new();
    for entry in &manifest.harnesses {
        if !ids.insert(entry.id) {
            return Err(format!(
                "embedded compatibility contains duplicate {}",
                entry.id
            ));
        }
        if entry.last_compatible_version < entry.minimum_version {
            return Err(format!(
                "embedded compatibility reports {} version {}, below minimum {}",
                entry.id, entry.last_compatible_version, entry.minimum_version
            ));
        }
        parse_timestamp(&entry.compatible_at, "compatibleAt")?;
        match (&entry.last_live_verified_version, &entry.live_verified_at) {
            (None, None) => {}
            (Some(version), Some(timestamp)) => {
                if version < &entry.minimum_version {
                    return Err(format!(
                        "embedded compatibility reports {} live version {}, below minimum {}",
                        entry.id, version, entry.minimum_version
                    ));
                }
                if version > &entry.last_compatible_version {
                    return Err(format!(
                        "embedded compatibility reports {} live version {} newer than compatible version {}",
                        entry.id, version, entry.last_compatible_version
                    ));
                }
                parse_timestamp(timestamp, "liveVerifiedAt")?;
            }
            _ => {
                return Err(format!(
                    "embedded compatibility entry {} has an incomplete live evidence pair",
                    entry.id
                ));
            }
        }
    }
    Ok(())
}

fn bundled_verification_manifest() -> Result<VerificationManifest, String> {
    let source = bundled_compatibility_manifest()?;
    let release = bundled_verification_release(&source);
    Ok(VerificationManifest {
        schema_version: COMPATIBILITY_FEED_SCHEMA_VERSION,
        releases: vec![release],
    })
}

fn bundled_verification_release(source: &CompatibilityManifest) -> VerificationRelease {
    VerificationRelease {
        nan_harness_version: current_release_version(),
        verifications: source
            .harnesses
            .iter()
            .map(|entry| VerificationEntry {
                id: entry.id.to_string(),
                last_compatible_version: Some(entry.last_compatible_version.clone()),
                compatible_at: Some(entry.compatible_at.clone()),
                last_live_verified_version: entry.last_live_verified_version.clone(),
                live_verified_at: entry.live_verified_at.clone(),
            })
            .collect(),
    }
}

fn read_verification_manifest(path: &Path) -> Result<VerificationManifest, String> {
    let contents =
        fs::read(path).map_err(|error| format!("could not read '{}': {error}", path.display()))?;
    serde_json::from_slice(&contents)
        .map_err(|error| format!("could not parse '{}': {error}", path.display()))
}

fn validate_manifest_header(manifest: &VerificationManifest) -> Result<(), String> {
    if manifest.schema_version != COMPATIBILITY_FEED_SCHEMA_VERSION {
        return Err(format!(
            "compatibility feed schema {} is not supported",
            manifest.schema_version
        ));
    }
    Ok(())
}

fn validate_releases(
    releases: &[VerificationRelease],
    requirements: &std::collections::BTreeMap<HarnessKind, HarnessRequirement>,
    source: &str,
) -> Result<(), String> {
    let mut release_versions = BTreeSet::new();
    for release in releases {
        if !release_versions.insert(release.nan_harness_version.clone()) {
            return Err(format!(
                "{source} contains duplicate release {}",
                release.nan_harness_version
            ));
        }
        let mut ids = BTreeSet::new();
        for entry in &release.verifications {
            let id = validate_verification_entry(entry, requirements, None, source)?;
            if let Some(id) = id
                && !ids.insert(id)
            {
                return Err(format!(
                    "{source} contains duplicate entry for {} in release {}",
                    id, release.nan_harness_version
                ));
            }
        }
    }
    Ok(())
}

fn apply_release_update(
    releases: &mut Vec<VerificationRelease>,
    update: VerificationRelease,
    requirements: &std::collections::BTreeMap<HarnessKind, HarnessRequirement>,
    source: &str,
) -> Result<(), String> {
    let release = if let Some(release) = releases
        .iter_mut()
        .find(|release| release.nan_harness_version == update.nan_harness_version)
    {
        release
    } else {
        releases.push(VerificationRelease {
            nan_harness_version: update.nan_harness_version.clone(),
            verifications: Vec::new(),
        });
        releases.last_mut().expect("the release was just appended")
    };
    let mut ids = BTreeSet::new();
    for entry in &update.verifications {
        let existing_compatible = entry.id.parse::<HarnessKind>().ok().and_then(|id| {
            release
                .verifications
                .iter()
                .find(|existing| existing.id.parse::<HarnessKind>().ok() == Some(id))
                .and_then(|existing| existing.last_compatible_version.as_ref())
        });
        let id = validate_verification_entry(entry, requirements, existing_compatible, source)?;
        if let Some(id) = id
            && !ids.insert(id)
        {
            return Err(format!("{source} contains duplicate entry for {id}"));
        }
    }
    for entry in update.verifications {
        let Ok(id) = entry.id.parse::<HarnessKind>() else {
            continue;
        };
        let requirement = requirements
            .get(&id)
            .expect("all known harnesses have embedded requirements");
        let existing = release
            .verifications
            .iter()
            .find(|existing| existing.id.parse::<HarnessKind>().ok() == Some(id));
        if let Some(live_version) = &entry.last_live_verified_version {
            let compatible_version = entry
                .last_compatible_version
                .as_ref()
                .or_else(|| existing.and_then(|entry| entry.last_compatible_version.as_ref()))
                .unwrap_or(&requirement.compatible_version);
            if live_version > compatible_version {
                return Err(format!(
                    "{source} reports {id} live version {live_version} newer than compatible version {compatible_version}"
                ));
            }
        }
        let Some(existing) = release
            .verifications
            .iter_mut()
            .find(|existing| existing.id.parse::<HarnessKind>().ok() == Some(id))
        else {
            release.verifications.push(entry);
            continue;
        };
        merge_verification_entry(existing, &entry, source)?;
    }
    Ok(())
}

fn validate_verification_entry(
    entry: &VerificationEntry,
    requirements: &std::collections::BTreeMap<HarnessKind, HarnessRequirement>,
    compatible_fallback: Option<&Version>,
    source: &str,
) -> Result<Option<HarnessKind>, String> {
    let compatible_at = validate_evidence_pair(
        entry,
        "compatible",
        entry.last_compatible_version.as_ref(),
        entry.compatible_at.as_ref(),
        source,
    )?;
    let live_at = validate_evidence_pair(
        entry,
        "live",
        entry.last_live_verified_version.as_ref(),
        entry.live_verified_at.as_ref(),
        source,
    )?;
    if compatible_at.is_none() && live_at.is_none() {
        return Err(format!("{source} entry {} contains no evidence", entry.id));
    }
    let Ok(id) = entry.id.parse::<HarnessKind>() else {
        return Ok(None);
    };
    let Some(requirement) = requirements.get(&id) else {
        return Ok(None);
    };
    if let Some(version) = &entry.last_compatible_version
        && version < &requirement.minimum_version
    {
        return Err(format!(
            "{source} reports {id} version {version}, below minimum {}",
            requirement.minimum_version
        ));
    }
    if let Some(version) = &entry.last_live_verified_version
        && version < &requirement.minimum_version
    {
        return Err(format!(
            "{source} reports {id} live version {version}, below minimum {}",
            requirement.minimum_version
        ));
    }
    if let Some(live_version) = &entry.last_live_verified_version {
        let compatible_version = entry
            .last_compatible_version
            .as_ref()
            .or(compatible_fallback)
            .unwrap_or(&requirement.compatible_version);
        if live_version > compatible_version {
            return Err(format!(
                "{source} reports {id} live version {live_version} newer than compatible version {compatible_version}"
            ));
        }
    }
    Ok(Some(id))
}

fn validate_evidence_pair(
    entry: &VerificationEntry,
    track: &'static str,
    version: Option<&Version>,
    timestamp: Option<&String>,
    source: &str,
) -> Result<Option<OffsetDateTime>, String> {
    match (version, timestamp) {
        (None, None) => Ok(None),
        (Some(_), Some(timestamp)) => {
            parse_timestamp(timestamp, &format!("{source} {} {track}", entry.id)).map(Some)
        }
        _ => Err(format!(
            "{source} entry {} has an incomplete {track} evidence pair",
            entry.id
        )),
    }
}

fn merge_verification_entry(
    current: &mut VerificationEntry,
    update: &VerificationEntry,
    source: &str,
) -> Result<(), String> {
    merge_evidence_pair(
        &mut current.last_compatible_version,
        &mut current.compatible_at,
        update.last_compatible_version.as_ref(),
        update.compatible_at.as_ref(),
        &current.id,
        "compatible",
        source,
    )?;
    merge_evidence_pair(
        &mut current.last_live_verified_version,
        &mut current.live_verified_at,
        update.last_live_verified_version.as_ref(),
        update.live_verified_at.as_ref(),
        &current.id,
        "live",
        source,
    )
}

fn merge_evidence_pair(
    current_version: &mut Option<Version>,
    current_at: &mut Option<String>,
    update_version: Option<&Version>,
    update_at: Option<&String>,
    id: &str,
    track: &'static str,
    source: &str,
) -> Result<(), String> {
    let Some((update_version, update_at, update_instant)) =
        validate_update_evidence(update_version, update_at, id, track, source)?
    else {
        return Ok(());
    };
    let Some((current_version_value, current_at_value)) = current_evidence_pair(
        current_version.as_ref(),
        current_at.as_ref(),
        id,
        track,
        source,
    )?
    else {
        *current_version = Some(update_version.clone());
        *current_at = Some(update_at.clone());
        return Ok(());
    };
    merge_existing_evidence(
        current_version,
        current_at,
        &current_version_value,
        &current_at_value,
        update_version,
        update_at,
        update_instant,
        id,
        track,
        source,
    )
}

fn validate_update_evidence<'a>(
    version: Option<&'a Version>,
    timestamp: Option<&'a String>,
    id: &str,
    track: &'static str,
    source: &str,
) -> Result<Option<(&'a Version, &'a String, OffsetDateTime)>, String> {
    let Some(version) = version else {
        return Ok(None);
    };
    let Some(timestamp) = timestamp else {
        return Err(format!(
            "{source} entry {id} has an incomplete {track} evidence pair"
        ));
    };
    let instant = parse_timestamp(timestamp, &format!("{source} {id} {track}"))?;
    Ok(Some((version, timestamp, instant)))
}

fn current_evidence_pair(
    version: Option<&Version>,
    timestamp: Option<&String>,
    id: &str,
    track: &'static str,
    source: &str,
) -> Result<Option<(Version, String)>, String> {
    match (version, timestamp) {
        (None, None) => Ok(None),
        (Some(version), Some(timestamp)) => Ok(Some((version.clone(), timestamp.clone()))),
        _ => Err(format!(
            "{source} entry {id} has an incomplete {track} evidence pair"
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn merge_existing_evidence(
    current_version: &mut Option<Version>,
    current_at: &mut Option<String>,
    current_version_value: &Version,
    current_at_value: &str,
    update_version: &Version,
    update_at: &str,
    update_instant: OffsetDateTime,
    id: &str,
    track: &'static str,
    source: &str,
) -> Result<(), String> {
    match update_version.cmp(current_version_value) {
        std::cmp::Ordering::Greater => {
            *current_version = Some(update_version.clone());
            *current_at = Some(newer_timestamp(
                current_at_value,
                update_at,
                update_instant,
                id,
                track,
                source,
            )?);
        }
        std::cmp::Ordering::Equal => {
            if timestamp_is_newer(current_at_value, update_instant, id, track, source)? {
                *current_at = Some(update_at.to_owned());
            }
        }
        std::cmp::Ordering::Less => {}
    }
    Ok(())
}

fn newer_timestamp(
    current_at: &str,
    update_at: &str,
    update_instant: OffsetDateTime,
    id: &str,
    track: &'static str,
    source: &str,
) -> Result<String, String> {
    if timestamp_is_newer(current_at, update_instant, id, track, source)? {
        Ok(update_at.to_owned())
    } else {
        Ok(current_at.to_owned())
    }
}

fn timestamp_is_newer(
    current_at: &str,
    update_instant: OffsetDateTime,
    id: &str,
    track: &'static str,
    source: &str,
) -> Result<bool, String> {
    let current_instant = parse_timestamp(current_at, &format!("{source} {id} {track}"))?;
    Ok(update_instant > current_instant)
}

fn parse_timestamp(value: &str, field: &str) -> Result<OffsetDateTime, String> {
    OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| format!("{field} must be a valid RFC3339 timestamp"))
}

fn current_release_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("workspace package version should be valid")
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

fn copy_distribution_file(file_name: &str, directory: &Path) -> Result<(), String> {
    let source = repository_root().join(file_name);
    require_regular_file(&source)?;
    let contents = fs::read(&source)
        .map_err(|error| format!("could not read '{}': {error}", source.display()))?;
    fs::write(directory.join(file_name), contents)
        .map_err(|error| format!("could not write {file_name}: {error}"))
}

fn citation_contents() -> Result<String, String> {
    let path = repository_root().join(CITATION_FILE_NAME);
    fs::read_to_string(&path)
        .map_err(|error| format!("could not read '{}': {error}", path.display()))
}

fn repository_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn replace_manifest_version(path: &Path, current: &str, next: &str) -> Result<(), String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("could not read '{}': {error}", path.display()))?;
    let needle = format!("version = \"{current}\"");
    let replacement = format!("version = \"{next}\"");
    let mut section = ManifestSection::Other;
    let mut updated = String::with_capacity(contents.len());
    for line in contents.split_inclusive('\n') {
        let line_without_newline = line.strip_suffix('\n').unwrap_or(line);
        let trimmed = line_without_newline.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = manifest_section(trimmed);
        }
        let replace = matches!(
            section,
            ManifestSection::WorkspacePackage | ManifestSection::LocalDependency
        ) || local_dependency_inline_table(trimmed);
        if replace {
            updated.push_str(&line_without_newline.replacen(&needle, &replacement, 1));
            updated.push_str(&line[line_without_newline.len()..]);
        } else {
            updated.push_str(line);
        }
    }
    fs::write(path, updated)
        .map_err(|error| format!("could not update '{}': {error}", path.display()))
}

#[derive(Clone, Copy)]
enum ManifestSection {
    WorkspacePackage,
    LocalDependency,
    Other,
}

fn manifest_section(header: &str) -> ManifestSection {
    if header == "[workspace.package]" {
        return ManifestSection::WorkspacePackage;
    }
    let section = header
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or_default();
    let Some((scope, dependency)) = section.rsplit_once('.') else {
        return ManifestSection::Other;
    };
    let dependency_kind = scope.rsplit('.').next().unwrap_or_default();
    if matches!(
        dependency_kind,
        "dependencies" | "dev-dependencies" | "build-dependencies"
    ) && LOCAL_PACKAGE_NAMES.contains(&dependency)
    {
        ManifestSection::LocalDependency
    } else {
        ManifestSection::Other
    }
}

fn local_dependency_inline_table(line: &str) -> bool {
    LOCAL_PACKAGE_NAMES.iter().any(|name| {
        line.strip_prefix(name)
            .is_some_and(|remainder| remainder.trim_start().starts_with('='))
            && line.contains("path =")
    })
}

fn replace_lockfile_version(path: &Path, current: &str, next: &str) -> Result<(), String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("could not read '{}': {error}", path.display()))?;
    let mut package_name = None;
    let mut updated = String::with_capacity(contents.len());

    for line in contents.split_inclusive('\n') {
        let line_without_newline = line.strip_suffix('\n').unwrap_or(line);
        let trimmed = line_without_newline.trim();
        if trimmed == "[[package]]" {
            package_name = None;
        } else if let Some(name) = trimmed
            .strip_prefix("name = \"")
            .and_then(|value| value.strip_suffix('"'))
        {
            package_name = LOCAL_PACKAGE_NAMES.contains(&name).then_some(name);
        }

        if package_name.is_some() && trimmed == format!("version = \"{current}\"") {
            let indentation = &line_without_newline
                [..line_without_newline.len() - line_without_newline.trim_start().len()];
            updated.push_str(indentation);
            let _ = write!(updated, "version = \"{next}\"");
            updated.push_str(&line[line_without_newline.len()..]);
            package_name = None;
        } else {
            updated.push_str(line);
        }
    }

    fs::write(path, updated)
        .map_err(|error| format!("could not update '{}': {error}", path.display()))
}

fn replace_citation_version(path: &Path, next: &str) -> Result<(), String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("could not read '{}': {error}", path.display()))?;
    let mut replaced = false;
    let mut updated = String::with_capacity(contents.len());

    for line in contents.split_inclusive('\n') {
        let line_without_newline = line.strip_suffix('\n').unwrap_or(line);
        if line_without_newline.trim_start().starts_with("version:") {
            let indentation = &line_without_newline
                [..line_without_newline.len() - line_without_newline.trim_start().len()];
            updated.push_str(indentation);
            let _ = write!(updated, "version: \"{next}\"");
            updated.push_str(&line[line_without_newline.len()..]);
            replaced = true;
        } else {
            updated.push_str(line);
        }
    }

    if !replaced {
        return Err(format!(
            "{CITATION_FILE_NAME} does not contain a version field"
        ));
    }
    fs::write(path, updated)
        .map_err(|error| format!("could not update '{}': {error}", path.display()))
}

fn validate_citation_version() -> Result<(), String> {
    let citation = citation_contents()?;
    let version = citation
        .lines()
        .find_map(|line| line.trim().strip_prefix("version:"))
        .map(str::trim)
        .and_then(|value| {
            value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .or_else(|| {
                    value
                        .strip_prefix('\'')
                        .and_then(|value| value.strip_suffix('\''))
                })
                .or(Some(value))
        })
        .filter(|value| !value.is_empty());

    match version {
        Some(env!("CARGO_PKG_VERSION")) => Ok(()),
        Some(version) => Err(format!(
            "{CITATION_FILE_NAME} version '{version}' does not match workspace version {}",
            env!("CARGO_PKG_VERSION")
        )),
        None => Err(format!(
            "{CITATION_FILE_NAME} does not contain a version field"
        )),
    }
}

fn reject_unexpected_files(directory: &Path, expected: &BTreeSet<String>) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("could not inspect '{}': {error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("could not inspect release entry: {error}"))?;
        let name = file_name(&entry.path())?;
        if !expected.contains(&name) {
            return Err(format!("unexpected file '{name}' in the release directory"));
        }
    }
    Ok(())
}

fn write_combined_checksums(directory: &Path, expected: &BTreeSet<String>) -> Result<(), String> {
    let mut output = String::new();
    for file_name in expected {
        let digest = checksum(&directory.join(file_name))?;
        writeln!(output, "{digest}  {file_name}")
            .map_err(|error| format!("could not format combined checksums: {error}"))?;
    }
    fs::write(directory.join("SHA256SUMS"), output)
        .map_err(|error| format!("could not write combined checksums: {error}"))
}

fn file_name(path: &Path) -> Result<String, String> {
    path.file_name()
        .and_then(OsStr::to_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("release path '{}' is not valid UTF-8", path.display()))
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{
        AUXILIARY_ARTIFACTS, CARGO_MANIFEST_FILES, CITATION_FILE_NAME, COMPATIBILITY_FILE_NAME,
        HarnessRequirement, LOCAL_PACKAGE_NAMES, RELEASE_TARGETS, VerificationEntry,
        VerificationRelease, artifact_file_name, bundled_compatibility_manifest,
        current_release_version, generate_compatibility_feed, generate_metadata,
        merge_compatibility_feed, merge_evidence_pair, merge_verification_entry,
        replace_lockfile_version, replace_manifest_version, validate_releases, validate_tag,
    };
    use nan_harness_core::HarnessKind;
    use semver::Version;
    use serde_json::Value;
    use std::fs;

    #[test]
    fn accepts_only_the_exact_workspace_release_tag() {
        assert!(validate_tag(&format!("v{}", env!("CARGO_PKG_VERSION"))).is_ok());
        assert!(validate_tag(env!("CARGO_PKG_VERSION")).is_err());
        assert!(validate_tag("v999.0.0").is_err());
    }

    #[test]
    fn creates_the_complete_release_contract() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        fs::write(directory.path().join("install.sh"), "installer")
            .expect("shell installer should exist");
        fs::write(directory.path().join("install.ps1"), "installer")
            .expect("PowerShell installer should exist");
        for target in RELEASE_TARGETS {
            fs::write(directory.path().join(artifact_file_name(target)), target)
                .expect("artifact should exist");
        }
        for artifact in AUXILIARY_ARTIFACTS {
            fs::write(directory.path().join(artifact), artifact)
                .expect("auxiliary artifact should exist");
        }

        let tag = format!("v{}", env!("CARGO_PKG_VERSION"));
        generate_metadata(&tag, "DavidLMS/nan-harness", directory.path())
            .expect("metadata should be generated");

        let manifest: Value = serde_json::from_slice(
            &fs::read(directory.path().join("update-manifest.json"))
                .expect("manifest should exist"),
        )
        .expect("manifest should be valid JSON");
        assert_eq!(manifest["schemaVersion"], 1);
        assert_eq!(manifest["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(
            manifest["artifacts"]
                .as_array()
                .expect("artifacts should be an array")
                .len(),
            RELEASE_TARGETS.len()
        );
        assert!(
            manifest["artifacts"]
                .as_array()
                .expect("artifacts should be an array")
                .iter()
                .all(|artifact| artifact["url"]
                    .as_str()
                    .is_some_and(|url| !url.contains("canary")))
        );
        let compatibility: Value = serde_json::from_slice(
            &fs::read(directory.path().join(COMPATIBILITY_FILE_NAME))
                .expect("compatibility manifest should exist"),
        )
        .expect("compatibility manifest should be valid JSON");
        assert_eq!(compatibility["schemaVersion"], 2);
        assert_eq!(
            compatibility["releases"][0]["verifications"][0]["compatibleAt"],
            "2026-08-29T00:00:00Z"
        );
        assert_eq!(
            compatibility["releases"][0]["verifications"]
                .as_array()
                .expect("verifications should be an array")
                .len(),
            15
        );

        let citation = fs::read_to_string(directory.path().join(CITATION_FILE_NAME))
            .expect("citation file should be generated");
        assert!(citation.contains(&format!("version: \"{}\"", env!("CARGO_PKG_VERSION"))));

        let checksums = fs::read_to_string(directory.path().join("SHA256SUMS"))
            .expect("checksums should exist");
        assert!(checksums.contains("  install.sh\n"));
        assert!(checksums.contains("  CITATION.cff\n"));
        assert!(checksums.contains("  compatibility.json\n"));
        assert!(checksums.contains("  LICENSE\n"));
        assert!(checksums.contains("  NOTICE.md\n"));
        assert!(checksums.contains("  update-manifest.json\n"));
        for artifact in AUXILIARY_ARTIFACTS {
            assert!(checksums.contains(&format!("  {artifact}\n")));
        }
    }

    #[test]
    fn version_updates_only_touch_workspace_and_local_packages() {
        assert!(CARGO_MANIFEST_FILES.contains(&"crates/nan-harness-private-fs/Cargo.toml"));
        assert!(LOCAL_PACKAGE_NAMES.contains(&"nan-harness-private-fs"));

        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let manifest = directory.path().join("Cargo.toml");
        fs::write(
            &manifest,
            concat!(
                "[workspace.package]\n",
                "version = \"0.0.1\"\n",
                "\n",
                "[workspace.dependencies]\n",
                "nan-harness-core = { path = \"core\", version = \"0.0.1\" }\n",
                "nan-harness-diagnostics = { path = \"diagnostics\", version = \"0.0.1\" }\n",
                "nan-harness-private-fs = { path = \"private-fs\", version = \"0.0.1\" }\n",
                "unrelated = { version = \"0.0.1\" }\n",
                "\n",
                "[dependencies.nan-harness-runtime]\n",
                "path = \"runtime\"\n",
                "version = \"0.0.1\"\n",
            ),
        )
        .expect("manifest fixture should exist");

        replace_manifest_version(&manifest, "0.0.1", "0.0.2")
            .expect("manifest versions should update");
        let updated = fs::read_to_string(manifest).expect("updated manifest should be readable");

        assert!(updated.contains("version = \"0.0.2\""));
        assert!(updated.contains("nan-harness-core = { path = \"core\", version = \"0.0.2\" }"));
        assert!(
            updated.contains(
                "nan-harness-diagnostics = { path = \"diagnostics\", version = \"0.0.2\" }"
            )
        );
        assert!(updated.contains(
            "[dependencies.nan-harness-runtime]\npath = \"runtime\"\nversion = \"0.0.2\""
        ));
        assert!(
            updated.contains(
                "nan-harness-private-fs = { path = \"private-fs\", version = \"0.0.2\" }"
            )
        );
        assert!(updated.contains("unrelated = { version = \"0.0.1\" }"));

        let private_manifest = directory.path().join("private-fs/Cargo.toml");
        fs::create_dir_all(
            private_manifest
                .parent()
                .expect("fixture parent should exist"),
        )
        .expect("private filesystem fixture directory should exist");
        fs::write(
            &private_manifest,
            concat!(
                "[package]\n",
                "name = \"nan-harness-private-fs\"\n",
                "version = \"0.0.1\"\n",
                "\n",
                "[dev-dependencies]\n",
                "nan-harness-test-support = { path = \"../test-support\", version = \"0.0.1\" }\n",
            ),
        )
        .expect("private filesystem manifest fixture should exist");

        replace_manifest_version(&private_manifest, "0.0.1", "0.0.2")
            .expect("private filesystem manifest versions should update");
        let private_updated =
            fs::read_to_string(&private_manifest).expect("private manifest should be readable");
        assert!(private_updated.contains("version = \"0.0.2\""));
        assert!(private_updated.contains(
            "nan-harness-test-support = { path = \"../test-support\", version = \"0.0.2\" }"
        ));

        let lockfile = directory.path().join("Cargo.lock");
        fs::write(
            &lockfile,
            concat!(
                "version = 4\n\n",
                "[[package]]\n",
                "name = \"nan-harness-private-fs\"\n",
                "version = \"0.0.1\"\n",
                "dependencies = [\n",
                " \"nan-harness-test-support\",\n",
                "]\n\n",
                "[[package]]\n",
                "name = \"nan-harness-test-support\"\n",
                "version = \"0.0.1\"\n\n",
                "[[package]]\n",
                "name = \"unrelated\"\n",
                "version = \"0.0.1\"\n",
            ),
        )
        .expect("lockfile fixture should exist");

        replace_lockfile_version(&lockfile, "0.0.1", "0.0.2")
            .expect("local package lockfile versions should update");
        let lock_updated =
            fs::read_to_string(lockfile).expect("updated lockfile should be readable");
        assert!(lock_updated.contains("name = \"nan-harness-private-fs\"\nversion = \"0.0.2\""));
        assert!(lock_updated.contains("name = \"nan-harness-test-support\"\nversion = \"0.0.2\""));
        assert!(lock_updated.contains("name = \"unrelated\"\nversion = \"0.0.1\""));
    }

    #[test]
    fn compatibility_merges_only_known_non_regressing_updates() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let base = directory.path().join("base.json");
        let updates = directory.path().join("updates");
        let output = directory.path().join("merged.json");
        fs::create_dir(&updates).expect("updates directory should exist");
        generate_compatibility_feed(&base).expect("base feed should be generated");
        fs::write(
            updates.join("fx.json"),
            r#"{"id":"fx","lastCompatibleVersion":"0.0.4","compatibleAt":"2026-08-20T08:00:00Z","lastLiveVerifiedVersion":"0.0.4","liveVerifiedAt":"2026-08-20T08:00:00Z"}"#,
        )
        .expect("fx update should exist");

        merge_compatibility_feed(&base, &updates, &output)
            .expect("compatibility feed should merge");
        let merged: Value =
            serde_json::from_slice(&fs::read(output).expect("merged feed should be readable"))
                .expect("merged feed should be JSON");
        assert_eq!(merged["schemaVersion"], 2);
        let fx = merged["releases"][0]["verifications"]
            .as_array()
            .expect("verifications should be an array")
            .iter()
            .find(|entry| entry["id"] == "fx")
            .expect("fx should remain in the feed");

        assert_eq!(fx["lastCompatibleVersion"], "0.0.7");
        assert_eq!(fx["lastLiveVerifiedVersion"], "0.0.4");
        assert_eq!(fx["compatibleAt"], "2026-08-29T00:00:00Z");
    }

    #[test]
    fn compatibility_preserves_releases_and_merges_partial_updates_monotonically() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let base = directory.path().join("base.json");
        let updates = directory.path().join("updates");
        let output = directory.path().join("merged.json");
        fs::create_dir(&updates).expect("updates directory should exist");
        fs::write(
            &base,
            format!(
                r#"{{"schemaVersion":2,"releases":[{{"nanHarnessVersion":"0.0.5","verifications":[{{"id":"fx","lastCompatibleVersion":"0.0.5","compatibleAt":"2026-08-01T00:00:00Z"}}]}},{{"nanHarnessVersion":"{}","verifications":[]}}]}}"#,
                env!("CARGO_PKG_VERSION")
            ),
        )
        .expect("base feed should exist");
        fs::write(
            updates.join("fx.json"),
            r#"{"id":"fx","lastCompatibleVersion":"0.0.4","compatibleAt":"2026-08-01T00:00:00Z"}"#,
        )
        .expect("partial update should exist");

        merge_compatibility_feed(&base, &updates, &output)
            .expect("partial compatibility update should merge");
        let merged: Value =
            serde_json::from_slice(&fs::read(output).expect("merged feed should be readable"))
                .expect("merged feed should be JSON");
        let releases = merged["releases"]
            .as_array()
            .expect("releases should be an array");
        assert_eq!(releases.len(), 2);
        assert_eq!(releases[0]["nanHarnessVersion"], "0.0.5");
        let current = releases
            .iter()
            .find(|release| release["nanHarnessVersion"] == env!("CARGO_PKG_VERSION"))
            .expect("current release should remain in the feed");
        assert!(current["verifications"].as_array().is_some());
    }

    #[test]
    fn compatibility_does_not_seed_a_new_release_before_an_update() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let base = directory.path().join("base.json");
        let updates = directory.path().join("updates");
        let output = directory.path().join("merged.json");
        fs::create_dir(&updates).expect("updates directory should exist");
        fs::write(
            &base,
            r#"{"schemaVersion":2,"releases":[{"nanHarnessVersion":"0.0.5","verifications":[]}]}"#,
        )
        .expect("base feed should exist");
        fs::write(
            updates.join("fx.json"),
            format!(
                r#"{{"nanHarnessVersion":"{}","id":"fx","lastCompatibleVersion":"0.0.4","compatibleAt":"2026-08-20T08:00:00Z"}}"#,
                env!("CARGO_PKG_VERSION")
            ),
        )
        .expect("fx update should exist");

        merge_compatibility_feed(&base, &updates, &output)
            .expect("compatibility feed should merge");
        let merged: Value =
            serde_json::from_slice(&fs::read(output).expect("merged feed should be readable"))
                .expect("merged feed should be JSON");
        let current = merged["releases"]
            .as_array()
            .expect("releases should be an array")
            .iter()
            .find(|release| release["nanHarnessVersion"] == env!("CARGO_PKG_VERSION"))
            .expect("updated release should exist");
        assert_eq!(current["verifications"].as_array().unwrap().len(), 1);
        assert_eq!(current["verifications"][0]["id"], "fx");
    }

    #[test]
    fn compatibility_merge_rejects_malformed_pairs_and_missing_evidence() {
        let requirements = requirements();
        let cases = [
            VerificationEntry {
                id: "codex".to_owned(),
                last_compatible_version: Some(Version::new(0, 147, 0)),
                compatible_at: None,
                last_live_verified_version: None,
                live_verified_at: None,
            },
            VerificationEntry {
                id: "codex".to_owned(),
                last_compatible_version: None,
                compatible_at: None,
                last_live_verified_version: None,
                live_verified_at: None,
            },
            VerificationEntry {
                id: "codex".to_owned(),
                last_compatible_version: Some(Version::new(0, 147, 0)),
                compatible_at: Some("2026-08-19".to_owned()),
                last_live_verified_version: None,
                live_verified_at: None,
            },
        ];
        for entry in cases {
            let result = validate_releases(
                &[VerificationRelease {
                    nan_harness_version: current_release_version(),
                    verifications: vec![entry],
                }],
                &requirements,
                "test feed",
            );
            assert!(result.is_err());
        }
    }

    #[test]
    fn compatibility_merge_rejects_minimum_duplicate_and_live_order_violations() {
        let requirements = requirements();
        let below_minimum = entry("codex", "0.145.0", "2026-08-19T00:00:00Z");
        assert!(validate_single(&requirements, below_minimum).is_err());

        let duplicate = entry("codex", "0.147.0", "2026-08-19T00:00:00Z");
        assert!(
            validate_releases(
                &[VerificationRelease {
                    nan_harness_version: current_release_version(),
                    verifications: vec![duplicate.clone(), duplicate],
                }],
                &requirements,
                "test feed",
            )
            .is_err()
        );
        assert!(
            validate_releases(
                &[
                    VerificationRelease {
                        nan_harness_version: current_release_version(),
                        verifications: Vec::new(),
                    },
                    VerificationRelease {
                        nan_harness_version: current_release_version(),
                        verifications: Vec::new(),
                    },
                ],
                &requirements,
                "test feed",
            )
            .is_err()
        );

        let live_ahead = VerificationEntry {
            id: "codex".to_owned(),
            last_compatible_version: Some(Version::new(0, 146, 0)),
            compatible_at: Some("2026-08-19T00:00:00Z".to_owned()),
            last_live_verified_version: Some(Version::new(0, 147, 0)),
            live_verified_at: Some("2026-08-20T00:00:00Z".to_owned()),
        };
        assert!(validate_single(&requirements, live_ahead).is_err());
    }

    #[test]
    fn compatibility_merge_preserves_unknown_ids_and_merges_pairs_atomically() {
        let requirements = requirements();
        let unknown = entry("future-harness", "99.0.0", "2026-08-19T00:00:00Z");
        assert!(validate_single(&requirements, unknown.clone()).is_ok());

        let mut current = entry("fx", "0.0.3", "2026-08-20T00:00:00Z");
        merge_verification_entry(
            &mut current,
            &entry("fx", "0.0.4", "2026-08-19T00:00:00Z"),
            "test feed",
        )
        .expect("higher version should replace the complete pair");
        assert_eq!(current.last_compatible_version, Some(Version::new(0, 0, 4)));
        assert_eq!(
            current.compatible_at.as_deref(),
            Some("2026-08-20T00:00:00Z")
        );

        merge_verification_entry(
            &mut current,
            &entry("fx", "0.0.5", "2026-08-18T00:00:00Z"),
            "test feed",
        )
        .expect("higher version should retain the newer existing timestamp");
        assert_eq!(current.last_compatible_version, Some(Version::new(0, 0, 5)));
        assert_eq!(
            current.compatible_at.as_deref(),
            Some("2026-08-20T00:00:00Z")
        );

        merge_verification_entry(
            &mut current,
            &entry("fx", "0.0.4", "2026-08-20T00:00:00Z"),
            "test feed",
        )
        .expect("equal version with later timestamp should advance the timestamp");
        assert_eq!(
            current.compatible_at.as_deref(),
            Some("2026-08-20T00:00:00Z")
        );
        let unchanged = current.clone();
        merge_verification_entry(
            &mut current,
            &entry("fx", "0.0.3", "2026-08-21T00:00:00Z"),
            "test feed",
        )
        .expect("lower version should be ignored");
        assert_eq!(current, unchanged);

        merge_verification_entry(
            &mut current,
            &entry("fx", "0.0.5", "2026-08-19T00:00:00Z"),
            "test feed",
        )
        .expect("equal version with an older timestamp should be ignored");
        assert_eq!(current, unchanged);

        let mut absent_version = None;
        let mut stray_timestamp = Some("2026-08-21T00:00:00Z".to_owned());
        merge_evidence_pair(
            &mut absent_version,
            &mut stray_timestamp,
            None,
            Some(&"2026-08-22T00:00:00Z".to_owned()),
            "fx",
            "compatible",
            "test feed",
        )
        .expect("an update without a version should be ignored");
        assert_eq!(absent_version, None);
        assert_eq!(stray_timestamp.as_deref(), Some("2026-08-21T00:00:00Z"));

        let mut incomplete_version = Some(Version::new(0, 0, 3));
        let mut incomplete_timestamp = None;
        assert!(
            merge_evidence_pair(
                &mut incomplete_version,
                &mut incomplete_timestamp,
                Some(&Version::new(0, 0, 4)),
                Some(&"2026-08-22T00:00:00Z".to_owned()),
                "fx",
                "compatible",
                "test feed",
            )
            .is_err()
        );
    }

    fn requirements() -> std::collections::BTreeMap<HarnessKind, HarnessRequirement> {
        let manifest = bundled_compatibility_manifest().expect("embedded manifest");
        manifest
            .harnesses
            .into_iter()
            .map(|entry| {
                (
                    entry.id,
                    HarnessRequirement {
                        minimum_version: entry.minimum_version,
                        compatible_version: entry.last_compatible_version,
                    },
                )
            })
            .collect()
    }

    fn validate_single(
        requirements: &std::collections::BTreeMap<HarnessKind, HarnessRequirement>,
        entry: VerificationEntry,
    ) -> Result<(), String> {
        validate_releases(
            &[VerificationRelease {
                nan_harness_version: current_release_version(),
                verifications: vec![entry],
            }],
            requirements,
            "test feed",
        )
    }

    fn entry(id: &str, version: &str, timestamp: &str) -> VerificationEntry {
        VerificationEntry {
            id: id.to_owned(),
            last_compatible_version: Some(Version::parse(version).expect("version")),
            compatible_at: Some(timestamp.to_owned()),
            last_live_verified_version: None,
            live_verified_at: None,
        }
    }
}
