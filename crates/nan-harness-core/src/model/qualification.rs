use crate::HarnessKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualificationStatus {
    Qualified,
    Unqualified,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualificationTransport {
    DirectChat,
    AnthropicBridge,
    ResponsesBridge,
    FxGatewayBridge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelQualification {
    pub status: QualificationStatus,
    pub transport: QualificationTransport,
    pub tested_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualificationMatrix {
    #[serde(rename = "claude-code")]
    pub claude_code: ModelQualification,
    pub codex: ModelQualification,
    pub opencode: ModelQualification,
    pub hermes: ModelQualification,
    pub pi: ModelQualification,
    pub omp: ModelQualification,
    #[serde(rename = "prime-agent")]
    pub prime_agent: ModelQualification,
    #[serde(rename = "deepseek-harness")]
    pub deepseek_harness: ModelQualification,
    #[serde(rename = "openclaw")]
    pub openclaw: ModelQualification,
    pub cline: ModelQualification,
    #[serde(rename = "qwen-code")]
    pub qwen_code: ModelQualification,
    #[serde(rename = "kimi-code")]
    pub kimi_code: ModelQualification,
    pub aider: ModelQualification,
    pub goose: ModelQualification,
    pub fx: ModelQualification,
}

impl QualificationMatrix {
    #[must_use]
    pub const fn for_harness(&self, harness: HarnessKind) -> &ModelQualification {
        match harness {
            HarnessKind::ClaudeCode => &self.claude_code,
            HarnessKind::Codex => &self.codex,
            HarnessKind::OpenCode => &self.opencode,
            HarnessKind::Hermes => &self.hermes,
            HarnessKind::Pi => &self.pi,
            HarnessKind::Omp => &self.omp,
            HarnessKind::PrimeAgent => &self.prime_agent,
            HarnessKind::DeepSeekHarness => &self.deepseek_harness,
            HarnessKind::OpenClaw => &self.openclaw,
            HarnessKind::Cline => &self.cline,
            HarnessKind::QwenCode => &self.qwen_code,
            HarnessKind::KimiCode => &self.kimi_code,
            HarnessKind::Aider => &self.aider,
            HarnessKind::Goose => &self.goose,
            HarnessKind::Fx => &self.fx,
        }
    }
}
