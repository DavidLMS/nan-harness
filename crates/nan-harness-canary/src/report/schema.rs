use clap::ValueEnum;
use nan_harness_core::HarnessKind;
use serde::{Deserialize, Serialize};

pub(crate) const REPORT_SCHEMA_VERSION: u8 = 2;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CanaryTrigger {
    Daily,
    Weekly,
    Release,
    Manual,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CanaryTier {
    Installation,
    Deterministic,
    LiveCore,
    LiveExtended,
    ReleaseGate,
}

impl CanaryTier {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Installation => "installation",
            Self::Deterministic => "deterministic",
            Self::LiveCore => "live-core",
            Self::LiveExtended => "live-extended",
            Self::ReleaseGate => "release-gate",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CheckStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CanaryOutcome {
    Passed,
    Failed,
    InfrastructureFailure,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CanaryObservationKind {
    InventoryDrift,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CanaryObservation {
    pub kind: CanaryObservationKind,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum FailureClass {
    NanHarness,
    Harness,
    Installation,
    Provider,
    Infrastructure,
    TestContract,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NanHarnessEvidence {
    pub version: String,
    pub source: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct EnvironmentEvidence {
    pub operating_system: String,
    pub architecture: String,
    pub image: String,
    pub profile: String,
    #[serde(default)]
    pub runtimes: Vec<RuntimeEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RuntimeEvidence {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HarnessEvidence {
    pub id: HarnessKind,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CheckReport {
    pub name: String,
    pub status: CheckStatus,
    pub duration_milliseconds: u64,
    pub attempts: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FailureReport {
    pub class: FailureClass,
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub summary: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CanaryReport {
    pub schema_version: u8,
    pub run_id: String,
    pub cell_id: String,
    pub spec_sha256: String,
    pub trigger: CanaryTrigger,
    pub tier: CanaryTier,
    pub scenario: String,
    pub started_at: String,
    pub completed_at: String,
    pub duration_milliseconds: u64,
    pub nan_harness: NanHarnessEvidence,
    pub environment: EnvironmentEvidence,
    pub harness: HarnessEvidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub checks: Vec<CheckReport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observations: Vec<CanaryObservation>,
    pub outcome: CanaryOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<FailureReport>,
}
