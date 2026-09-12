use crate::prepared::PreparedError;
use crate::process::ProcessError;
use crate::search_policy::SearchPolicyError;
use nan_harness_bridge::BridgeError;
use nan_harness_core::{PlanError, SecretError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("launch plan is invalid: {0}")]
    InvalidPlan(PlanError),
    #[error("could not bind the local bridge: {0}")]
    BindBridge(std::io::Error),
    #[error(transparent)]
    Bridge(#[from] BridgeError),
    #[error("the local bridge stopped before the harness process")]
    BridgeExited,
    #[error(transparent)]
    Prepared(#[from] PreparedError),
    #[error(transparent)]
    Process(#[from] ProcessError),
    #[error(transparent)]
    Secret(SecretError),
    #[error("could not generate a private bridge token: {0}")]
    Random(getrandom::Error),
    #[error("could not wait for the harness process: {0}")]
    WaitForProcess(std::io::Error),
    #[error("could not terminate the harness process: {0}")]
    TerminateProcess(std::io::Error),
    #[error("the harness process ID is unavailable")]
    MissingProcessId,
    #[error(transparent)]
    SearchPolicy(#[from] SearchPolicyError),
}

impl RuntimeError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidPlan(_) => "NH-RUNTIME-001",
            Self::BindBridge(_) | Self::Bridge(_) | Self::BridgeExited => "NH-RUNTIME-003",
            Self::Prepared(_) => "NH-RUNTIME-004",
            Self::Process(_) => "NH-RUNTIME-005",
            Self::Secret(_) | Self::Random(_) => "NH-RUNTIME-006",
            Self::WaitForProcess(_) | Self::TerminateProcess(_) | Self::MissingProcessId => {
                "NH-RUNTIME-007"
            }
            Self::SearchPolicy(_) => "NH-RUNTIME-008",
        }
    }

    #[must_use]
    pub fn unavailable_model(&self) -> Option<(&str, &[String])> {
        match self {
            Self::Bridge(BridgeError::SelectedModelUnavailable { model, available }) => {
                Some((model, available))
            }
            _ => None,
        }
    }
}

// Terminal localization is separate from canonical Display used by machine contracts.
impl nan_harness_i18n::TerminalMessage for RuntimeError {
    fn terminal_message(&self, locale: nan_harness_i18n::Locale) -> String {
        use nan_harness_i18n::messages as m;
        if locale == nan_harness_i18n::Locale::En {
            return self.to_string();
        }
        match self {
            Self::InvalidPlan(field_0) => m::error_runtime_invalid_plan(
                locale,
                &(nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)),
            ),
            Self::BindBridge(field_0) => m::error_runtime_bind_bridge(locale, &(field_0)),
            Self::Bridge(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::BridgeExited => m::error_runtime_bridge_exited(locale),
            Self::Prepared(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::Process(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::Secret(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::Random(field_0) => m::error_runtime_random(locale, &(field_0)),
            Self::WaitForProcess(field_0) => m::error_runtime_wait_for_process(locale, &(field_0)),
            Self::TerminateProcess(field_0) => {
                m::error_runtime_terminate_process(locale, &(field_0))
            }
            Self::MissingProcessId => m::error_runtime_missing_process_id(locale),
            Self::SearchPolicy(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
        }
    }
}
