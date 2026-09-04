use super::{
    BridgeEndpoint, DiagnosticOperation, DocumentKind, IoErrorKind, ModelPolicy, ReasoningRequest,
    VersionComponent,
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
