use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticOperation {
    LoadCompatibilityManifest,
    ReadWorkingDirectory,
    ResolveExecutable,
    RunVersionCommand,
    DownloadInstaller,
    RunInstaller,
    RunPostInstallCheck,
    BindBridge,
    RunBridge,
    PrepareLaunch,
    StartHarness,
    WaitForHarness,
    StopHarness,
    DiscoverModels,
    ReadConfiguration,
    WriteConfiguration,
    RemoveConfiguration,
    ReadCredential,
    WriteCredential,
    RemoveCredential,
    FetchUpdateManifest,
    DownloadUpdate,
    VerifyUpdate,
    ReplaceExecutable,
    RemoveInstallation,
    ConfigureTelemetry,
}

impl DiagnosticOperation {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LoadCompatibilityManifest
            | Self::ResolveExecutable
            | Self::RunVersionCommand
            | Self::DownloadInstaller
            | Self::RunInstaller
            | Self::RunPostInstallCheck
            | Self::BindBridge
            | Self::RunBridge
            | Self::PrepareLaunch
            | Self::StartHarness
            | Self::WaitForHarness
            | Self::StopHarness
            | Self::DiscoverModels => diagnostic_launch_operation(self),
            Self::ReadWorkingDirectory
            | Self::ReadConfiguration
            | Self::WriteConfiguration
            | Self::RemoveConfiguration
            | Self::ReadCredential
            | Self::WriteCredential
            | Self::RemoveCredential
            | Self::ConfigureTelemetry => diagnostic_configuration_operation(self),
            Self::FetchUpdateManifest
            | Self::DownloadUpdate
            | Self::VerifyUpdate
            | Self::ReplaceExecutable
            | Self::RemoveInstallation => diagnostic_update_operation(self),
        }
    }
}

const fn diagnostic_launch_operation(operation: DiagnosticOperation) -> &'static str {
    match operation {
        DiagnosticOperation::LoadCompatibilityManifest => "load-compatibility-manifest",
        DiagnosticOperation::ResolveExecutable => "resolve-executable",
        DiagnosticOperation::RunVersionCommand => "run-version-command",
        DiagnosticOperation::DownloadInstaller => "download-installer",
        DiagnosticOperation::RunInstaller => "run-installer",
        DiagnosticOperation::RunPostInstallCheck => "run-post-install-check",
        DiagnosticOperation::BindBridge => "bind-bridge",
        DiagnosticOperation::RunBridge => "run-bridge",
        DiagnosticOperation::PrepareLaunch => "prepare-launch",
        DiagnosticOperation::StartHarness => "start-harness",
        DiagnosticOperation::WaitForHarness => "wait-for-harness",
        DiagnosticOperation::StopHarness => "stop-harness",
        DiagnosticOperation::DiscoverModels => "discover-models",
        _ => unreachable!(),
    }
}

const fn diagnostic_configuration_operation(operation: DiagnosticOperation) -> &'static str {
    match operation {
        DiagnosticOperation::ReadWorkingDirectory => "read-working-directory",
        DiagnosticOperation::ReadConfiguration => "read-configuration",
        DiagnosticOperation::WriteConfiguration => "write-configuration",
        DiagnosticOperation::RemoveConfiguration => "remove-configuration",
        DiagnosticOperation::ReadCredential => "read-credential",
        DiagnosticOperation::WriteCredential => "write-credential",
        DiagnosticOperation::RemoveCredential => "remove-credential",
        DiagnosticOperation::ConfigureTelemetry => "configure-telemetry",
        _ => unreachable!(),
    }
}

const fn diagnostic_update_operation(operation: DiagnosticOperation) -> &'static str {
    match operation {
        DiagnosticOperation::FetchUpdateManifest => "fetch-update-manifest",
        DiagnosticOperation::DownloadUpdate => "download-update",
        DiagnosticOperation::VerifyUpdate => "verify-update",
        DiagnosticOperation::ReplaceExecutable => "replace-executable",
        DiagnosticOperation::RemoveInstallation => "remove-installation",
        _ => unreachable!(),
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum IoErrorKind {
    NotFound,
    PermissionDenied,
    TimedOut,
    ConnectionRefused,
    ConnectionReset,
    ConnectionAborted,
    NotConnected,
    AddressInUse,
    AddressUnavailable,
    BrokenPipe,
    InvalidData,
    InvalidInput,
    UnexpectedEof,
    Other,
}

impl IoErrorKind {
    #[must_use]
    pub fn from_std(kind: std::io::ErrorKind) -> Self {
        match kind {
            std::io::ErrorKind::NotFound => Self::NotFound,
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            std::io::ErrorKind::TimedOut => Self::TimedOut,
            std::io::ErrorKind::ConnectionRefused => Self::ConnectionRefused,
            std::io::ErrorKind::ConnectionReset => Self::ConnectionReset,
            std::io::ErrorKind::ConnectionAborted => Self::ConnectionAborted,
            std::io::ErrorKind::NotConnected => Self::NotConnected,
            std::io::ErrorKind::AddrInUse => Self::AddressInUse,
            std::io::ErrorKind::AddrNotAvailable => Self::AddressUnavailable,
            std::io::ErrorKind::BrokenPipe => Self::BrokenPipe,
            std::io::ErrorKind::InvalidData => Self::InvalidData,
            std::io::ErrorKind::InvalidInput => Self::InvalidInput,
            std::io::ErrorKind::UnexpectedEof => Self::UnexpectedEof,
            _ => Self::Other,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not-found",
            Self::PermissionDenied => "permission-denied",
            Self::TimedOut => "timed-out",
            Self::ConnectionRefused => "connection-refused",
            Self::ConnectionReset => "connection-reset",
            Self::ConnectionAborted => "connection-aborted",
            Self::NotConnected => "not-connected",
            Self::AddressInUse => "address-in-use",
            Self::AddressUnavailable => "address-unavailable",
            Self::BrokenPipe => "broken-pipe",
            Self::InvalidData => "invalid-data",
            Self::InvalidInput => "invalid-input",
            Self::UnexpectedEof => "unexpected-eof",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum VersionComponent {
    Application,
    Harness,
    Runtime,
    ManifestSchema,
    StateSchema,
    UpdateCandidate,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DocumentKind {
    CompatibilityManifest,
    ModelCatalog,
    LaunchPlan,
    HarnessConfiguration,
    IntegrationState,
    CredentialReceipt,
    UpdateManifest,
    UpdateState,
    InstallationReceipt,
    TelemetrySettings,
}
