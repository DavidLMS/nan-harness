use crate::commands::persistence::PersistenceError;
use keyring::Error as KeyringError;
use nan_harness_core::SecretError;
use nan_harness_runtime::ConfigError;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum CredentialError {
    #[error(
        "no NaN API key is configured; run `nan auth login` in an interactive terminal or set NAN_API_KEY"
    )]
    MissingCredential,
    #[error(
        "no API key is saved by nan-harness; run `nan auth login` interactively before using `nan config`"
    )]
    MissingSavedCredential,
    #[error("`nan auth login` requires an interactive terminal")]
    InteractiveLoginRequired,
    #[error("could not determine the nan-harness configuration directory")]
    MissingConfigDirectory,
    #[error("nan-harness configuration directory '{}' must be absolute", .0.display())]
    InvalidConfigDirectory(PathBuf),
    #[error("NAN_HARNESS_CREDENTIAL_BACKEND must be auto, keyring, or file; received '{0}'")]
    InvalidBackend(String),
    #[error("NAN_HARNESS_CREDENTIAL_BACKEND is not valid Unicode")]
    NonUnicodeBackend,
    #[error("could not read the hidden API key: {0}")]
    Prompt(std::io::Error),
    #[error("could not access the system credential store: {0}")]
    Keyring(KeyringError),
    #[error("could not read credential file '{}': {source}", path.display())]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not remove credential file '{}': {source}", path.display())]
    RemoveFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("credential receipt is not valid JSON: {0}")]
    ParseReceipt(serde_json::Error),
    #[error("credential receipt schema {0} is not supported")]
    UnsupportedReceiptSchema(u8),
    #[error("could not serialize the credential receipt: {0}")]
    SerializeReceipt(serde_json::Error),
    #[error("credential verification receipt is not valid JSON: {0}")]
    ParseVerificationReceipt(serde_json::Error),
    #[error("could not serialize the credential verification receipt: {0}")]
    SerializeVerificationReceipt(serde_json::Error),
    #[error("the system clock is earlier than the Unix epoch: {0}")]
    SystemTime(std::time::SystemTimeError),
    #[error("`nan auth logout` requires --yes in a non-interactive terminal")]
    LogoutConfirmationRequired,
    #[error(
        "non-interactive logout with managed configurations requires --yes and exactly one of --remove-configs or --keep-configs"
    )]
    LogoutModeRequired,
    #[error("logout choice must be 1, 2, or 3")]
    InvalidLogoutChoice,
    #[error("could not update managed harness configurations: {0}")]
    ConfigurationOperation(String),
    #[error("could not store the NaN API key: {0}")]
    State(#[from] PersistenceError),
    #[error("the NaN API key is invalid: {0}")]
    Secret(SecretError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("could not verify the NaN API key: {0}")]
    Verification(PersistenceError),
    #[error("NaN API key verification timed out after 10 seconds")]
    VerificationTimeout,
}

impl CredentialError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::MissingCredential
            | Self::MissingSavedCredential
            | Self::InteractiveLoginRequired => "NH-CREDENTIAL-001",
            Self::Prompt(_) | Self::Secret(_) => "NH-CREDENTIAL-002",
            Self::Verification(_) | Self::VerificationTimeout => "NH-CREDENTIAL-004",
            Self::Config(error) => error.code(),
            Self::MissingConfigDirectory
            | Self::InvalidConfigDirectory(_)
            | Self::InvalidBackend(_)
            | Self::NonUnicodeBackend
            | Self::Keyring(_)
            | Self::ReadFile { .. }
            | Self::RemoveFile { .. }
            | Self::ParseReceipt(_)
            | Self::UnsupportedReceiptSchema(_)
            | Self::SerializeReceipt(_)
            | Self::ParseVerificationReceipt(_)
            | Self::SerializeVerificationReceipt(_)
            | Self::SystemTime(_)
            | Self::LogoutConfirmationRequired
            | Self::LogoutModeRequired
            | Self::InvalidLogoutChoice
            | Self::ConfigurationOperation(_)
            | Self::State(_) => "NH-CREDENTIAL-003",
        }
    }
}
