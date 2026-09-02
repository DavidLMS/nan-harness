use crate::commands::configuration::ConfigurationError;
use crate::commands::credentials::CredentialError;
use crate::commands::hermes_desktop::HermesDesktopError;
use crate::commands::pen_desktop::PenDesktopError;
use crate::commands::persistence::PersistenceError;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum UninstallError {
    #[error(transparent)]
    Configuration(#[from] ConfigurationError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    Credential(#[from] CredentialError),
    #[error(transparent)]
    HermesDesktop(#[from] HermesDesktopError),
    #[error(transparent)]
    PenDesktop(#[from] PenDesktopError),
    #[error("uninstall confirmation requires an interactive terminal; rerun with --yes")]
    ConfirmationRequired,
    #[error(
        "{0} has recovery state; close the app and run its `nan ...-desktop --restore` command before uninstalling"
    )]
    DesktopRecoveryRequired(&'static str),
    #[error("this nan-harness executable is not managed by the release installer")]
    InstallationNotManaged,
    #[error("could not determine the current nan-harness executable: {0}")]
    CurrentExecutable(std::io::Error),
    #[error("could not resolve executable '{}': {source}", path.display())]
    CanonicalizeExecutable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("installation receipt points to '{}', but the running executable is '{}'", actual.display(), expected.display())]
    ExecutableMismatch { expected: PathBuf, actual: PathBuf },
    #[error("unsafe nan-harness installation path '{}'", .0.display())]
    UnsafeInstallationPath(PathBuf),
    #[error("unsafe nan-harness alias path '{}'", .0.display())]
    UnsafeAliasPath(PathBuf),
    #[error("unsafe nan-harness application data directory '{}'", .0.display())]
    UnsafeDataDirectory(PathBuf),
    #[error("could not inspect application data directory '{}': {source}", path.display())]
    InspectDataDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not inspect alias '{}': {source}", path.display())]
    InspectAlias {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not read installation receipt '{}': {source}", path.display())]
    ReadReceipt {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("installation receipt is invalid: {0}")]
    ParseReceipt(serde_json::Error),
    #[error("installation receipt uses unsupported schema version {0}")]
    UnsupportedReceiptSchema(u8),
    #[error("could not create application data directory '{}': {source}", path.display())]
    CreateDataDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not serialize the installation receipt: {0}")]
    SerializeReceipt(serde_json::Error),
    #[error("could not write installation receipt '{}': {source}", path.display())]
    WriteReceipt {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not read uninstall confirmation: {0}")]
    Prompt(std::io::Error),
    #[cfg(not(windows))]
    #[error("could not remove '{}': {source}", path.display())]
    RemoveFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[cfg(not(windows))]
    #[error("could not remove application data '{}': {source}", path.display())]
    RemoveDataDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[cfg(windows)]
    #[error("could not create the Windows uninstall helper: {0}")]
    CreateHelper(std::io::Error),
    #[cfg(windows)]
    #[error("could not start the Windows uninstall helper: {0}")]
    StartHelper(std::io::Error),
}

impl UninstallError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Persistence(error) => error.code(),
            Self::Configuration(error) => error.code(),
            Self::Credential(error) => error.code(),
            Self::HermesDesktop(error) => error.code(),
            Self::PenDesktop(error) => error.code(),
            Self::ConfirmationRequired | Self::DesktopRecoveryRequired(_) | Self::Prompt(_) => {
                "NH-UNINSTALL-001"
            }
            Self::InstallationNotManaged
            | Self::ExecutableMismatch { .. }
            | Self::UnsafeInstallationPath(_)
            | Self::UnsafeAliasPath(_)
            | Self::UnsafeDataDirectory(_) => "NH-UNINSTALL-002",
            Self::ReadReceipt { .. }
            | Self::ParseReceipt(_)
            | Self::UnsupportedReceiptSchema(_)
            | Self::SerializeReceipt(_)
            | Self::WriteReceipt { .. } => "NH-UNINSTALL-003",
            Self::CurrentExecutable(_)
            | Self::CanonicalizeExecutable { .. }
            | Self::InspectDataDirectory { .. }
            | Self::InspectAlias { .. }
            | Self::CreateDataDirectory { .. } => "NH-UNINSTALL-004",
            #[cfg(not(windows))]
            Self::RemoveFile { .. } | Self::RemoveDataDirectory { .. } => "NH-UNINSTALL-004",
            #[cfg(windows)]
            Self::CreateHelper(_) | Self::StartHelper(_) => "NH-UNINSTALL-005",
        }
    }
}
