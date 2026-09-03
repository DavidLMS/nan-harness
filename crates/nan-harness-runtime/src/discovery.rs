use nan_harness_core::{
    CompatibilityManifest, DetectedHarness, HarnessCapability, HarnessCompatibility, HarnessKind,
    VersionStatus,
};
use semver::Version;
use std::collections::BTreeSet;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;
use thiserror::Error;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const COMPATIBILITY_MANIFEST: &str = include_str!("../resources/compatibility.json");
const VERSION_COMMAND_ATTEMPTS: usize = 3;
const VERSION_COMMAND_RETRY_DELAY: Duration = Duration::from_millis(10);
const CLAUDE_MODEL_PICKER_MIN_VERSION: (u64, u64, u64) = (2, 1, 243);
const FORWARD_COMPATIBILITY_QUIPS: [&str; 10] = [
    "In NaN we trust!",
    "May your compatibility checks be green and your stack traces short.",
    "Say every prayer you know.",
    "Pray to the machine spirits.",
    "Hold onto your butts.",
    "There is no spoon, only semver.",
    "Here be dragons—forward-compatible ones, hopefully.",
    "I've got a good feeling about this.",
    "So long, and thanks for all the semver.",
    "What could possibly go wrong?",
];

#[derive(Debug, Clone, Copy, Default)]
pub struct DiscoveryOptions {
    pub allow_unsupported: bool,
    pub allow_untested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryReport {
    pub harness: DetectedHarness,
    pub last_compatible_version: Version,
    pub compatible_at: String,
    pub last_live_verified_version: Option<Version>,
    pub live_verified_at: Option<String>,
    pub minimum_supported_version: Version,
    pub warnings: Vec<String>,
}

/// Loads the compatibility manifest embedded in the runtime binary.
///
/// # Errors
///
/// Returns [`DiscoveryError`] if the bundled resource cannot be deserialized or violates its
/// compatibility contract.
pub fn bundled_compatibility_manifest() -> Result<CompatibilityManifest, DiscoveryError> {
    let manifest: CompatibilityManifest =
        serde_json::from_str(COMPATIBILITY_MANIFEST).map_err(DiscoveryError::InvalidManifest)?;
    validate_embedded_manifest(&manifest).map_err(DiscoveryError::InvalidManifestContract)?;
    Ok(manifest)
}

fn validate_embedded_manifest(manifest: &CompatibilityManifest) -> Result<(), String> {
    if manifest.schema_version != CompatibilityManifest::SCHEMA_VERSION {
        return Err(format!(
            "schema {} is not supported",
            manifest.schema_version
        ));
    }
    parse_timestamp(&manifest.tested_at, "testedAt")?;
    let mut ids = BTreeSet::new();
    for entry in &manifest.harnesses {
        if !ids.insert(entry.id) {
            return Err(format!("duplicate harness entry for {}", entry.id));
        }
        if entry.last_compatible_version < entry.minimum_version {
            return Err(format!(
                "{} compatible version {} is below minimum {}",
                entry.id, entry.last_compatible_version, entry.minimum_version
            ));
        }
        parse_timestamp(&entry.compatible_at, "compatibleAt")?;
        match (&entry.last_live_verified_version, &entry.live_verified_at) {
            (None, None) => {}
            (Some(version), Some(timestamp)) => {
                if version < &entry.minimum_version {
                    return Err(format!(
                        "{} live version {} is below minimum {}",
                        entry.id, version, entry.minimum_version
                    ));
                }
                if version > &entry.last_compatible_version {
                    return Err(format!(
                        "{} live version {} is newer than compatible version {}",
                        entry.id, version, entry.last_compatible_version
                    ));
                }
                parse_timestamp(timestamp, "liveVerifiedAt")?;
            }
            _ => {
                return Err(format!(
                    "{} live evidence must include both version and timestamp",
                    entry.id
                ));
            }
        }
    }
    Ok(())
}

fn parse_timestamp(value: &str, field: &str) -> Result<OffsetDateTime, String> {
    OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| format!("{field} must be a valid RFC3339 timestamp"))
}

/// Locates and validates a harness executable.
///
/// # Errors
///
/// Returns [`DiscoveryError`] when an override is not executable or the harness cannot be found on
/// `PATH`.
pub fn locate_harness_executable(
    kind: HarnessKind,
    executable_override: Option<&Path>,
) -> Result<PathBuf, DiscoveryError> {
    match executable_override {
        Some(path) => validate_executable(path),
        None => find_executable(kind.binary_name())
            .ok_or_else(|| DiscoveryError::ExecutableNotFound(kind.binary_name().to_owned())),
    }
}

/// Inspects a located harness executable and applies compatibility policy.
///
/// # Errors
///
/// Returns [`DiscoveryError`] when version detection, capability detection, or compatibility
/// policy checks fail.
pub fn inspect_harness(
    kind: HarnessKind,
    executable: &Path,
    options: DiscoveryOptions,
) -> Result<DiscoveryReport, DiscoveryError> {
    let mut manifest = bundled_compatibility_manifest()?;
    crate::compatibility::apply_cached_verifications(&mut manifest);
    let entry = manifest
        .entry(kind)
        .ok_or(DiscoveryError::MissingCompatibilityEntry(kind))?;
    let version_arguments = version_arguments(entry)?;

    let version_command = format!("{} {}", executable.display(), version_arguments.join(" "));
    let output = run_version_command(executable, &version_arguments).map_err(|source| {
        DiscoveryError::VersionCommand {
            command: version_command.clone(),
            source,
        }
    })?;
    if !output.status.success() {
        return Err(DiscoveryError::VersionCommandFailed {
            command: version_command,
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
    let mut warnings = version_warnings(
        kind,
        version_status,
        &detected_version,
        parsed_version.as_ref(),
        &entry.last_compatible_version,
    );
    let (capabilities, capability_warnings) =
        detect_capabilities(kind, executable, parsed_version.as_ref());
    warnings.extend(capability_warnings);

    Ok(DiscoveryReport {
        harness: DetectedHarness {
            kind,
            executable: executable.to_string_lossy().into_owned(),
            detected_version,
            version_status,
            capabilities,
        },
        last_compatible_version: entry.last_compatible_version.clone(),
        compatible_at: entry.compatible_at.clone(),
        last_live_verified_version: entry.last_live_verified_version.clone(),
        live_verified_at: entry.live_verified_at.clone(),
        minimum_supported_version: entry.minimum_version.clone(),
        warnings,
    })
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
    let executable = locate_harness_executable(kind, executable_override)?;
    inspect_harness(kind, &executable, options)
}

fn detect_capabilities(
    kind: HarnessKind,
    executable: &Path,
    parsed_version: Option<&Version>,
) -> (BTreeSet<HarnessCapability>, Vec<String>) {
    if kind == HarnessKind::ClaudeCode {
        let minimum = Version::new(
            CLAUDE_MODEL_PICKER_MIN_VERSION.0,
            CLAUDE_MODEL_PICKER_MIN_VERSION.1,
            CLAUDE_MODEL_PICKER_MIN_VERSION.2,
        );
        return match parsed_version {
            Some(version) if version >= &minimum => (
                BTreeSet::from([HarnessCapability::ClaudeModelPicker]),
                Vec::new(),
            ),
            Some(_) => (
                BTreeSet::new(),
                vec![format!(
                    "Claude Code {minimum} or newer is required to show explicit 1M-context model variants; using the standard model picker."
                )],
            ),
            None => (BTreeSet::new(), Vec::new()),
        };
    }
    if kind != HarnessKind::Codex {
        return (BTreeSet::new(), Vec::new());
    }
    let output = match run_version_command(executable, &["--help"]) {
        Ok(output) => output,
        Err(error) => {
            return (
                BTreeSet::new(),
                vec![format!(
                    "could not inspect Codex configuration-profile support ({error}); using isolated compatibility mode."
                )],
            );
        }
    };
    if !output.status.success() {
        return (
            BTreeSet::new(),
            vec![
                "Codex does not expose configuration-profile support; using isolated compatibility mode."
                    .to_owned(),
            ],
        );
    }
    let help = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if help.contains("--profile") {
        (
            BTreeSet::from([HarnessCapability::CodexConfigProfile]),
            Vec::new(),
        )
    } else {
        (
            BTreeSet::new(),
            vec![
                "Codex does not expose configuration-profile support; using isolated compatibility mode."
                    .to_owned(),
            ],
        )
    }
}

fn run_version_command(executable: &Path, arguments: &[&str]) -> std::io::Result<Output> {
    for attempt in 1..=VERSION_COMMAND_ATTEMPTS {
        match Command::new(executable).args(arguments).output() {
            Err(error)
                if executable_is_temporarily_busy(&error) && attempt < VERSION_COMMAND_ATTEMPTS =>
            {
                std::thread::sleep(VERSION_COMMAND_RETRY_DELAY);
            }
            result => return result,
        }
    }
    unreachable!("the bounded version command loop always returns")
}

fn executable_is_temporarily_busy(error: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        error.raw_os_error() == Some(nix::libc::ETXTBSY)
    }
    #[cfg(not(unix))]
    {
        let _ = error;
        false
    }
}

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("bundled compatibility manifest is invalid: {0}")]
    InvalidManifest(serde_json::Error),
    #[error("bundled compatibility manifest violates its contract: {0}")]
    InvalidManifestContract(String),
    #[error("compatibility manifest has no entry for {0}")]
    MissingCompatibilityEntry(HarnessKind),
    #[error("compatibility manifest has an invalid version command '{command}' for {harness}")]
    InvalidVersionCommand {
        harness: HarnessKind,
        command: String,
    },
    #[error("executable '{0}' was not found")]
    ExecutableNotFound(String),
    #[error("executable path '{}' is not an executable file", .0.display())]
    InvalidExecutable(PathBuf),
    #[error("could not run '{command}': {source}")]
    VersionCommand {
        command: String,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "'{command}' failed{}",
        exit_code.map_or_else(String::new, |code| format!(" with exit code {code}"))
    )]
    VersionCommandFailed {
        command: String,
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
            Self::InvalidManifest(_)
            | Self::InvalidManifestContract(_)
            | Self::MissingCompatibilityEntry(_)
            | Self::InvalidVersionCommand { .. } => "NH-DISCOVERY-001",
            Self::ExecutableNotFound(_) | Self::InvalidExecutable(_) => "NH-DISCOVERY-002",
            Self::VersionCommand { .. } | Self::VersionCommandFailed { .. } => "NH-DISCOVERY-003",
            Self::UnsupportedVersion { .. } => "NH-DISCOVERY-004",
            Self::UnparseableVersion { .. } => "NH-DISCOVERY-005",
        }
    }
}

fn version_arguments(entry: &HarnessCompatibility) -> Result<Vec<&str>, DiscoveryError> {
    let mut parts = entry.command.split_ascii_whitespace();
    let executable = parts.next();
    let arguments = parts.collect::<Vec<_>>();
    if executable != Some(entry.id.binary_name()) || arguments.is_empty() {
        return Err(DiscoveryError::InvalidVersionCommand {
            harness: entry.id,
            command: entry.command.clone(),
        });
    }
    Ok(arguments)
}

fn validate_executable(path: &Path) -> Result<PathBuf, DiscoveryError> {
    if is_executable_file(path) {
        Ok(path.to_path_buf())
    } else {
        Err(DiscoveryError::InvalidExecutable(path.to_path_buf()))
    }
}

#[must_use]
pub fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn find_executable(binary_name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .flat_map(|directory| executable_candidates(&directory, binary_name))
        .find(|candidate| is_executable_file(candidate))
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
            .rsplit_once('/')
            .map_or(token, |(_, version)| version)
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
    parsed_version: Option<&Version>,
    last_compatible_version: &Version,
) -> Vec<String> {
    match status {
        VersionStatus::Tested | VersionStatus::Supported => Vec::new(),
        VersionStatus::NewerUntested => {
            let detected_version =
                parsed_version.map_or_else(|| detected.to_owned(), ToString::to_string);
            vec![format!(
                "The detected {harness} ({detected_version}) is newer than the last version confirmed compatible with this nan-harness release ({last_compatible_version}); continuing with forward-compatible safeguards. {}",
                random_forward_compatibility_quip()
            )]
        }
        VersionStatus::OlderUnsupported => vec![format!(
            "{harness} version '{detected}' is older than the supported minimum."
        )],
        VersionStatus::Unparseable => vec![format!(
            "nan-harness could not parse the {harness} version from '{detected}'."
        )],
    }
}

fn random_forward_compatibility_quip() -> &'static str {
    let mut bytes = [0; size_of::<usize>()];
    if getrandom::fill(&mut bytes).is_err() {
        return FORWARD_COMPATIBILITY_QUIPS[0];
    }
    choose_forward_compatibility_quip(usize::from_ne_bytes(bytes))
}

fn choose_forward_compatibility_quip(random_value: usize) -> &'static str {
    FORWARD_COMPATIBILITY_QUIPS[random_value % FORWARD_COMPATIBILITY_QUIPS.len()]
}

#[cfg(test)]
mod tests {
    use super::{FORWARD_COMPATIBILITY_QUIPS, choose_forward_compatibility_quip};

    #[test]
    fn forward_compatibility_quips_have_the_requested_variety() {
        assert_eq!(FORWARD_COMPATIBILITY_QUIPS.len(), 10);
        assert_eq!(
            choose_forward_compatibility_quip(0),
            FORWARD_COMPATIBILITY_QUIPS[0]
        );
        assert_eq!(
            choose_forward_compatibility_quip(10),
            FORWARD_COMPATIBILITY_QUIPS[0]
        );
    }
}
