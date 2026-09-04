use super::{
    AttemptBucket, BridgeEndpoint, DiagnosticOperation, DocumentKind, IoErrorKind, ModelPolicy,
    ReasoningRequest, RecoveryOutcome, RequestPriority, TimeoutPhase, VersionComponent,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DiagnosticDetails {
    General,
    Bridge {
        endpoint: BridgeEndpoint,
        #[serde(skip_serializing_if = "Option::is_none")]
        model_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        requested_reasoning: Option<ReasoningRequest>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model_policy: Option<ModelPolicy>,
        #[serde(skip_serializing_if = "Option::is_none")]
        timeout_phase: Option<TimeoutPhase>,
        #[serde(skip_serializing_if = "Option::is_none")]
        recovery_outcome: Option<RecoveryOutcome>,
        #[serde(skip_serializing_if = "Option::is_none")]
        attempt: Option<AttemptBucket>,
        #[serde(skip_serializing_if = "Option::is_none")]
        priority: Option<RequestPriority>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_replay_detected: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_bypass_attempted: Option<bool>,
    },
    Io {
        operation: DiagnosticOperation,
        error_kind: IoErrorKind,
    },
    Process {
        operation: DiagnosticOperation,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
    },
    Version {
        component: VersionComponent,
        #[serde(skip_serializing_if = "Option::is_none")]
        detected: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        expected: Option<String>,
    },
    Http {
        operation: DiagnosticOperation,
        status: u16,
    },
    Schema {
        document: DocumentKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        observed_version: Option<u16>,
    },
}
