use crate::{HarnessKind, TransportKind};
use nan_harness_i18n::DiagnosticText;
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
        message: DiagnosticText,
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
    UnsafeTemporaryArtifact {
        artifact_id: String,
        reason: DiagnosticText,
    },
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

// Terminal localization is separate from canonical Display used by machine contracts.
impl nan_harness_i18n::TerminalMessage for PlanError {
    fn terminal_message(&self, locale: nan_harness_i18n::Locale) -> String {
        use nan_harness_i18n::messages as m;
        if locale == nan_harness_i18n::Locale::En {
            return self.to_string();
        }
        match self {
            Self::InvalidField { field, message } => m::error_plan_invalid_field(
                locale,
                &(field),
                &(nan_harness_i18n::TerminalMessage::terminal_message(message, locale)),
            ),
            Self::AdapterMismatch { adapter, requested } => {
                m::error_plan_adapter_mismatch(locale, &(adapter), &(requested))
            }
            Self::TransportMismatch {
                harness,
                expected,
                actual,
            } => m::error_plan_transport_mismatch(locale, &(actual), &(expected), &(harness)),
            Self::MissingSecretReference { reference } => {
                m::error_plan_missing_secret_reference(locale, &(reference))
            }
            Self::ConflictingEnvironment { variable } => {
                m::error_plan_conflicting_environment(locale, &(variable))
            }
            Self::UnsafeTemporaryArtifact {
                artifact_id,
                reason,
            } => m::error_plan_unsafe_temporary_artifact(
                locale,
                &(artifact_id),
                &(nan_harness_i18n::TerminalMessage::terminal_message(reason, locale)),
            ),
        }
    }
}
