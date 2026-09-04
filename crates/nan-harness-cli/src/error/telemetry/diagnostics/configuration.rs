use super::{Classification, credentials, io, persistence};
use crate::commands::configuration::ConfigurationError;
use nan_harness_telemetry::event::FailureCause;

pub(super) fn classify(error: &ConfigurationError) -> Classification {
    match error {
        ConfigurationError::Credential(error) => credentials::classify(error),
        ConfigurationError::Persistence(error) => persistence::classify(error),
        ConfigurationError::ReadDocument { source, .. }
        | ConfigurationError::RemoveDocument { source, .. }
        | ConfigurationError::ReadState { source, .. }
        | ConfigurationError::Prompt(source) => (io::classify(source), None),
        ConfigurationError::ParseDocument { .. }
        | ConfigurationError::InvalidUtf8 { .. }
        | ConfigurationError::ParseState(_)
        | ConfigurationError::UnsupportedStateSchema(_)
        | ConfigurationError::SerializeState(_)
        | ConfigurationError::SerializeDocument(_) => (FailureCause::InvalidData, None),
        ConfigurationError::MissingStateDirectory | ConfigurationError::MissingHomeDirectory => {
            (FailureCause::Filesystem, None)
        }
        _ => (FailureCause::InvalidConfiguration, None),
    }
}
