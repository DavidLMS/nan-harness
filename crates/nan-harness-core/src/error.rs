use crate::{HarnessKind, TransportKind};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    Configuration,
    Contract,
    Discovery,
    Security,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PlanError {
    #[error("invalid field '{field}': {message}")]
    InvalidField {
        field: &'static str,
        message: String,
    },
    #[error("adapter for {adapter} cannot plan a {requested} harness")]
    AdapterMismatch {
        adapter: HarnessKind,
        requested: HarnessKind,
    },
    #[error("{harness} requires {expected}, but the plan selected {actual}")]
    TransportMismatch {
        harness: HarnessKind,
        expected: TransportKind,
        actual: TransportKind,
    },
    #[error("secret reference '{reference}' is not mapped into the child environment")]
    MissingSecretReference { reference: String },
    #[error("environment variable '{variable}' has conflicting instructions")]
    ConflictingEnvironment { variable: String },
    #[error("temporary artifact '{artifact_id}' is unsafe: {reason}")]
    UnsafeTemporaryArtifact { artifact_id: String, reason: String },
}

impl PlanError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidField { .. } => "NH-PLAN-001",
            Self::AdapterMismatch { .. } => "NH-PLAN-002",
            Self::TransportMismatch { .. } => "NH-PLAN-003",
            Self::MissingSecretReference { .. } => "NH-PLAN-004",
            Self::ConflictingEnvironment { .. } => "NH-PLAN-005",
            Self::UnsafeTemporaryArtifact { .. } => "NH-PLAN-006",
        }
    }

    #[must_use]
    pub const fn category(&self) -> ErrorCategory {
        match self {
            Self::InvalidField { .. }
            | Self::AdapterMismatch { .. }
            | Self::TransportMismatch { .. } => ErrorCategory::Contract,
            Self::MissingSecretReference { .. }
            | Self::ConflictingEnvironment { .. }
            | Self::UnsafeTemporaryArtifact { .. } => ErrorCategory::Security,
        }
    }
}
