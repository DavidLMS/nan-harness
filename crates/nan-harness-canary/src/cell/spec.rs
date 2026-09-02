use super::errors::CellError;
use crate::report::{CanaryTier, CanaryTrigger, FailureClass, RuntimeEvidence, sha256_hex};
use nan_harness_core::HarnessKind;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

const CELL_SCHEMA_VERSION: u8 = 1;
const DEFAULT_BOOT_TIMEOUT_SECONDS: u64 = 180;
const DEFAULT_CLONE_TIMEOUT_SECONDS: u64 = 1_800;
pub(crate) const DEFAULT_STEP_TIMEOUT_SECONDS: u64 = 300;
const DEFAULT_OVERALL_TIMEOUT_SECONDS: u64 = 1_800;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CellSpec {
    pub(crate) schema_version: u8,
    pub(crate) id: String,
    pub(crate) harness: HarnessKind,
    pub(crate) trigger: CanaryTrigger,
    pub(crate) tier: CanaryTier,
    pub(crate) scenario: String,
    pub(crate) image: String,
    pub(crate) guest: GuestOperatingSystem,
    #[serde(default)]
    pub(crate) network: GuestNetwork,
    pub(crate) profile: String,
    pub(crate) nan_harness: NanHarnessArtifact,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default = "default_boot_timeout")]
    pub(crate) boot_timeout_seconds: u64,
    #[serde(default = "default_clone_timeout")]
    pub(crate) clone_timeout_seconds: u64,
    #[serde(default = "default_overall_timeout")]
    pub(crate) overall_timeout_seconds: u64,
    pub(crate) harness_version_file: PathBuf,
    #[serde(default)]
    pub(crate) runtimes: Vec<RuntimeEvidence>,
    #[serde(default)]
    pub(crate) artifacts: Vec<Artifact>,
    pub(crate) steps: Vec<Step>,
}

impl CellSpec {
    pub(crate) fn validate(&self) -> Result<(), CellError> {
        if self.schema_version != CELL_SCHEMA_VERSION {
            return Err(CellError::UnsupportedSpecSchema(self.schema_version));
        }
        for (field, value) in [
            ("id", self.id.as_str()),
            ("scenario", self.scenario.as_str()),
            ("image", self.image.as_str()),
            ("profile", self.profile.as_str()),
            ("nanHarness.version", self.nan_harness.version.as_str()),
            ("nanHarness.source", self.nan_harness.source.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(CellError::EmptySpecField(field));
            }
        }
        semver::Version::parse(&self.nan_harness.version)
            .map_err(|source| CellError::InvalidNanHarnessVersion(source.to_string()))?;
        if self.steps.is_empty() {
            return Err(CellError::MissingSteps);
        }
        if self.overall_timeout_seconds == 0
            || self.clone_timeout_seconds == 0
            || self.boot_timeout_seconds == 0
            || self
                .steps
                .iter()
                .any(|step| step.timeout_seconds == 0 || step.attempts == 0)
        {
            return Err(CellError::InvalidTimeout);
        }
        validate_relative_path(&self.nan_harness.artifact, "nanHarness.artifact")?;
        validate_relative_path(&self.harness_version_file, "harnessVersionFile")?;
        for artifact in &self.artifacts {
            validate_relative_path(&artifact.source, "artifacts.source")?;
            validate_file_name(&artifact.name)?;
        }
        for step in &self.steps {
            if step.name.trim().is_empty() || step.script.trim().is_empty() {
                return Err(CellError::InvalidStep);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum GuestOperatingSystem {
    Linux,
    Macos,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum GuestNetwork {
    #[default]
    Shared,
    Softnet,
}

impl GuestOperatingSystem {
    pub(crate) const fn as_str(&self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Macos => "macos",
        }
    }

    pub(crate) const fn input_path(&self) -> &'static str {
        match self {
            Self::Linux => "/mnt/shared/nan-input",
            Self::Macos => "/Volumes/My Shared Files/nan-input",
        }
    }

    pub(crate) const fn output_path(&self) -> &'static str {
        match self {
            Self::Linux => "/mnt/shared/nan-output",
            Self::Macos => "/Volumes/My Shared Files/nan-output",
        }
    }

    pub(crate) const fn mount_script(&self) -> &'static str {
        match self {
            Self::Linux => {
                "set -euo pipefail\nsudo mkdir -p /mnt/shared\nmountpoint -q /mnt/shared || sudo mount -t virtiofs com.apple.virtio-fs.automount /mnt/shared\ntest -d /mnt/shared/nan-input\ntest -d /mnt/shared/nan-output\n"
            }
            Self::Macos => {
                "set -euo pipefail\ntest -d '/Volumes/My Shared Files/nan-input'\ntest -d '/Volumes/My Shared Files/nan-output'\n"
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NanHarnessArtifact {
    pub(crate) version: String,
    pub(crate) source: String,
    pub(crate) artifact: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Artifact {
    pub(crate) source: PathBuf,
    pub(crate) name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Step {
    pub(crate) name: String,
    pub(crate) script: String,
    pub(crate) failure_class: FailureClass,
    #[serde(default)]
    pub(crate) requires_api_key: bool,
    #[serde(default = "default_step_timeout")]
    pub(crate) timeout_seconds: u64,
    #[serde(default = "default_attempts")]
    pub(crate) attempts: u8,
}

pub(crate) struct LoadedSpec {
    pub(crate) value: CellSpec,
    pub(crate) path: PathBuf,
    pub(crate) sha256: String,
}

impl LoadedSpec {
    pub(crate) fn load(path: &Path) -> Result<Self, CellError> {
        let contents = fs::read(path).map_err(|source| CellError::ReadSpec {
            path: path.to_owned(),
            source,
        })?;
        let value: CellSpec =
            toml::from_slice(&contents).map_err(|source| CellError::ParseSpec {
                path: path.to_owned(),
                source,
            })?;
        value.validate()?;
        Ok(Self {
            value,
            path: path.to_owned(),
            sha256: sha256_hex(&contents),
        })
    }

    pub(crate) fn resolve(&self, path: &Path) -> Result<PathBuf, CellError> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| CellError::InvalidSpecPath(self.path.clone()))?;
        Ok(parent.join(path))
    }
}

fn validate_relative_path(path: &Path, field: &'static str) -> Result<(), CellError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(CellError::UnsafeRelativePath(field));
    }
    Ok(())
}

fn validate_file_name(name: &str) -> Result<(), CellError> {
    if name.is_empty() || Path::new(name).components().count() != 1 || matches!(name, "." | "..") {
        return Err(CellError::InvalidArtifactName(name.to_owned()));
    }
    Ok(())
}

const fn default_boot_timeout() -> u64 {
    DEFAULT_BOOT_TIMEOUT_SECONDS
}

const fn default_clone_timeout() -> u64 {
    DEFAULT_CLONE_TIMEOUT_SECONDS
}

const fn default_step_timeout() -> u64 {
    DEFAULT_STEP_TIMEOUT_SECONDS
}

const fn default_overall_timeout() -> u64 {
    DEFAULT_OVERALL_TIMEOUT_SECONDS
}

const fn default_attempts() -> u8 {
    1
}
