use nan_harness_core::{CompatibilityManifest, DetectedHarness, HarnessKind, VersionStatus};
use semver::Version;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

const COMPATIBILITY_MANIFEST: &str = include_str!("../resources/compatibility.json");

#[derive(Debug, Clone, Copy, Default)]
pub struct DiscoveryOptions {
    pub allow_unsupported: bool,
    pub allow_untested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryReport {
    pub harness: DetectedHarness,
    pub last_verified_version: Version,
    pub minimum_supported_version: Version,
    pub warnings: Vec<String>,
}

/// Loads the compatibility manifest embedded in the runtime binary.
///
/// # Errors
///
/// Returns [`DiscoveryError`] if the bundled resource cannot be deserialized.
pub fn bundled_compatibility_manifest() -> Result<CompatibilityManifest, DiscoveryError> {
    serde_json::from_str(COMPATIBILITY_MANIFEST).map_err(DiscoveryError::InvalidManifest)
}

/// Locates a harness, runs its version command, and applies compatibility policy.
///
/// # Errors
///
/// Returns [`DiscoveryError`] when discovery, version detection, or policy checks fail.
pub fn discover_harness(
    kind: HarnessKind,
    executable_override: Option<&Path>,
    options: DiscoveryOptions,
) -> Result<DiscoveryReport, DiscoveryError> {
    let mut manifest = bundled_compatibility_manifest()?;
    crate::compatibility::apply_cached_verifications(&mut manifest);
    let entry = manifest
        .entry(kind)
        .ok_or(DiscoveryError::MissingCompatibilityEntry(kind))?;
    let executable = match executable_override {
        Some(path) => validate_executable(path)?,
        None => find_executable(kind.binary_name())
            .ok_or_else(|| DiscoveryError::ExecutableNotFound(kind.binary_name().to_owned()))?,
    };

    let output = Command::new(&executable)
        .arg("--version")
        .output()
        .map_err(|source| DiscoveryError::VersionCommand {
            executable: executable.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(DiscoveryError::VersionCommandFailed {
            executable,
            exit_code: output.status.code(),
        });
    }

    let detected_version = first_non_empty_line(&output.stdout, &output.stderr);
    let parsed_version = parse_version(&detected_version);
    let version_status = match parsed_version.as_ref() {
        Some(version) => manifest
            .classify(kind, version)
            .ok_or(DiscoveryError::MissingCompatibilityEntry(kind))?,
        None => VersionStatus::Unparseable,
    };

    enforce_version_policy(kind, version_status, &detected_version, options)?;
    let warnings = version_warnings(
        kind,
        version_status,
        &detected_version,
        &entry.last_verified_version,
    );

    Ok(DiscoveryReport {
        harness: DetectedHarness {
            kind,
            executable: executable.to_string_lossy().into_owned(),
            detected_version,
            version_status,
        },
        last_verified_version: entry.last_verified_version.clone(),
        minimum_supported_version: entry.minimum_version.clone(),
        warnings,
    })
}

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("bundled compatibility manifest is invalid: {0}")]
    InvalidManifest(serde_json::Error),
    #[error("compatibility manifest has no entry for {0}")]
    MissingCompatibilityEntry(HarnessKind),
    #[error("executable '{0}' was not found")]
    ExecutableNotFound(String),
    #[error("executable path '{}' is not a file", .0.display())]
    InvalidExecutable(PathBuf),
    #[error("could not run '{} --version': {source}", executable.display())]
    VersionCommand {
        executable: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "'{} --version' failed{}",
        executable.display(),
        exit_code.map_or_else(String::new, |code| format!(" with exit code {code}"))
    )]
    VersionCommandFailed {
        executable: PathBuf,
        exit_code: Option<i32>,
    },
    #[error(
        "{harness} version '{detected}' is older than the supported minimum; pass --allow-unsupported to continue"
    )]
    UnsupportedVersion {
        harness: HarnessKind,
        detected: String,
    },
    #[error(
        "could not parse {harness} version from '{detected}'; pass --allow-untested to continue"
    )]
    UnparseableVersion {
        harness: HarnessKind,
        detected: String,
    },
}

impl DiscoveryError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidManifest(_) | Self::MissingCompatibilityEntry(_) => "NH-DISCOVERY-001",
            Self::ExecutableNotFound(_) | Self::InvalidExecutable(_) => "NH-DISCOVERY-002",
            Self::VersionCommand { .. } | Self::VersionCommandFailed { .. } => "NH-DISCOVERY-003",
            Self::UnsupportedVersion { .. } => "NH-DISCOVERY-004",
            Self::UnparseableVersion { .. } => "NH-DISCOVERY-005",
        }
    }
}

fn validate_executable(path: &Path) -> Result<PathBuf, DiscoveryError> {
    if fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
        Ok(path.to_path_buf())
    } else {
        Err(DiscoveryError::InvalidExecutable(path.to_path_buf()))
    }
}

fn find_executable(binary_name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .flat_map(|directory| executable_candidates(&directory, binary_name))
        .find(|candidate| fs::metadata(candidate).is_ok_and(|metadata| metadata.is_file()))
}

fn executable_candidates(directory: &Path, binary_name: &str) -> Vec<PathBuf> {
    let base = directory.join(binary_name);
    if cfg!(windows) {
        let extensions = env::var_os("PATHEXT").unwrap_or_else(|| OsString::from(".EXE;.CMD;.BAT"));
        extensions
            .to_string_lossy()
            .split(';')
            .filter(|extension| !extension.is_empty())
            .map(|extension| directory.join(format!("{binary_name}{extension}")))
            .chain(std::iter::once(base))
            .collect()
    } else {
        vec![base]
    }
}

fn first_non_empty_line(stdout: &[u8], stderr: &[u8]) -> String {
    [stdout, stderr]
        .into_iter()
        .flat_map(|stream| {
            String::from_utf8_lossy(stream)
                .lines()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .map(|line| line.trim().to_owned())
        .find(|line| !line.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn parse_version(output: &str) -> Option<Version> {
    output.split_whitespace().find_map(|token| {
        let candidate = token
            .trim_matches(|character: char| !character.is_ascii_alphanumeric() && character != '.')
            .trim_start_matches('v');
        Version::parse(candidate).ok()
    })
}

fn enforce_version_policy(
    harness: HarnessKind,
    status: VersionStatus,
    detected: &str,
    options: DiscoveryOptions,
) -> Result<(), DiscoveryError> {
    match status {
        VersionStatus::OlderUnsupported if !options.allow_unsupported => {
            Err(DiscoveryError::UnsupportedVersion {
                harness,
                detected: detected.to_owned(),
            })
        }
        VersionStatus::Unparseable if !options.allow_untested => {
            Err(DiscoveryError::UnparseableVersion {
                harness,
                detected: detected.to_owned(),
            })
        }
        VersionStatus::Tested
        | VersionStatus::Supported
        | VersionStatus::NewerUntested
        | VersionStatus::OlderUnsupported
        | VersionStatus::Unparseable => Ok(()),
    }
}

fn version_warnings(
    harness: HarnessKind,
    status: VersionStatus,
    detected: &str,
    last_verified_version: &Version,
) -> Vec<String> {
    match status {
        VersionStatus::Tested | VersionStatus::Supported => Vec::new(),
        VersionStatus::NewerUntested => vec![format!(
            "{harness} version '{detected}' is newer than the last version verified by NaN Harness ({last_verified_version}); continuing with forward-compatible safeguards."
        )],
        VersionStatus::OlderUnsupported => vec![format!(
            "{harness} version '{detected}' is older than the supported minimum."
        )],
        VersionStatus::Unparseable => vec![format!(
            "NaN Harness could not parse the {harness} version from '{detected}'."
        )],
    }
}
