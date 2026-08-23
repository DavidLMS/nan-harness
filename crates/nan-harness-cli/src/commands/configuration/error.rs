use crate::commands::credentials::CredentialError;
use crate::commands::persistence::PersistenceError;
use nan_harness_core::HarnessKind;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ConfigurationError {
    #[error("a harness is required; use `nan config <harness>` or `nan config --status`")]
    HarnessRequired,
    #[error("--yes only applies to first-time native configuration or --remove-all")]
    UnusedYes,
    #[error(
        "{0} cannot store this provider configuration natively; launch it through nan-harness instead"
    )]
    BridgeOnly(HarnessKind),
    #[error("{0} is not configured by nan-harness; run `nan config {0}` first")]
    RefreshRequiresConfiguration(HarnessKind),
    #[error("this configuration change requires an interactive confirmation or --yes")]
    ConfirmationRequired,
    #[error("could not determine the nan-harness state directory")]
    MissingStateDirectory,
    #[error("could not determine the current user's home directory")]
    MissingHomeDirectory,
    #[error("managed configuration receipt does not match the current harness layout")]
    ReceiptMismatch,
    #[error("managed JSON path is empty")]
    InvalidManagedPath,
    #[error("managed text block markers are missing, duplicated, or out of order")]
    InvalidManagedBlock,
    #[error("'{}' already contains configuration that nan-harness does not own", .0.display())]
    UnmanagedDocumentConflict(PathBuf),
    #[error("'{}' changed after nan-harness configured it; refusing to overwrite user changes", .0.display())]
    ManagedDocumentChanged(PathBuf),
    #[error("configuration document '{}' must contain a JSON object", .0.display())]
    DocumentRootNotObject(PathBuf),
    #[error("configuration field '{field}' in '{}' must contain a JSON object", path.display())]
    DocumentFieldNotObject { path: PathBuf, field: String },
    #[error("could not read configuration document '{}': {source}", path.display())]
    ReadDocument {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not remove configuration document '{}': {source}", path.display())]
    RemoveDocument {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("configuration document '{}' is not valid JSON: {source}", path.display())]
    ParseDocument {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("configuration document '{}' is not valid TOML: {source}", path.display())]
    ParseToml {
        path: PathBuf,
        source: toml_edit::TomlError,
    },
    #[error("could not normalize managed TOML data: {0}")]
    NormalizeToml(toml_edit::de::Error),
    #[error("configuration field '{field}' in '{}' must contain a TOML table", path.display())]
    TomlFieldNotTable { path: PathBuf, field: String },
    #[error("configuration field '{field}' in '{}' must contain a TOML string", path.display())]
    TomlFieldNotString { path: PathBuf, field: String },
    #[error("configuration document '{}' is not UTF-8: {source}", path.display())]
    InvalidUtf8 {
        path: PathBuf,
        source: std::string::FromUtf8Error,
    },
    #[error("could not read nan-harness configuration receipts '{}': {source}", path.display())]
    ReadState {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("nan-harness configuration receipts are not valid JSON: {0}")]
    ParseState(serde_json::Error),
    #[error("nan-harness configuration receipt schema {0} is not supported")]
    UnsupportedStateSchema(u8),
    #[error("could not serialize nan-harness configuration receipts: {0}")]
    SerializeState(serde_json::Error),
    #[error("could not serialize a harness configuration document: {0}")]
    SerializeDocument(serde_json::Error),
    #[error(
        "model '{model}' has a {field} that cannot be represented in native TOML configuration"
    )]
    ModelValueOutOfRange { field: &'static str, model: String },
    #[error("could not read confirmation: {0}")]
    Prompt(std::io::Error),
    #[error(transparent)]
    Credential(#[from] CredentialError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
}

impl ConfigurationError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::BridgeOnly(_)
            | Self::RefreshRequiresConfiguration(_)
            | Self::HarnessRequired
            | Self::UnusedYes => "NH-CONFIG-001",
            Self::ConfirmationRequired | Self::Prompt(_) => "NH-CONFIG-002",
            Self::UnmanagedDocumentConflict(_) => "NH-CONFIG-003",
            Self::ManagedDocumentChanged(_)
            | Self::ReceiptMismatch
            | Self::InvalidManagedBlock
            | Self::InvalidManagedPath => "NH-CONFIG-004",
            Self::Credential(error) => error.code(),
            Self::Persistence(error) => error.code(),
            _ => "NH-CONFIG-005",
        }
    }
}
