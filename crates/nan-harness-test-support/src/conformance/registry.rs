use super::constants::{TEST_CREDENTIAL, WRAPPER_TIMEOUT};
use super::prime_cleanup::prime_status_path;
use crate::manifest::{
    ConformanceManifest, Coverage, embedded_manifest, embedded_manifest_sources,
    embedded_tool_scenario,
};
use crate::terminal::TerminalCommand;
use nan_harness_core::HarnessKind;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessRegistration {
    pub kind: HarnessKind,
}

impl HarnessRegistration {
    #[must_use]
    pub const fn binary_name(self) -> &'static str {
        self.kind.binary_name()
    }

    /// Parses this registration's compile-time embedded manifest.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError`] if the embedded source is malformed.
    pub fn manifest(self) -> Result<ConformanceManifest, RegistryError> {
        embedded_manifest(self.kind).map_err(|error| RegistryError::Manifest {
            kind: self.kind,
            message: error.to_string(),
        })
    }
}

const fn registration(kind: HarnessKind) -> HarnessRegistration {
    HarnessRegistration { kind }
}

const REGISTRY: [HarnessRegistration; HarnessKind::ALL.len()] = [
    registration(HarnessKind::ALL[0]),
    registration(HarnessKind::ALL[1]),
    registration(HarnessKind::ALL[2]),
    registration(HarnessKind::ALL[3]),
    registration(HarnessKind::ALL[4]),
    registration(HarnessKind::ALL[5]),
    registration(HarnessKind::ALL[6]),
    registration(HarnessKind::ALL[7]),
    registration(HarnessKind::ALL[8]),
    registration(HarnessKind::ALL[9]),
    registration(HarnessKind::ALL[10]),
    registration(HarnessKind::ALL[11]),
    registration(HarnessKind::ALL[12]),
    registration(HarnessKind::ALL[13]),
    registration(HarnessKind::ALL[14]),
];

#[must_use]
pub fn harness_registry() -> &'static [HarnessRegistration] {
    &REGISTRY
}

#[must_use]
pub fn harness_registration(kind: HarnessKind) -> Option<&'static HarnessRegistration> {
    REGISTRY
        .iter()
        .find(|registration| registration.kind == kind)
}

/// Validates the exact one-to-one relationship between canonical harness identities, embedded
/// manifests, and canonical binary names.
///
/// # Errors
///
/// Returns [`RegistryError`] when a manifest is missing, duplicated, malformed, stale, or has no
/// tool/protocol contract.
pub fn validate_harness_registry() -> Result<(), RegistryError> {
    validate_registry_counts()?;
    let mut kinds = BTreeSet::new();
    for registration in REGISTRY {
        validate_registration(&mut kinds, registration)?;
    }
    for kind in HarnessKind::ALL {
        validate_manifest_source(&kinds, kind)?;
    }
    Ok(())
}

fn validate_registry_counts() -> Result<(), RegistryError> {
    if REGISTRY.len() != HarnessKind::ALL.len() {
        return Err(RegistryError::Count {
            expected: HarnessKind::ALL.len(),
            actual: REGISTRY.len(),
        });
    }
    let source_count = embedded_manifest_sources().len();
    if source_count != HarnessKind::ALL.len() {
        return Err(RegistryError::ManifestCount {
            expected: HarnessKind::ALL.len(),
            actual: source_count,
        });
    }
    Ok(())
}

fn validate_registration(
    kinds: &mut BTreeSet<HarnessKind>,
    registration: HarnessRegistration,
) -> Result<(), RegistryError> {
    if !kinds.insert(registration.kind) {
        return Err(RegistryError::Duplicate(registration.kind));
    }
    if registration.binary_name() != registration.kind.binary_name() {
        return Err(RegistryError::BinaryMapping {
            kind: registration.kind,
            actual: registration.binary_name().to_owned(),
            expected: registration.kind.binary_name().to_owned(),
        });
    }
    let manifest = registration.manifest()?;
    validate_manifest(registration, &manifest)
}

fn validate_manifest(
    registration: HarnessRegistration,
    manifest: &ConformanceManifest,
) -> Result<(), RegistryError> {
    if manifest.harness != registration.kind {
        return Err(RegistryError::ManifestIdentity {
            kind: registration.kind,
            manifest: manifest.harness,
        });
    }
    if manifest.tool_names().is_empty() {
        return Err(RegistryError::EmptyInventory(registration.kind));
    }
    let external = manifest
        .tools
        .iter()
        .filter(|entry| entry.coverage == Coverage::ExternalAuthentication)
        .count();
    if registration.kind == HarnessKind::ClaudeCode {
        return validate_claude_manifest(registration.kind, manifest, external);
    }
    if external != 0 {
        return Err(RegistryError::ScenarioContract {
            kind: registration.kind,
            message: "only Claude may declare an external-authentication scenario".into(),
        });
    }
    Ok(())
}

fn validate_claude_manifest(
    kind: HarnessKind,
    manifest: &ConformanceManifest,
    external: usize,
) -> Result<(), RegistryError> {
    if external != 1 {
        return Err(RegistryError::ScenarioContract {
            kind,
            message: "Claude must declare exactly one external-authentication scenario".into(),
        });
    }
    let Some(external_entry) = manifest
        .tools
        .iter()
        .find(|entry| entry.coverage == Coverage::ExternalAuthentication)
    else {
        return Err(RegistryError::ScenarioContract {
            kind,
            message: "Claude DesignSync scenario must be embedded".into(),
        });
    };
    if external_entry.name != "DesignSync"
        || embedded_tool_scenario(kind, &external_entry.scenario).is_err()
    {
        return Err(RegistryError::ScenarioContract {
            kind,
            message: "Claude DesignSync scenario must be embedded".into(),
        });
    }
    Ok(())
}

fn validate_manifest_source(
    kinds: &BTreeSet<HarnessKind>,
    kind: HarnessKind,
) -> Result<(), RegistryError> {
    if !kinds.contains(&kind) {
        return Err(RegistryError::Missing(kind));
    }
    let source_count = embedded_manifest_sources()
        .iter()
        .filter(|(source_kind, _)| *source_kind == kind)
        .count();
    if source_count != 1 {
        return Err(RegistryError::ManifestIdentityCount {
            kind,
            actual: source_count,
        });
    }
    Ok(())
}

/// Builds a clean command prefix for a conformance test process.
#[must_use]
pub fn conformance_command(
    nan_harness: impl Into<PathBuf>,
    harness: HarnessKind,
    workspace: impl AsRef<Path>,
    provider_base_url: &str,
) -> TerminalCommand {
    TerminalCommand::new(nan_harness, workspace.as_ref())
        .clear_environment()
        .args([
            OsString::from(harness.binary_name()),
            OsString::from("--provider-base-url"),
            OsString::from(provider_base_url),
            OsString::from("--"),
        ])
        .env("CI", "1")
        .env(
            "PATH",
            if harness == HarnessKind::PrimeAgent {
                prime_status_path()
            } else {
                std::env::var_os("PATH").unwrap_or_default()
            },
        )
        .env("NAN_API_KEY", TEST_CREDENTIAL)
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env("NAN_NO_UPDATE_CHECK", "1")
        .env(
            "NAN_HARNESS_CONFIG_DIR",
            workspace.as_ref().join("nan-config"),
        )
        .env("HOME", workspace.as_ref().join("home"))
        .timeout(WRAPPER_TIMEOUT)
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("harness registry contains {actual} entries; expected {expected}")]
    Count { expected: usize, actual: usize },
    #[error("embedded conformance source contains {actual} manifests; expected {expected}")]
    ManifestCount { expected: usize, actual: usize },
    #[error("harness registry contains duplicate {0}")]
    Duplicate(HarnessKind),
    #[error("harness registry is missing {0}")]
    Missing(HarnessKind),
    #[error("harness registry has no inventory for {0}")]
    EmptyInventory(HarnessKind),
    #[error("harness registry maps {kind} to binary '{actual}', expected '{expected}'")]
    BinaryMapping {
        kind: HarnessKind,
        actual: String,
        expected: String,
    },
    #[error("manifest for {kind} identifies itself as {manifest}")]
    ManifestIdentity {
        kind: HarnessKind,
        manifest: HarnessKind,
    },
    #[error("manifest for {kind} appears {actual} times in embedded sources")]
    ManifestIdentityCount { kind: HarnessKind, actual: usize },
    #[error("could not load manifest for {kind}: {message}")]
    Manifest { kind: HarnessKind, message: String },
    #[error("invalid scenario contract for {kind}: {message}")]
    ScenarioContract { kind: HarnessKind, message: String },
}
