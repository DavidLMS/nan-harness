mod executable;
mod manifest;
mod probe;
mod version_policy;
mod warnings;
use nan_harness_i18n::{Locale, TerminalMessage};
pub use warnings::DiscoveryWarning;

use nan_harness_core::{DetectedHarness, HarnessCapability, HarnessKind, VersionStatus};
use semver::Version;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub use executable::{is_executable_file, locate_harness_executable};
pub use manifest::bundled_compatibility_manifest;

const CLAUDE_MODEL_PICKER_MIN_VERSION: (u64, u64, u64) = (2, 1, 243);

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
    pub warning_details: Vec<DiscoveryWarning>,
}

impl DiscoveryReport {
    /// Render advisory warnings without modifying the canonical machine report.
    #[must_use]
    pub fn terminal_warnings(&self, locale: Locale) -> Vec<String> {
        self.warning_details
            .iter()
            .map(|warning| warning.terminal_message(locale))
            .collect()
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
    let version_arguments = executable::version_arguments(entry)?;

    let version_command = format!("{} {}", executable.display(), version_arguments.join(" "));
    let output =
        executable::run_command(executable, &version_arguments).map_err(|error| match error {
            probe::ProbeError::Timeout => DiscoveryError::VersionProbeTimeout,
            probe::ProbeError::OutputLimit => DiscoveryError::VersionProbeOutputLimit,
            probe::ProbeError::Io(source) => DiscoveryError::VersionCommand {
                command: version_command.clone(),
                source,
            },
        })?;
    if !output.status.success() {
        return Err(DiscoveryError::VersionCommandFailed {
            command: version_command,
            exit_code: output.status.code(),
        });
    }

    let detected_version = executable::first_non_empty_line(&output.stdout, &output.stderr);
    let parsed_version = version_policy::parse_version(&detected_version);
    let version_status = match parsed_version.as_ref() {
        Some(version) => manifest
            .classify(kind, version)
            .ok_or(DiscoveryError::MissingCompatibilityEntry(kind))?,
        None => VersionStatus::Unparseable,
    };

    version_policy::enforce(kind, version_status, &detected_version, options)?;
    let mut warnings = version_policy::warnings(
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
        warnings: warnings
            .iter()
            .map(|warning| warning.terminal_message(Locale::En))
            .collect(),
        warning_details: warnings,
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
) -> (BTreeSet<HarnessCapability>, Vec<DiscoveryWarning>) {
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
            Some(_) | None => (BTreeSet::new(), Vec::new()),
        };
    }
    if kind != HarnessKind::Codex {
        return (BTreeSet::new(), Vec::new());
    }
    let output = match executable::run_command(executable, &["--help"]) {
        Ok(output) => output,
        Err(error) => {
            return (
                BTreeSet::new(),
                vec![match error {
                    probe::ProbeError::Timeout => DiscoveryWarning::ProfileProbeTimeout,
                    probe::ProbeError::OutputLimit => DiscoveryWarning::ProfileProbeOutputLimit,
                    probe::ProbeError::Io(source) => {
                        DiscoveryWarning::ProfileProbeIo(source.to_string())
                    }
                }],
            );
        }
    };
    if !output.status.success() {
        return (BTreeSet::new(), vec![DiscoveryWarning::ProfileUnavailable]);
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
        (BTreeSet::new(), vec![DiscoveryWarning::ProfileUnavailable])
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
        "harness version probe exceeded 30 seconds; check the executable installation or select a working executable with --executable"
    )]
    VersionProbeTimeout,
    #[error(
        "harness version probe exceeded 1 MiB of combined stdout/stderr; check the executable installation or select a working executable with --executable"
    )]
    VersionProbeOutputLimit,
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
            Self::VersionProbeTimeout => "NH-DISCOVERY-006",
            Self::VersionProbeOutputLimit => "NH-DISCOVERY-007",
        }
    }
}

// Terminal localization is separate from canonical Display used by machine contracts.
impl TerminalMessage for DiscoveryError {
    fn terminal_message(&self, locale: Locale) -> String {
        use nan_harness_i18n::messages as m;
        if locale == Locale::En {
            return self.to_string();
        }
        match self {
            Self::InvalidManifest(field_0) => {
                m::error_discovery_invalid_manifest(locale, &(field_0))
            }
            Self::InvalidManifestContract(field_0) => {
                m::error_discovery_invalid_manifest_contract(locale, &(field_0))
            }
            Self::MissingCompatibilityEntry(field_0) => {
                m::error_discovery_missing_compatibility_entry(locale, &(field_0))
            }
            Self::InvalidVersionCommand { harness, command } => {
                m::error_discovery_invalid_version_command(locale, &(command), &(harness))
            }
            Self::ExecutableNotFound(field_0) => {
                m::error_discovery_executable_not_found(locale, &(field_0))
            }
            Self::InvalidExecutable(field_0) => {
                m::error_discovery_invalid_executable(locale, &(field_0.display()))
            }
            Self::VersionCommand { command, source } => {
                m::error_discovery_version_command(locale, &(command), &(source))
            }
            Self::VersionCommandFailed { command, exit_code } => {
                m::error_discovery_version_command_failed(
                    locale,
                    &(command),
                    &(exit_code.map_or_else(String::new, |code| m::error_exit_code(locale, &code))),
                )
            }
            Self::VersionProbeTimeout => m::error_discovery_version_probe_timeout(locale),
            Self::VersionProbeOutputLimit => m::error_discovery_version_probe_output_limit(locale),
            Self::UnsupportedVersion { harness, detected } => {
                m::error_discovery_unsupported_version(locale, &(detected), &(harness))
            }
            Self::UnparseableVersion { harness, detected } => {
                m::error_discovery_unparseable_version(locale, &(detected), &(harness))
            }
        }
    }
}
