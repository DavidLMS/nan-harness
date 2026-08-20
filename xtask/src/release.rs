use nan_harness_core::{CompatibilityManifest, HarnessKind};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const MAX_ARTIFACT_SIZE: u64 = 128 * 1024 * 1024;
const INSTALLER_FILES: [&str; 2] = ["install.sh", "install.ps1"];
const CITATION_FILE_NAME: &str = "CITATION.cff";
const COMPATIBILITY_FILE_NAME: &str = "compatibility.json";
const COMPATIBILITY_SOURCE_PATH: &str = "crates/nan-harness-runtime/resources/compatibility.json";
const DISTRIBUTION_FILES: [&str; 3] = [CITATION_FILE_NAME, "LICENSE", "NOTICE.md"];
const CARGO_MANIFEST_FILES: [&str; 9] = [
    "Cargo.toml",
    "crates/nan-harness-adapters/Cargo.toml",
    "crates/nan-harness-bridge/Cargo.toml",
    "crates/nan-harness-cli/Cargo.toml",
    "crates/nan-harness-core/Cargo.toml",
    "crates/nan-harness-runtime/Cargo.toml",
    "crates/nan-harness-telemetry/Cargo.toml",
    "crates/nan-harness-test-support/Cargo.toml",
    "xtask/Cargo.toml",
];
const LOCAL_PACKAGE_NAMES: [&str; 8] = [
    "nan-harness-adapters",
    "nan-harness-bridge",
    "nan-harness-cli",
    "nan-harness-core",
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
    generated_at: String,
    verifications: Vec<VerificationEntry>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VerificationEntry {
    id: HarnessKind,
    last_verified_version: Version,
}

pub(crate) fn set_version(raw_version: &str) -> Result<(), String> {
    let version = raw_version.strip_prefix('v').unwrap_or(raw_version);
    let version = Version::parse(version)
        .map_err(|error| format!("release version '{raw_version}' is invalid: {error}"))?
        .to_string();
    let current_version = env!("CARGO_PKG_VERSION");
    let root = repository_root();

    for manifest in CARGO_MANIFEST_FILES {
        replace_manifest_version(&root.join(manifest), current_version, &version)?;
    }
    replace_lockfile_version(&root.join("Cargo.lock"), current_version, &version)?;
    replace_citation_version(&root.join(CITATION_FILE_NAME), &version)?;
    Ok(())
}

pub(crate) fn validate_tag(tag: &str) -> Result<(), String> {
    let expected = format!("v{}", env!("CARGO_PKG_VERSION"));
    if tag != expected {
        return Err(format!(
            "release tag '{tag}' does not match workspace version {}; expected '{expected}'",
            env!("CARGO_PKG_VERSION")
        ));
    }

    validate_citation_version()
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
    format!("nan-{target}{extension}")
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
    let mut versions = source
        .harnesses
        .iter()
        .map(|entry| (entry.id, entry.last_verified_version.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let minimums = source
        .harnesses
        .iter()
        .map(|entry| (entry.id, entry.minimum_version.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let base_manifest = read_verification_manifest(base)?;
    validate_manifest_header(&base_manifest)?;
    apply_verification_entries(
        &mut versions,
        &minimums,
        base_manifest.verifications,
        "base compatibility feed",
    )?;

    let mut update_count = 0_usize;
    let mut update_ids = BTreeSet::new();
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
        let verification: VerificationEntry = serde_json::from_slice(&contents)
            .map_err(|error| format!("could not parse '{}': {error}", path.display()))?;
        if !update_ids.insert(verification.id) {
            return Err(format!(
                "compatibility updates contain duplicate entry for {}",
                verification.id
            ));
        }
        apply_verification_entries(&mut versions, &minimums, [verification], "canary update")?;
        update_count += 1;
    }
    if update_count == 0 {
        return Err("compatibility updates contain no verification entries".to_owned());
    }

    let generated_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| format!("could not format compatibility timestamp: {error}"))?;
    let manifest = VerificationManifest {
        schema_version: 1,
        generated_at,
        verifications: versions
            .into_iter()
            .map(|(id, last_verified_version)| VerificationEntry {
                id,
                last_verified_version,
            })
            .collect(),
    };
    write_verification_manifest(output, &manifest)
}

fn bundled_compatibility_manifest() -> Result<CompatibilityManifest, String> {
    let source_path = repository_root().join(COMPATIBILITY_SOURCE_PATH);
    let source = fs::read(&source_path)
        .map_err(|error| format!("could not read '{}': {error}", source_path.display()))?;
    serde_json::from_slice(&source).map_err(|error| {
        format!(
            "could not parse compatibility manifest '{}': {error}",
            source_path.display()
        )
    })
}

fn bundled_verification_manifest() -> Result<VerificationManifest, String> {
    let source = bundled_compatibility_manifest()?;
    let verifications = source
        .harnesses
        .into_iter()
        .map(|entry| VerificationEntry {
            id: entry.id,
            last_verified_version: entry.last_verified_version,
        })
        .collect();
    Ok(VerificationManifest {
        schema_version: 1,
        generated_at: source.tested_at,
        verifications,
    })
}

fn read_verification_manifest(path: &Path) -> Result<VerificationManifest, String> {
    let contents =
        fs::read(path).map_err(|error| format!("could not read '{}': {error}", path.display()))?;
    serde_json::from_slice(&contents)
        .map_err(|error| format!("could not parse '{}': {error}", path.display()))
}

fn validate_manifest_header(manifest: &VerificationManifest) -> Result<(), String> {
    if manifest.schema_version != 1 {
        return Err(format!(
            "compatibility feed schema {} is not supported",
            manifest.schema_version
        ));
    }
    if manifest.generated_at.trim().is_empty() {
        return Err("compatibility feed has no generatedAt value".to_owned());
    }
    Ok(())
}

fn apply_verification_entries(
    versions: &mut std::collections::BTreeMap<HarnessKind, Version>,
    minimums: &std::collections::BTreeMap<HarnessKind, Version>,
    entries: impl IntoIterator<Item = VerificationEntry>,
    source: &str,
) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for entry in entries {
        if !seen.insert(entry.id) {
            return Err(format!(
                "{source} contains duplicate entry for {}",
                entry.id
            ));
        }
        let minimum = minimums
            .get(&entry.id)
            .ok_or_else(|| format!("{source} contains unknown harness {}", entry.id))?;
        if entry.last_verified_version < *minimum {
            return Err(format!(
                "{source} reports {} version {}, below minimum {minimum}",
                entry.id, entry.last_verified_version
            ));
        }
        let current = versions
            .get_mut(&entry.id)
            .expect("known minimums and versions have matching harnesses");
        if entry.last_verified_version > *current {
            *current = entry.last_verified_version;
        }
    }
    Ok(())
}

fn write_verification_manifest(path: &Path, manifest: &VerificationManifest) -> Result<(), String> {
    let mut payload = serde_json::to_vec_pretty(manifest)
        .map_err(|error| format!("could not serialize compatibility manifest: {error}"))?;
    payload.push(b'\n');
    fs::write(path, payload)
        .map_err(|error| format!("could not write compatibility manifest: {error}"))
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
        CITATION_FILE_NAME, COMPATIBILITY_FILE_NAME, RELEASE_TARGETS, artifact_file_name,
        generate_compatibility_feed, generate_metadata, merge_compatibility_feed,
        replace_manifest_version, validate_tag,
    };
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
        let compatibility: Value = serde_json::from_slice(
            &fs::read(directory.path().join(COMPATIBILITY_FILE_NAME))
                .expect("compatibility manifest should exist"),
        )
        .expect("compatibility manifest should be valid JSON");
        assert_eq!(compatibility["schemaVersion"], 1);
        assert_eq!(
            compatibility["verifications"]
                .as_array()
                .expect("verifications should be an array")
                .len(),
            14
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
    }

    #[test]
    fn version_updates_only_touch_workspace_and_local_packages() {
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
        assert!(updated.contains(
            "[dependencies.nan-harness-runtime]\npath = \"runtime\"\nversion = \"0.0.2\""
        ));
        assert!(updated.contains("unrelated = { version = \"0.0.1\" }"));
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
            r#"{"id":"fx","lastVerifiedVersion":"0.0.4"}"#,
        )
        .expect("fx update should exist");

        merge_compatibility_feed(&base, &updates, &output)
            .expect("compatibility feed should merge");
        let merged: Value =
            serde_json::from_slice(&fs::read(output).expect("merged feed should be readable"))
                .expect("merged feed should be JSON");
        let fx = merged["verifications"]
            .as_array()
            .expect("verifications should be an array")
            .iter()
            .find(|entry| entry["id"] == "fx")
            .expect("fx should remain in the feed");

        assert_eq!(fx["lastVerifiedVersion"], "0.0.4");
        assert!(
            merged["generatedAt"]
                .as_str()
                .is_some_and(|value| value.contains('T'))
        );
    }
}
