use super::{Classification, io, persistence};
use crate::commands::credentials::CredentialError;
use nan_harness_telemetry::event::FailureCause;

pub(super) fn classify(error: &CredentialError) -> Classification {
    match error {
        CredentialError::MissingCredential | CredentialError::MissingSavedCredential => {
            (FailureCause::MissingCredential, None)
        }
        CredentialError::InteractiveLoginRequired
        | CredentialError::InvalidConfigDirectory(_)
        | CredentialError::InvalidBackend(_)
        | CredentialError::NonUnicodeBackend
        | CredentialError::ParseReceipt(_)
        | CredentialError::UnsupportedReceiptSchema(_)
        | CredentialError::SerializeReceipt(_)
        | CredentialError::ParseVerificationReceipt(_)
        | CredentialError::SerializeVerificationReceipt(_)
        | CredentialError::LogoutConfirmationRequired
        | CredentialError::LogoutModeRequired
        | CredentialError::InvalidLogoutChoice
        | CredentialError::ConfigurationOperation(_)
        | CredentialError::Secret(_)
        | CredentialError::Config(_) => (FailureCause::InvalidConfiguration, None),
        CredentialError::Prompt(error) => (io::classify(error), None),
        CredentialError::Verification(error) | CredentialError::State(error) => {
            persistence::classify(error)
        }
        CredentialError::VerificationTimeout => (FailureCause::Timeout, None),
        CredentialError::Keyring(_) => (FailureCause::PermissionDenied, None),
        CredentialError::ReadFile { source, .. } | CredentialError::RemoveFile { source, .. } => {
            (io::classify(source), None)
        }
        CredentialError::SystemTime(_) => (FailureCause::Internal, None),
        CredentialError::MissingConfigDirectory => (FailureCause::Filesystem, None),
    }
}
