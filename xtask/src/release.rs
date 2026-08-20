use serde::Serialize;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

const MAX_ARTIFACT_SIZE: u64 = 128 * 1024 * 1024;
const INSTALLER_FILES: [&str; 2] = ["install.sh", "install.ps1"];
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

pub(crate) fn validate_tag(tag: &str) -> Result<(), String> {
    let expected = format!("v{}", env!("CARGO_PKG_VERSION"));
    if tag == expected {
        Ok(())
    } else {
        Err(format!(
            "release tag '{tag}' does not match workspace version {}; expected '{expected}'",
            env!("CARGO_PKG_VERSION")
        ))
    }
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
    use super::{RELEASE_TARGETS, artifact_file_name, generate_metadata, validate_tag};
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

        let checksums = fs::read_to_string(directory.path().join("SHA256SUMS"))
            .expect("checksums should exist");
        assert!(checksums.contains("  install.sh\n"));
        assert!(checksums.contains("  update-manifest.json\n"));
    }
}
