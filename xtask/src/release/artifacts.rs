use super::compatibility::{COMPATIBILITY_FILE_NAME, generate_compatibility_feed};
use super::validation::{
    CITATION_FILE_NAME, repository_root, require_regular_file, validate_repository, validate_tag,
};
use serde::Serialize;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

const MAX_ARTIFACT_SIZE: u64 = 128 * 1024 * 1024;
const INSTALLER_FILES: [&str; 2] = ["install.sh", "install.ps1"];
const DISTRIBUTION_FILES: [&str; 3] = [CITATION_FILE_NAME, "LICENSE", "NOTICE.md"];
pub(super) const RELEASE_TARGETS: [&str; 5] = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-musl",
    "x86_64-unknown-linux-musl",
    "x86_64-pc-windows-msvc",
];
pub(super) const AUXILIARY_ARTIFACTS: [&str; 2] = [
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

pub(super) fn artifact_file_name(target: &str) -> String {
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

fn copy_distribution_file(file_name: &str, directory: &Path) -> Result<(), String> {
    let source = repository_root().join(file_name);
    require_regular_file(&source)?;
    let contents = fs::read(&source)
        .map_err(|error| format!("could not read '{}': {error}", source.display()))?;
    fs::write(directory.join(file_name), contents)
        .map_err(|error| format!("could not write {file_name}: {error}"))
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
