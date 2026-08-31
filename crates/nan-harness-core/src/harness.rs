use semver::Version;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessKind {
    ClaudeCode,
    Codex,
    #[serde(rename = "opencode")]
    OpenCode,
    Hermes,
    Pi,
    Omp,
    PrimeAgent,
    #[serde(rename = "deepseek-harness")]
    DeepSeekHarness,
    #[serde(rename = "openclaw")]
    OpenClaw,
    Cline,
    QwenCode,
    KimiCode,
    Aider,
    Goose,
    Fx,
}

impl HarnessKind {
    pub const ALL: [Self; 15] = [
        Self::ClaudeCode,
        Self::Codex,
        Self::OpenCode,
        Self::Hermes,
        Self::Pi,
        Self::Omp,
        Self::PrimeAgent,
        Self::DeepSeekHarness,
        Self::OpenClaw,
        Self::Cline,
        Self::QwenCode,
        Self::KimiCode,
        Self::Aider,
        Self::Goose,
        Self::Fx,
    ];

    #[must_use]
    pub const fn binary_name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Hermes => "hermes",
            Self::Pi => "pi",
            Self::Omp => "omp",
            Self::PrimeAgent => "prime-agent",
            Self::DeepSeekHarness => "dsh",
            Self::OpenClaw => "openclaw",
            Self::Cline => "cline",
            Self::QwenCode => "qwen",
            Self::KimiCode => "kimi",
            Self::Aider => "aider",
            Self::Goose => "goose",
            Self::Fx => "fx",
        }
    }
}

impl fmt::Display for HarnessKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Hermes => "hermes",
            Self::Pi => "pi",
            Self::Omp => "omp",
            Self::PrimeAgent => "prime-agent",
            Self::DeepSeekHarness => "deepseek-harness",
            Self::OpenClaw => "openclaw",
            Self::Cline => "cline",
            Self::QwenCode => "qwen-code",
            Self::KimiCode => "kimi-code",
            Self::Aider => "aider",
            Self::Goose => "goose",
            Self::Fx => "fx",
        };
        formatter.write_str(value)
    }
}

impl FromStr for HarnessKind {
    type Err = ParseHarnessKindError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "claude-code" | "claude" => Ok(Self::ClaudeCode),
            "codex" => Ok(Self::Codex),
            "opencode" => Ok(Self::OpenCode),
            "hermes" => Ok(Self::Hermes),
            "pi" => Ok(Self::Pi),
            "omp" | "oh-my-pi" => Ok(Self::Omp),
            "prime-agent" | "prime" => Ok(Self::PrimeAgent),
            "deepseek-harness" | "deepseek" | "dsh" => Ok(Self::DeepSeekHarness),
            "openclaw" | "claw" => Ok(Self::OpenClaw),
            "cline" => Ok(Self::Cline),
            "qwen-code" | "qwen" => Ok(Self::QwenCode),
            "kimi-code" | "kimi" => Ok(Self::KimiCode),
            "aider" => Ok(Self::Aider),
            "goose" => Ok(Self::Goose),
            "fx" => Ok(Self::Fx),
            _ => Err(ParseHarnessKindError(value.to_owned())),
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("unknown harness '{0}'")]
pub struct ParseHarnessKindError(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VersionStatus {
    Tested,
    Supported,
    NewerUntested,
    OlderUnsupported,
    Unparseable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessCapability {
    CodexConfigProfile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedHarness {
    pub kind: HarnessKind,
    pub executable: String,
    pub detected_version: String,
    pub version_status: VersionStatus,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub capabilities: BTreeSet<HarnessCapability>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompatibilityTransport {
    DirectChat,
    AnthropicBridge,
    ResponsesBridge,
    FxGatewayBridge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompatibilityStatus {
    Verified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeCompatibility {
    pub command: String,
    pub minimum_version: Version,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HarnessCompatibility {
    pub id: HarnessKind,
    pub command: String,
    pub last_compatible_version: Version,
    pub compatible_at: String,
    #[serde(default)]
    pub last_live_verified_version: Option<Version>,
    #[serde(default)]
    pub live_verified_at: Option<String>,
    pub minimum_version: Version,
    #[serde(default)]
    pub runtime: Option<RuntimeCompatibility>,
    pub transport: CompatibilityTransport,
    pub status: CompatibilityStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NewerVersionPolicy {
    AllowWithWarning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OlderVersionPolicy {
    RequireAllowUnsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnparseableVersionPolicy {
    ConfirmOrRequireAllowUntested,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityPolicy {
    pub newer: NewerVersionPolicy,
    pub older: OlderVersionPolicy,
    pub unparseable: UnparseableVersionPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompatibilityManifest {
    pub schema_version: u8,
    pub tested_at: String,
    pub policy: CompatibilityPolicy,
    pub harnesses: Vec<HarnessCompatibility>,
}

impl CompatibilityManifest {
    pub const SCHEMA_VERSION: u8 = 3;

    #[must_use]
    pub fn entry(&self, kind: HarnessKind) -> Option<&HarnessCompatibility> {
        self.harnesses.iter().find(|entry| entry.id == kind)
    }

    #[must_use]
    pub fn classify(&self, kind: HarnessKind, detected: &Version) -> Option<VersionStatus> {
        let entry = self.entry(kind)?;
        if detected < &entry.minimum_version {
            return Some(VersionStatus::OlderUnsupported);
        }

        match detected.cmp(&entry.last_compatible_version) {
            Ordering::Less => Some(VersionStatus::Supported),
            Ordering::Equal => Some(VersionStatus::Tested),
            Ordering::Greater => Some(VersionStatus::NewerUntested),
        }
    }
}
