use super::validation::{CITATION_FILE_NAME, repository_root};
use crate::changelog;
use semver::Version;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use time::OffsetDateTime;

pub(super) const CARGO_MANIFEST_FILES: [&str; 13] = [
    "Cargo.toml",
    "crates/nan-harness-adapters/Cargo.toml",
    "crates/nan-harness-bridge/Cargo.toml",
    "crates/nan-harness-canary/Cargo.toml",
    "crates/nan-harness-cli/Cargo.toml",
    "crates/nan-harness-coordinator/Cargo.toml",
    "crates/nan-harness-core/Cargo.toml",
    "crates/nan-harness-diagnostics/Cargo.toml",
    "crates/nan-harness-private-fs/Cargo.toml",
    "crates/nan-harness-runtime/Cargo.toml",
    "crates/nan-harness-telemetry/Cargo.toml",
    "crates/nan-harness-test-support/Cargo.toml",
    "xtask/Cargo.toml",
];
pub(super) const LOCAL_PACKAGE_NAMES: [&str; 12] = [
    "nan-harness-adapters",
    "nan-harness-bridge",
    "nan-harness-canary",
    "nan-harness-cli",
    "nan-harness-coordinator",
    "nan-harness-core",
    "nan-harness-diagnostics",
    "nan-harness-private-fs",
    "nan-harness-runtime",
    "nan-harness-telemetry",
    "nan-harness-test-support",
    "xtask",
];

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

pub(super) fn replace_manifest_version(
    path: &Path,
    current: &str,
    next: &str,
) -> Result<(), String> {
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

pub(super) fn replace_lockfile_version(
    path: &Path,
    current: &str,
    next: &str,
) -> Result<(), String> {
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
