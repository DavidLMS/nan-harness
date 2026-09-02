use crate::changelog;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) const CITATION_FILE_NAME: &str = "CITATION.cff";

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

pub(super) fn validate_repository(repository: &str) -> Result<(), String> {
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

pub(super) fn require_regular_file(path: &Path) -> Result<(), String> {
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

pub(super) fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn citation_contents() -> Result<String, String> {
    let path = repository_root().join(CITATION_FILE_NAME);
    fs::read_to_string(&path)
        .map_err(|error| format!("could not read '{}': {error}", path.display()))
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
