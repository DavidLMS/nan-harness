//! Exact hosted evidence; a live result certifies only its named model.

use semver::Version;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HostedSuite {
    Cli,
    Desktop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HostedOutcome {
    Passed,
    Failed,
    Blocked,
}

/// One validated observation from a trusted runner, never raw harness output.
/// Multiple versions and models remain independent; absence is not a success.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedCheck {
    pub suite: HostedSuite,
    pub id: String,
    pub platform: String,
    pub architecture: String,
    pub harness_version: Version,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_version: Option<Version>,
    /// None is deterministic evidence. Some is a live check with deterministic prerequisites.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub checked_at: String,
    pub outcome: HostedOutcome,
    pub nan_harness_sha256: String,
    pub spec_sha256: String,
    pub evidence_sha256: String,
    pub source_run: u64,
}

impl HostedCheck {
    /// Checks the closed identity fields before timestamps or registry bounds are examined.
    ///
    /// # Errors
    /// Returns a fixed diagnostic when a field cannot describe hosted evidence.
    pub fn validate_identity(&self) -> Result<(), &'static str> {
        let known = match self.suite {
            HostedSuite::Cli => self.id.parse::<crate::HarnessKind>().is_ok(),
            HostedSuite::Desktop => self.id.parse::<crate::DesktopHarnessKind>().is_ok(),
        };
        if !known || (self.suite == HostedSuite::Cli && self.runtime_version.is_some()) {
            return Err("unknown harness or unexpected bundled runtime");
        }
        if !matches!(self.platform.as_str(), "linux" | "macos" | "windows")
            || !matches!(self.architecture.as_str(), "aarch64" | "x86_64")
        {
            return Err("unknown platform or architecture");
        }
        if self.model.as_ref().is_some_and(|model| {
            model.is_empty()
                || model.len() > 128
                || !model.as_bytes()[0].is_ascii_alphanumeric()
                || !model
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte))
        }) {
            return Err("invalid live model identifier");
        }
        if self.source_run == 0
            || [
                &self.nan_harness_sha256,
                &self.spec_sha256,
                &self.evidence_sha256,
            ]
            .iter()
            .any(|digest| {
                digest.len() != 64
                    || !digest
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
        {
            return Err("invalid evidence identity");
        }
        Ok(())
    }

    /// Every identity dimension must agree before an observation can replace another.
    #[must_use]
    pub fn same_target(&self, other: &Self) -> bool {
        self.suite == other.suite
            && self.id == other.id
            && self.platform == other.platform
            && self.architecture == other.architecture
            && self.harness_version == other.harness_version
            && self.runtime_version == other.runtime_version
            && self.model == other.model
            && self.nan_harness_sha256 == other.nan_harness_sha256
            && self.spec_sha256 == other.spec_sha256
    }
}
