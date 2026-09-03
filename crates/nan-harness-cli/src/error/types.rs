use crate::commands::chatgpt_desktop::ChatGptDesktopError;
use crate::commands::claude_desktop::ClaudeDesktopError;
use crate::commands::configuration::ConfigurationError;
use crate::commands::credentials::CredentialError;
use crate::commands::hermes_desktop::HermesDesktopError;
use crate::commands::install::InstallError;
use crate::commands::pen_desktop::PenDesktopError;
use crate::commands::persistence::PersistenceError;
use crate::commands::uninstall::UninstallError;
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
    #[error("internal credential preflight was not completed")]
    CredentialInvariant,
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
            Self::Runtime(error) => error.code(),
            Self::SerializePlan(_) => "NH-CLI-003",
            Self::CurrentDirectory(_) | Self::Random(_) | Self::CredentialInvariant => "NH-CLI-005",
            Self::InvalidPlan(error) => error.code(),
            Self::TelemetrySettings(_) => "NH-TELEMETRY-001",
            Self::Update(error) => error.code(),
            Self::Persistence(error) => error.code(),
            Self::Uninstall(error) => error.code(),
            Self::UsageEvidence(_) => "NH-CLI-006",
        }
    }
}
