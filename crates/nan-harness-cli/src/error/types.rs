use crate::commands::chatgpt_desktop::ChatGptDesktopError;
use crate::commands::claude_desktop::ClaudeDesktopError;
use crate::commands::configuration::ConfigurationError;
use crate::commands::credentials::CredentialError;
use crate::commands::hermes_desktop::HermesDesktopError;
use crate::commands::install::InstallError;
use crate::commands::pen_desktop::PenDesktopError;
use crate::commands::persistence::PersistenceError;
use crate::commands::search::SearchCommandError;
use crate::commands::uninstall::UninstallError;
use crate::commands::zed_desktop::ZedDesktopError;
use crate::usage_evidence::UsageEvidenceError;
use nan_harness_core::PlanError;
use nan_harness_runtime::{DiscoveryError, RuntimeError};
use nan_harness_telemetry::consent::SettingsError;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum CliError {
    #[error(transparent)]
    Discovery(#[from] DiscoveryError),
    #[error(transparent)]
    Install(#[from] InstallError),
    #[error(transparent)]
    Credential(#[from] CredentialError),
    #[error(transparent)]
    Configuration(#[from] ConfigurationError),
    #[error(transparent)]
    ChatGptDesktop(#[from] ChatGptDesktopError),
    #[error(transparent)]
    ClaudeDesktop(#[from] ClaudeDesktopError),
    #[error(transparent)]
    HermesDesktop(#[from] HermesDesktopError),
    #[error(transparent)]
    PenDesktop(#[from] PenDesktopError),
    #[error(transparent)]
    ZedDesktop(#[from] ZedDesktopError),
    #[error("internal credential preflight was not completed")]
    CredentialInvariant,
    #[error("terminal launch preflight task failed")]
    PreflightTaskFailed(#[source] tokio::task::JoinError),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error("could not read the current working directory: {0}")]
    CurrentDirectory(std::io::Error),
    #[error("could not generate a launch ID: {0}")]
    Random(getrandom::Error),
    #[error("launch plan is invalid: {0}")]
    InvalidPlan(PlanError),
    #[error("could not serialize the validated launch plan: {0}")]
    SerializePlan(serde_json::Error),
    #[error(transparent)]
    TelemetrySettings(#[from] SettingsError),
    #[error(transparent)]
    Update(#[from] nan_harness_runtime::update::UpdateError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    Search(#[from] SearchCommandError),
    #[error(transparent)]
    Uninstall(#[from] UninstallError),
    #[error(transparent)]
    UsageEvidence(UsageEvidenceError),
}

impl CliError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Discovery(error) => error.code(),
            Self::Install(_) => InstallError::code(),
            Self::Credential(error) => error.code(),
            Self::Configuration(error) => error.code(),
            Self::ChatGptDesktop(error) => error.code(),
            Self::ClaudeDesktop(error) => error.code(),
            Self::HermesDesktop(error) => error.code(),
            Self::PenDesktop(error) => error.code(),
            Self::ZedDesktop(error) => error.code(),
            Self::Runtime(error) => error.code(),
            Self::SerializePlan(_) => "NH-CLI-003",
            Self::CurrentDirectory(_)
            | Self::Random(_)
            | Self::CredentialInvariant
            | Self::PreflightTaskFailed(_) => "NH-CLI-005",
            Self::InvalidPlan(error) => error.code(),
            Self::TelemetrySettings(_) => "NH-TELEMETRY-001",
            Self::Update(error) => error.code(),
            Self::Persistence(error) => error.code(),
            Self::Search(error) => error.code(),
            Self::Uninstall(error) => error.code(),
            Self::UsageEvidence(_) => "NH-CLI-006",
        }
    }
}

// Terminal localization is separate from canonical Display used by machine contracts.
impl nan_harness_i18n::TerminalMessage for CliError {
    fn terminal_message(&self, locale: nan_harness_i18n::Locale) -> String {
        use nan_harness_i18n::messages as m;
        if locale == nan_harness_i18n::Locale::En {
            return self.to_string();
        }
        match self {
            Self::Discovery(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::Install(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::Credential(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::Configuration(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::ChatGptDesktop(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::ClaudeDesktop(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::HermesDesktop(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::PenDesktop(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::ZedDesktop(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::CredentialInvariant => m::error_cli_credential_invariant(locale),
            Self::PreflightTaskFailed(_) => m::error_cli_preflight_task_failed(locale),
            Self::Runtime(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::CurrentDirectory(field_0) => m::error_cli_current_directory(locale, &(field_0)),
            Self::Random(field_0) => m::error_cli_random(locale, &(field_0)),
            Self::InvalidPlan(field_0) => m::error_cli_invalid_plan(
                locale,
                &(nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)),
            ),
            Self::SerializePlan(field_0) => m::error_cli_serialize_plan(locale, &(field_0)),
            Self::TelemetrySettings(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::Update(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::Persistence(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::Search(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::Uninstall(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::UsageEvidence(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
        }
    }
}
