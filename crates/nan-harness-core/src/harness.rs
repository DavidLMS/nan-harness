use semver::Version;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
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
}

impl HarnessKind {
    #[must_use]
    pub const fn binary_name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Hermes => "hermes",
            Self::Pi => "pi",
            Self::PrimeAgent => "prime-agent",
            Self::DeepSeekHarness => "dsh",
            Self::OpenClaw => "openclaw",
            Self::Cline => "cline",
            Self::QwenCode => "qwen",
            Self::KimiCode => "kimi",
            Self::Aider => "aider",
            Self::Goose => "goose",
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
            Self::PrimeAgent => "prime-agent",
            Self::DeepSeekHarness => "deepseek-harness",
            Self::OpenClaw => "openclaw",
            Self::Cline => "cline",
            Self::QwenCode => "qwen-code",
            Self::KimiCode => "kimi-code",
            Self::Aider => "aider",
            Self::Goose => "goose",
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
            "prime-agent" | "prime" => Ok(Self::PrimeAgent),
            "deepseek-harness" | "dsh" => Ok(Self::DeepSeekHarness),
            "openclaw" | "claw" => Ok(Self::OpenClaw),
            "cline" => Ok(Self::Cline),
            "qwen-code" | "qwen" => Ok(Self::QwenCode),
            "kimi-code" | "kimi" => Ok(Self::KimiCode),
            "aider" => Ok(Self::Aider),
            "goose" => Ok(Self::Goose),
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedHarness {
    pub kind: HarnessKind,
    pub executable: String,
    pub detected_version: String,
    pub version_status: VersionStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompatibilityTransport {
    DirectChat,
    AnthropicBridge,
    ResponsesBridge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompatibilityStatus {
    Verified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessCompatibility {
    pub id: HarnessKind,
    pub command: String,
    pub last_verified_version: Version,
    pub minimum_version: Version,
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
pub struct CompatibilityPolicy {
    pub newer: NewerVersionPolicy,
    pub older: OlderVersionPolicy,
    pub unparseable: UnparseableVersionPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityManifest {
    pub schema_version: u8,
    pub tested_at: String,
    pub policy: CompatibilityPolicy,
    pub harnesses: Vec<HarnessCompatibility>,
}

impl CompatibilityManifest {
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

        match detected.cmp(&entry.last_verified_version) {
            Ordering::Less => Some(VersionStatus::Supported),
            Ordering::Equal => Some(VersionStatus::Tested),
            Ordering::Greater => Some(VersionStatus::NewerUntested),
        }
    }
}
