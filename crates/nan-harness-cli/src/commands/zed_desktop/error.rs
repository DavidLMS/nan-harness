use crate::commands::desktop::DesktopStateError;
use nan_harness_runtime::ChatGatewayError;
use nan_harness_telemetry::diagnostic::{Diagnostic, DiagnosticReason};
use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ZedDesktopError {
    #[error("Zed integration is available only on macOS, Windows, and Linux")]
    UnsupportedPlatform,
    #[error(transparent)]
    Compatibility(#[from] nan_harness_runtime::DesktopCompatibilityError),
    #[error(
        "this Zed version is older than the supported version; retry with --allow-unsupported only if you accept the risk"
    )]
    OlderUnsupported,
    #[error(
        "this Zed version is newer than the live-verified version; retry with --allow-untested only if you accept the risk"
    )]
    NewerUntested,
    #[error("Zed was not found; install the stable app from https://zed.dev or pass --executable")]
    AppNotFound,
    #[error("the selected Zed executable or app bundle is invalid")]
    InvalidInstallation,
    #[error("Zed is already running; quit it completely before continuing")]
    AlreadyRunning,
    #[error("an interrupted Zed session needs recovery; quit Zed and run `nanh zed --restore`")]
    PendingRecovery,
    #[error(
        "a Zed backup exists without a valid receipt; inspect the private nan-harness state before continuing"
    )]
    OrphanBackup,
    #[error("Zed did not start; its previous settings were restored")]
    DidNotStart,
    #[error("Zed did not terminate; quit it completely and run `nanh zed --restore`")]
    DidNotTerminate,
    #[error("could not determine the current user's home directory")]
    MissingHomeDirectory,
    #[error("could not determine the nan-harness state directory")]
    MissingStateDirectory,
    #[error("could not determine Zed's configuration directory on this platform")]
    MissingPlatformDirectory,
    #[error("a managed Zed path is invalid")]
    InvalidPath,
    #[error("the requested Zed workspace is not a directory")]
    InvalidWorkspace,
    #[error(
        "--foreground, --wait, and --user-data-dir are managed by nan-harness and cannot be passed to Zed"
    )]
    ReservedArgument,
    #[error(
        "Zed settings changed while the managed launch was being prepared; retry after closing every Zed process"
    )]
    SettingsChangedBeforeWrite,
    #[error("Zed settings are not valid UTF-8")]
    SettingsUtf8(#[source] std::str::Utf8Error),
    #[error("Zed settings are not valid JSONC")]
    ParseSettings(String),
    #[error("Zed settings must contain a JSON object")]
    SettingsRootNotObject,
    #[error("the Zed setting '{0}' must contain a JSON object")]
    SettingsFieldNotObject(&'static str),
    #[error("Zed's existing agent.default_model setting is invalid")]
    InvalidDefaultModel,
    #[error(
        "Zed already contains an openai_compatible.nan provider not owned by this nan-harness session"
    )]
    UnmanagedProviderConflict,
    #[error(
        "managed Zed provider or model settings changed; refusing to overwrite them; close Zed and resolve the settings before retrying `nanh zed --restore`"
    )]
    ManagedConfigurationChanged,
    #[error("could not generate the temporary Zed settings")]
    GenerateSettings(String),
    #[error("could not serialize temporary Zed state: {0}")]
    Serialize(serde_json::Error),
    #[error("the private Zed receipt is invalid: {0}")]
    ParseReceipt(serde_json::Error),
    #[error("the private Zed receipt schema or targets are invalid")]
    InvalidReceipt,
    #[error("could not read Zed settings: {0}")]
    ReadSettings(io::Error),
    #[error("could not read the private Zed backup: {0}")]
    ReadBackup(io::Error),
    #[error("the private Zed backup does not match its receipt hash")]
    BackupHashMismatch,
    #[error("could not remove private Zed backup state: {0}")]
    RemoveBackup(io::Error),
    #[error("could not bind the private Zed gateway: {0}")]
    BindGateway(io::Error),
    #[error(transparent)]
    Gateway(#[from] ChatGatewayError),
    #[error("the private Zed gateway exited while Zed was still active")]
    GatewayExited,
    #[error(transparent)]
    State(#[from] DesktopStateError),
    #[error("could not inspect the Zed version: {0}")]
    VersionCommand(io::Error),
    #[error("the Zed version command failed with exit code {0:?}")]
    VersionCommandFailed(Option<i32>),
    #[error("could not launch Zed: {0}")]
    Launch(io::Error),
    #[error("could not wait for Zed: {0}")]
    Wait(io::Error),
    #[error(
        "could not inspect the Zed process; settings recovery was preserved; quit Zed and run `nanh zed --restore`: {0}"
    )]
    ProcessCheck(io::Error),
    #[error(
        "the Zed process check failed with exit code {0:?}; recovery was preserved; quit Zed and run `nanh zed --restore`"
    )]
    ProcessCheckFailed(Option<i32>),
    #[error("could not terminate Zed; quit it completely and run `nanh zed --restore`: {0}")]
    Terminate(io::Error),
    #[error(
        "Zed termination failed with exit code {0:?}; quit it completely and run `nanh zed --restore`"
    )]
    TerminateFailed(Option<i32>),
    #[error("model '{model}' is not available; available models: {}", available.join(", "))]
    ModelUnavailable {
        model: String,
        available: Vec<String>,
    },
    #[error("the NaN text-model catalog is empty")]
    EmptyModelCatalog,
}

impl ZedDesktopError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Gateway(error) => error.code(),
            Self::UnsupportedPlatform
            | Self::Compatibility(_)
            | Self::OlderUnsupported
            | Self::NewerUntested
            | Self::AppNotFound
            | Self::InvalidInstallation
            | Self::VersionCommand(_)
            | Self::VersionCommandFailed(_) => "NH-ZED-001",
            Self::AlreadyRunning
            | Self::PendingRecovery
            | Self::OrphanBackup
            | Self::SettingsChangedBeforeWrite
            | Self::UnmanagedProviderConflict
            | Self::ManagedConfigurationChanged => "NH-ZED-002",
            _ => "NH-ZED-003",
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
            | Self::SettingsChangedBeforeWrite
            | Self::UnmanagedProviderConflict
            | Self::ManagedConfigurationChanged => {
                Diagnostic::general(DiagnosticReason::ConfigurationConflict)
            }
            Self::DidNotStart | Self::Launch(_) => {
                Diagnostic::general(DiagnosticReason::ProcessStartFailed)
            }
            Self::DidNotTerminate | Self::Terminate(_) | Self::TerminateFailed(_) => {
                Diagnostic::general(DiagnosticReason::ProcessTerminationFailed)
            }
            Self::Gateway(_) | Self::GatewayExited => {
                Diagnostic::general(DiagnosticReason::BridgeExited)
            }
            Self::ProcessCheck(_) | Self::ProcessCheckFailed(_) | Self::Wait(_) => {
                Diagnostic::general(DiagnosticReason::ProcessWaitFailed)
            }
            Self::VersionCommand(_) | Self::VersionCommandFailed(_) => {
                Diagnostic::general(DiagnosticReason::UnparseableVersion)
            }
            Self::Serialize(_) => Diagnostic::general(DiagnosticReason::SerializationFailed),
            Self::ModelUnavailable { .. } => {
                Diagnostic::general(DiagnosticReason::ModelUnavailable)
            }
            Self::EmptyModelCatalog => Diagnostic::general(DiagnosticReason::ModelCatalogEmpty),
            Self::ReservedArgument
            | Self::SettingsUtf8(_)
            | Self::ParseSettings(_)
            | Self::SettingsRootNotObject
            | Self::SettingsFieldNotObject(_)
            | Self::InvalidDefaultModel
            | Self::GenerateSettings(_)
            | Self::ParseReceipt(_)
            | Self::InvalidReceipt
            | Self::InvalidWorkspace => Diagnostic::general(DiagnosticReason::InvalidConfiguration),
            Self::MissingHomeDirectory
            | Self::MissingStateDirectory
            | Self::MissingPlatformDirectory
            | Self::InvalidPath
            | Self::ReadSettings(_)
            | Self::ReadBackup(_)
            | Self::BackupHashMismatch
            | Self::RemoveBackup(_)
            | Self::BindGateway(_)
            | Self::State(_) => Diagnostic::general(DiagnosticReason::FilesystemOperationFailed),
        }
    }
}
