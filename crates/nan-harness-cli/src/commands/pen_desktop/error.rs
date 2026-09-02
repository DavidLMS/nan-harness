use super::PenDocumentKind;
use crate::commands::credentials;
use crate::commands::desktop::DesktopStateError;
use nan_harness_core::SecretError;
use nan_harness_runtime::ChatGatewayError;
use nan_harness_telemetry::diagnostic::{Diagnostic, DiagnosticReason};
use serde_json::Error as JsonError;
use std::io;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum PenDesktopError {
    #[error("Pen Desktop integration is available only on macOS, Windows, and Linux")]
    UnsupportedPlatform,
    #[error(transparent)]
    Compatibility(#[from] nan_harness_runtime::DesktopCompatibilityError),
    #[error(
        "this Pen Desktop version is older than the supported version; retry with --allow-unsupported only if you accept the risk"
    )]
    OlderUnsupported,
    #[error(
        "this Pen Desktop version is newer than the live-verified version; retry with --allow-untested only if you accept the risk"
    )]
    NewerUntested,
    #[error("Pen Desktop was not found; install it from https://www.pen.dev or pass --executable")]
    AppNotFound,
    #[error("the Pen Desktop installation layout is invalid")]
    InvalidInstallation,
    #[error("Pen Desktop is already running; quit it completely before continuing")]
    AlreadyRunning,
    #[error("an interrupted Pen session needs recovery; quit Pen and run `nan pen --restore`")]
    PendingRecovery,
    #[error(
        "a Pen session backup exists without a valid receipt; inspect the private nan-harness state before continuing"
    )]
    OrphanBackup,
    #[error(
        "a persistent Pen backup exists without a valid receipt; inspect the private nan-harness state before continuing"
    )]
    OrphanPersistentBackup,
    #[error("Pen Desktop did not start; its previous configuration was restored")]
    DidNotStart,
    #[error("Pen Desktop did not terminate; quit it completely and run `nan pen --restore`")]
    DidNotTerminate,
    #[error("could not determine the current user's home directory")]
    MissingHomeDirectory,
    #[error("could not determine the nan-harness state directory")]
    MissingStateDirectory,
    #[error("a Pen Desktop path is not absolute")]
    InvalidPath,
    #[error("could not bind the private Pen gateway: {0}")]
    BindGateway(io::Error),
    #[error(transparent)]
    Gateway(#[from] ChatGatewayError),
    #[error(transparent)]
    State(#[from] DesktopStateError),
    #[error("model '{model}' is not available; available models: {}", available.join(", "))]
    ModelUnavailable {
        model: String,
        available: Vec<String>,
    },
    #[error("the NaN text-model catalog is empty")]
    EmptyModelCatalog,
    #[error("could not read Pen configuration '{}': {source}", path.display())]
    ReadDocument { path: PathBuf, source: io::Error },
    #[error("Pen configuration '{}' is not valid JSON: {source}", path.display())]
    ParseDocument { path: PathBuf, source: JsonError },
    #[error("Pen configuration '{}' must contain a JSON object", .0.display())]
    DocumentRootNotObject(PathBuf),
    #[error("the {document:?} configuration field '{field}' must contain a JSON object")]
    FieldNotObject {
        document: PenDocumentKind,
        field: &'static str,
    },
    #[error("could not serialize Pen configuration: {0}")]
    Serialize(JsonError),
    #[error("the managed {document:?} configuration is not valid JSON: {source}")]
    ParseManagedDocument {
        document: PenDocumentKind,
        source: JsonError,
    },
    #[error("the managed {0:?} configuration must contain a JSON object")]
    ManagedRootNotObject(PenDocumentKind),
    #[error("the managed NaN entry is missing from the {0:?} configuration")]
    ManagedEntryMissing(PenDocumentKind),
    #[error("'{}' changed while Pen was open; refusing to overwrite those changes", .0.display())]
    ManagedConfigurationChanged(PathBuf),
    #[error("the private Pen receipt is invalid: {0}")]
    ParseReceipt(JsonError),
    #[error("the private Pen receipt schema or targets are invalid")]
    InvalidReceipt,
    #[error("could not read a private Pen backup: {0}")]
    ReadBackup(io::Error),
    #[error("a private Pen backup does not match its receipt hash")]
    BackupHashMismatch,
    #[error("could not remove private Pen backups: {0}")]
    RemoveBackup(io::Error),
    #[error("could not launch Pen Desktop: {0}")]
    Launch(io::Error),
    #[error("could not inspect the Pen Desktop process: {0}")]
    ProcessCheck(io::Error),
    #[error("the Pen Desktop process check failed with exit code {0:?}")]
    ProcessCheckFailed(Option<i32>),
    #[error("could not terminate Pen Desktop: {0}")]
    Terminate(io::Error),
    #[error("Pen Desktop termination failed with exit code {0:?}")]
    TerminateFailed(Option<i32>),
    #[error("the saved credential could not be read: {0}")]
    Secret(SecretError),
    #[error("Pen Desktop is not configured by nan-harness")]
    PersistentNotConfigured,
    #[error("Pen Desktop's managed provider changed; refusing to overwrite user changes")]
    PersistentConfigurationChanged,
    #[error("persistent Pen configuration requires an interactive confirmation or --yes")]
    ConfirmationRequired,
    #[error("persistent Pen configuration was cancelled")]
    ConfigurationCancelled,
    #[error("could not read confirmation: {0}")]
    Prompt(io::Error),
    #[error(transparent)]
    Credential(#[from] credentials::CredentialError),
    #[error(transparent)]
    Persistence(#[from] crate::commands::persistence::PersistenceError),
}

impl PenDesktopError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Gateway(error) => error.code(),
            Self::UnsupportedPlatform
            | Self::Compatibility(_)
            | Self::OlderUnsupported
            | Self::NewerUntested
            | Self::AppNotFound
            | Self::InvalidInstallation => "NH-PEN-001",
            Self::AlreadyRunning
            | Self::PendingRecovery
            | Self::OrphanBackup
            | Self::OrphanPersistentBackup
            | Self::ManagedConfigurationChanged(_)
            | Self::PersistentConfigurationChanged => "NH-PEN-002",
            Self::Credential(error) => error.code(),
            Self::Persistence(error) => error.code(),
            _ => "NH-PEN-003",
        }
    }

    pub(crate) const fn diagnostic(&self) -> Diagnostic {
        match self {
            Self::UnsupportedPlatform
            | Self::Compatibility(_)
            | Self::OlderUnsupported
            | Self::NewerUntested => Diagnostic::general(DiagnosticReason::UnsupportedVersion),
            Self::AppNotFound => Diagnostic::general(DiagnosticReason::MissingExecutable),
            Self::InvalidInstallation => Diagnostic::general(DiagnosticReason::InvalidExecutable),
            Self::AlreadyRunning
            | Self::PendingRecovery
            | Self::OrphanBackup
            | Self::OrphanPersistentBackup
            | Self::ManagedConfigurationChanged(_)
            | Self::PersistentConfigurationChanged
            | Self::PersistentNotConfigured => {
                Diagnostic::general(DiagnosticReason::ConfigurationConflict)
            }
            Self::DidNotStart | Self::Launch(_) => {
                Diagnostic::general(DiagnosticReason::ProcessStartFailed)
            }
            Self::DidNotTerminate | Self::Terminate(_) | Self::TerminateFailed(_) => {
                Diagnostic::general(DiagnosticReason::ProcessTerminationFailed)
            }
            Self::Gateway(_) => Diagnostic::general(DiagnosticReason::BridgeExited),
            Self::Serialize(_) => Diagnostic::general(DiagnosticReason::SerializationFailed),
            Self::ProcessCheck(_) | Self::ProcessCheckFailed(_) => {
                Diagnostic::general(DiagnosticReason::ProcessWaitFailed)
            }
            Self::ModelUnavailable { .. }
            | Self::EmptyModelCatalog
            | Self::ParseDocument { .. }
            | Self::DocumentRootNotObject(_)
            | Self::FieldNotObject { .. }
            | Self::ParseManagedDocument { .. }
            | Self::ManagedRootNotObject(_)
            | Self::ManagedEntryMissing(_)
            | Self::ParseReceipt(_)
            | Self::InvalidReceipt
            | Self::ConfigurationCancelled => {
                Diagnostic::general(DiagnosticReason::InvalidConfiguration)
            }
            Self::MissingHomeDirectory
            | Self::MissingStateDirectory
            | Self::InvalidPath
            | Self::BindGateway(_)
            | Self::State(_)
            | Self::ReadDocument { .. }
            | Self::ReadBackup(_)
            | Self::BackupHashMismatch
            | Self::RemoveBackup(_)
            | Self::Secret(_)
            | Self::ConfirmationRequired
            | Self::Prompt(_)
            | Self::Credential(_)
            | Self::Persistence(_) => {
                Diagnostic::general(DiagnosticReason::FilesystemOperationFailed)
            }
        }
    }
}
