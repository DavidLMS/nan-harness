use super::ChatGptDesktopError;
use semver::Version;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(super) struct ChatGptInstallation {
    pub(super) executable: PathBuf,
    pub(super) app_version: Version,
    pub(super) bundled_codex_version: Version,
}

pub(super) fn discover_installation(
    explicit: Option<&Path>,
) -> Result<ChatGptInstallation, ChatGptDesktopError> {
    super::platform::discover_installation(explicit)
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux", test))]
pub(super) fn parse_version_output(output: &str) -> Result<Version, ChatGptDesktopError> {
    output
        .split_whitespace()
        .map(|token| {
            token.trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && !matches!(character, '.' | '-' | '+')
            })
        })
        .find_map(|candidate| Version::parse(candidate.trim_start_matches('v')).ok())
        .ok_or(ChatGptDesktopError::UnparseableVersion)
}
