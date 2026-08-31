use nan_harness_core::{DesktopHarnessKind, DesktopTransport};
use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const EMBEDDED_MANIFEST: &str = include_str!("../resources/desktop-compatibility.json");
const SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DesktopCompatibilityEvidence {
    LiveVerified,
    ContractOnly,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopCompatibilityStatus {
    Tested,
    ContractOnly,
    NewerUntested,
    OlderUnsupported,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopCompatibilityEntry {
    pub id: DesktopHarnessKind,
    pub platform: String,
    pub transport: DesktopTransport,
    pub evidence: DesktopCompatibilityEvidence,
    pub minimum_app_version: Option<Version>,
    pub last_compatible_app_version: Option<Version>,
    pub minimum_runtime_version: Option<Version>,
    pub last_compatible_runtime_version: Option<Version>,
    pub compatible_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopCompatibilityReport {
    pub status: DesktopCompatibilityStatus,
    pub minimum_app_version: Version,
    pub last_compatible_app_version: Version,
    pub minimum_bundled_codex_version: Version,
    pub last_compatible_bundled_codex_version: Version,
    pub compatible_at: String,
}

#[derive(Debug, Error)]
pub enum DesktopCompatibilityError {
    #[error("the embedded desktop compatibility registry is invalid: {0}")]
    InvalidRegistry(serde_json::Error),
    #[error("the embedded desktop compatibility registry uses unsupported schema version {0}")]
    UnsupportedSchema(u8),
    #[error("no Desktop compatibility record exists for this harness and platform")]
    MissingPlatform,
    #[error("the embedded desktop compatibility registry contains an invalid version")]
    InvalidVersion(semver::Error),
    #[error("the requested Desktop surface is unavailable on this platform")]
    Unavailable,
    #[error("the Desktop compatibility record does not contain runtime version evidence")]
    MissingRuntimeEvidence,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Registry {
    schema_version: u8,
    surfaces: Vec<RawEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawEntry {
    id: DesktopHarnessKind,
    platform: String,
    transport: DesktopTransport,
    evidence: DesktopCompatibilityEvidence,
    #[serde(default)]
    minimum_app_version: Option<String>,
    #[serde(default)]
    last_compatible_app_version: Option<String>,
    #[serde(default)]
    minimum_runtime_version: Option<String>,
    #[serde(default)]
    last_compatible_runtime_version: Option<String>,
    compatible_at: String,
}

/// Returns the local, non-refreshable compatibility record for a Desktop surface.
///
/// # Errors
///
/// Fails closed when the embedded registry is malformed or has no matching row.
pub fn desktop_compatibility(
    kind: DesktopHarnessKind,
) -> Result<DesktopCompatibilityEntry, DesktopCompatibilityError> {
    let registry: Registry = serde_json::from_str(EMBEDDED_MANIFEST)
        .map_err(DesktopCompatibilityError::InvalidRegistry)?;
    if registry.schema_version != SCHEMA_VERSION {
        return Err(DesktopCompatibilityError::UnsupportedSchema(
            registry.schema_version,
        ));
    }
    let platform = desktop_platform();
    let entry = registry
        .surfaces
        .into_iter()
        .find(|entry| entry.id == kind && entry.platform == platform)
        .ok_or(DesktopCompatibilityError::MissingPlatform)?;
    Ok(DesktopCompatibilityEntry {
        id: entry.id,
        platform: entry.platform,
        transport: entry.transport,
        evidence: entry.evidence,
        minimum_app_version: parse_optional(entry.minimum_app_version)?,
        last_compatible_app_version: parse_optional(entry.last_compatible_app_version)?,
        minimum_runtime_version: parse_optional(entry.minimum_runtime_version)?,
        last_compatible_runtime_version: parse_optional(entry.last_compatible_runtime_version)?,
        compatible_at: entry.compatible_at,
    })
}

#[must_use]
pub fn classify_desktop_version(
    entry: &DesktopCompatibilityEntry,
    installed: Option<&Version>,
) -> DesktopCompatibilityStatus {
    if entry.evidence == DesktopCompatibilityEvidence::Unavailable {
        return DesktopCompatibilityStatus::Unavailable;
    }
    if let (Some(installed), Some(minimum)) = (installed, entry.minimum_app_version.as_ref())
        && installed < minimum
    {
        return DesktopCompatibilityStatus::OlderUnsupported;
    }
    if entry.evidence == DesktopCompatibilityEvidence::ContractOnly {
        return DesktopCompatibilityStatus::ContractOnly;
    }
    if let (Some(installed), Some(last)) = (installed, entry.last_compatible_app_version.as_ref())
        && installed > last
    {
        return DesktopCompatibilityStatus::NewerUntested;
    }
    DesktopCompatibilityStatus::Tested
}

/// Backwards-compatible evaluator used by the `ChatGPT` Desktop launcher.
///
/// # Errors
///
/// Returns an error when this platform is unavailable or lacks bundled runtime evidence.
pub fn evaluate_desktop_compatibility(
    app_version: &Version,
    bundled_codex_version: &Version,
) -> Result<DesktopCompatibilityReport, DesktopCompatibilityError> {
    let entry = desktop_compatibility(DesktopHarnessKind::ChatGpt)?;
    if entry.evidence == DesktopCompatibilityEvidence::Unavailable {
        return Err(DesktopCompatibilityError::Unavailable);
    }
    let minimum_app_version = entry
        .minimum_app_version
        .ok_or(DesktopCompatibilityError::MissingRuntimeEvidence)?;
    let last_compatible_app_version = entry
        .last_compatible_app_version
        .ok_or(DesktopCompatibilityError::MissingRuntimeEvidence)?;
    let minimum_bundled_codex_version = entry
        .minimum_runtime_version
        .ok_or(DesktopCompatibilityError::MissingRuntimeEvidence)?;
    let last_compatible_bundled_codex_version = entry
        .last_compatible_runtime_version
        .ok_or(DesktopCompatibilityError::MissingRuntimeEvidence)?;
    let status = if app_version < &minimum_app_version
        || bundled_codex_version < &minimum_bundled_codex_version
    {
        DesktopCompatibilityStatus::OlderUnsupported
    } else if entry.evidence == DesktopCompatibilityEvidence::ContractOnly {
        DesktopCompatibilityStatus::ContractOnly
    } else if app_version > &last_compatible_app_version
        || bundled_codex_version > &last_compatible_bundled_codex_version
    {
        DesktopCompatibilityStatus::NewerUntested
    } else {
        DesktopCompatibilityStatus::Tested
    };
    Ok(DesktopCompatibilityReport {
        status,
        minimum_app_version,
        last_compatible_app_version,
        minimum_bundled_codex_version,
        last_compatible_bundled_codex_version,
        compatible_at: entry.compatible_at,
    })
}

fn parse_optional(value: Option<String>) -> Result<Option<Version>, DesktopCompatibilityError> {
    value
        .map(|value| Version::parse(&value).map_err(DesktopCompatibilityError::InvalidVersion))
        .transpose()
}

#[must_use]
pub const fn desktop_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DesktopCompatibilityEvidence, DesktopCompatibilityStatus, classify_desktop_version,
        desktop_compatibility,
    };
    use nan_harness_core::DesktopHarnessKind;
    use semver::Version;

    #[test]
    fn every_desktop_surface_has_a_platform_record() {
        for kind in DesktopHarnessKind::ALL {
            desktop_compatibility(kind).expect("current platform record should exist");
        }
    }

    #[test]
    fn contract_only_evidence_warns_without_becoming_an_error() {
        let mut entry = desktop_compatibility(DesktopHarnessKind::Claude)
            .expect("Claude platform record should exist");
        entry.evidence = DesktopCompatibilityEvidence::ContractOnly;
        assert_eq!(
            classify_desktop_version(&entry, Some(&Version::new(999, 0, 0))),
            DesktopCompatibilityStatus::ContractOnly
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn chatgpt_is_explicitly_unavailable_on_linux() {
        let entry = desktop_compatibility(DesktopHarnessKind::ChatGpt)
            .expect("Linux unavailable row should exist");
        assert_eq!(entry.evidence, DesktopCompatibilityEvidence::Unavailable);
    }
}
