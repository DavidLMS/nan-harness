mod configuration;
mod credentials;
mod discovery_install;
mod io;
mod persistence;
pub(super) mod runtime;
mod update;

use super::super::CliError;
use nan_harness_telemetry::event::FailureCause;

pub(super) type Classification = (FailureCause, Option<u16>);

pub(super) fn classify(error: &CliError) -> Classification {
    match error {
        CliError::Discovery(error) => discovery_install::classify_discovery(error),
        CliError::Install(error) => discovery_install::classify_install(error),
        CliError::Credential(error) => credentials::classify(error),
        CliError::Configuration(error) => configuration::classify(error),
        CliError::ChatGptDesktop(_)
        | CliError::ClaudeDesktop(_)
        | CliError::HermesDesktop(_)
        | CliError::PenDesktop(_)
        | CliError::ZedDesktop(_)
        | CliError::CredentialInvariant
        | CliError::InvalidPlan(_) => (FailureCause::InvalidConfiguration, None),
        CliError::Runtime(error) => runtime::classify(error),
        CliError::CurrentDirectory(source) => (io::classify(source), None),
        CliError::SerializePlan(_) => (FailureCause::Serialization, None),
        CliError::Random(_) | CliError::PreflightTaskFailed(_) => (FailureCause::Internal, None),
        CliError::TelemetrySettings(_) | CliError::Uninstall(_) | CliError::UsageEvidence(_) => {
            (FailureCause::Filesystem, None)
        }
        CliError::Update(error) => update::classify(error),
        CliError::Persistence(error) => persistence::classify(error),
    }
}
