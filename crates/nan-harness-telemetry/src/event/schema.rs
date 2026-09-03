use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HarnessIdentity {
    kind: HarnessKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compatibility: Option<CompatibilityStatus>,
}

impl HarnessIdentity {
    #[must_use]
    pub fn new(kind: HarnessKind, version: Option<String>) -> Self {
        Self {
            kind,
            version,
            compatibility: None,
        }
    }

    #[must_use]
    pub const fn with_compatibility(mut self, compatibility: CompatibilityStatus) -> Self {
        self.compatibility = Some(compatibility);
        self
    }

    #[must_use]
    pub fn kind(&self) -> HarnessKind {
        self.kind
    }

    #[must_use]
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    #[must_use]
    pub fn compatibility(&self) -> Option<CompatibilityStatus> {
        self.compatibility
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CompatibilityStatus {
    Tested,
    Supported,
    NewerUntested,
    OlderUnsupported,
    Unparseable,
}

impl CompatibilityStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tested => "tested",
            Self::Supported => "supported",
            Self::NewerUntested => "newer-untested",
            Self::OlderUnsupported => "older-unsupported",
            Self::Unparseable => "unparseable",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessKind {
    ClaudeCode,
    ChatGptDesktop,
    ClaudeDesktop,
    Codex,
    #[serde(rename = "opencode")]
    OpenCode,
    Hermes,
    HermesDesktop,
    PenDesktop,
    ZedDesktop,
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
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::ChatGptDesktop => "chatgpt-desktop",
            Self::ClaudeDesktop => "claude-desktop",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Hermes => "hermes",
            Self::HermesDesktop => "hermes-desktop",
            Self::PenDesktop => "pen-desktop",
            Self::ZedDesktop => "zed-desktop",
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
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Transport {
    DirectChat,
    AnthropicBridge,
    ResponsesBridge,
    FxGatewayBridge,
}

impl Transport {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DirectChat => "direct-chat",
            Self::AnthropicBridge => "anthropic-bridge",
            Self::ResponsesBridge => "responses-bridge",
            Self::FxGatewayBridge => "fx-gateway-bridge",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationContext {
    kind: OperationKind,
}

impl OperationContext {
    #[must_use]
    pub const fn new(kind: OperationKind) -> Self {
        Self { kind }
    }

    #[must_use]
    pub fn kind(&self) -> OperationKind {
        self.kind
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OperationKind {
    HarnessRun,
    HarnessDryRun,
    HarnessConfig,
    HarnessConfigRemove,
    Doctor,
    Update,
    Uninstall,
    TelemetryConfiguration,
}

impl OperationKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HarnessRun => "harness-run",
            Self::HarnessDryRun => "harness-dry-run",
            Self::HarnessConfig => "harness-config",
            Self::HarnessConfigRemove => "harness-config-remove",
            Self::Doctor => "doctor",
            Self::Update => "update",
            Self::Uninstall => "uninstall",
            Self::TelemetryConfiguration => "telemetry-configuration",
        }
    }
}
