use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Diagnostic {
    reason: DiagnosticReason,
    details: DiagnosticDetails,
}

impl Diagnostic {
    #[must_use]
    pub const fn new(reason: DiagnosticReason, details: DiagnosticDetails) -> Self {
        Self { reason, details }
    }

    #[must_use]
    pub const fn general(reason: DiagnosticReason) -> Self {
        Self::new(reason, DiagnosticDetails::General)
    }

    #[must_use]
    pub const fn unclassified() -> Self {
        Self::general(DiagnosticReason::Unclassified)
    }

    #[must_use]
    pub const fn legacy() -> Self {
        Self::general(DiagnosticReason::LegacyReport)
    }

    #[must_use]
    pub const fn reason(&self) -> DiagnosticReason {
        self.reason
    }

    #[must_use]
    pub const fn details(&self) -> &DiagnosticDetails {
        &self.details
    }
}

impl Default for Diagnostic {
    fn default() -> Self {
        Self::unclassified()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticReason {
    Unclassified,
    LegacyReport,
    AuthenticationRejected,
    InvalidRequest,
    ReasoningPolicyMismatch,
    NetworkRequestFailed,
    HttpRequestRejected,
    InvalidResponse,
    MissingExecutable,
    InvalidExecutable,
    UnsupportedVersion,
    UnparseableVersion,
    InvalidManifest,
    MissingManifestEntry,
    ProcessStartFailed,
    ProcessExited,
    ProcessWaitFailed,
    ProcessTerminationFailed,
    BridgeExited,
    InvalidLaunchPlan,
    LaunchPreparationFailed,
    SecretResolutionFailed,
    RandomGenerationFailed,
    FilesystemOperationFailed,
    SerializationFailed,
    ConfigurationConflict,
    InvalidConfiguration,
    MissingDirectory,
    ModelUnavailable,
    ModelCatalogEmpty,
    UpdateVerificationFailed,
    UpdateReplacementFailed,
    UserPromptFailed,
    InternalInvariant,
}

impl DiagnosticReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unclassified | Self::LegacyReport => diagnostic_lifecycle_reason(self),
            Self::AuthenticationRejected | Self::InvalidRequest | Self::ReasoningPolicyMismatch => {
                diagnostic_provider_reason(self)
            }
            Self::NetworkRequestFailed | Self::HttpRequestRejected | Self::InvalidResponse => {
                diagnostic_transport_reason(self)
            }
            Self::MissingExecutable
            | Self::InvalidExecutable
            | Self::UnsupportedVersion
            | Self::UnparseableVersion => diagnostic_executable_reason(self),
            Self::InvalidManifest
            | Self::MissingManifestEntry
            | Self::ProcessStartFailed
            | Self::ProcessExited
            | Self::ProcessWaitFailed
            | Self::ProcessTerminationFailed
            | Self::BridgeExited => diagnostic_runtime_reason(self),
            Self::InvalidLaunchPlan
            | Self::LaunchPreparationFailed
            | Self::SecretResolutionFailed
            | Self::RandomGenerationFailed => diagnostic_launch_reason(self),
            Self::FilesystemOperationFailed
            | Self::SerializationFailed
            | Self::ConfigurationConflict
            | Self::InvalidConfiguration
            | Self::MissingDirectory => diagnostic_configuration_reason(self),
            Self::ModelUnavailable | Self::ModelCatalogEmpty => diagnostic_model_reason(self),
            Self::UpdateVerificationFailed
            | Self::UpdateReplacementFailed
            | Self::UserPromptFailed => diagnostic_update_reason(self),
            Self::InternalInvariant => "internal-invariant",
        }
    }
}

const fn diagnostic_lifecycle_reason(reason: DiagnosticReason) -> &'static str {
    match reason {
        DiagnosticReason::Unclassified => "unclassified",
        DiagnosticReason::LegacyReport => "legacy-report",
        _ => unreachable!(),
    }
}

const fn diagnostic_provider_reason(reason: DiagnosticReason) -> &'static str {
    match reason {
        DiagnosticReason::AuthenticationRejected => "authentication-rejected",
        DiagnosticReason::InvalidRequest => "invalid-request",
        DiagnosticReason::ReasoningPolicyMismatch => "reasoning-policy-mismatch",
        _ => unreachable!(),
    }
}

const fn diagnostic_transport_reason(reason: DiagnosticReason) -> &'static str {
    match reason {
        DiagnosticReason::NetworkRequestFailed => "network-request-failed",
        DiagnosticReason::HttpRequestRejected => "http-request-rejected",
        DiagnosticReason::InvalidResponse => "invalid-response",
        _ => unreachable!(),
    }
}

const fn diagnostic_executable_reason(reason: DiagnosticReason) -> &'static str {
    match reason {
        DiagnosticReason::MissingExecutable => "missing-executable",
        DiagnosticReason::InvalidExecutable => "invalid-executable",
        DiagnosticReason::UnsupportedVersion => "unsupported-version",
        DiagnosticReason::UnparseableVersion => "unparseable-version",
        _ => unreachable!(),
    }
}

const fn diagnostic_runtime_reason(reason: DiagnosticReason) -> &'static str {
    match reason {
        DiagnosticReason::InvalidManifest => "invalid-manifest",
        DiagnosticReason::MissingManifestEntry => "missing-manifest-entry",
        DiagnosticReason::ProcessStartFailed => "process-start-failed",
        DiagnosticReason::ProcessExited => "process-exited",
        DiagnosticReason::ProcessWaitFailed => "process-wait-failed",
        DiagnosticReason::ProcessTerminationFailed => "process-termination-failed",
        DiagnosticReason::BridgeExited => "bridge-exited",
        _ => unreachable!(),
    }
}

const fn diagnostic_launch_reason(reason: DiagnosticReason) -> &'static str {
    match reason {
        DiagnosticReason::InvalidLaunchPlan => "invalid-launch-plan",
        DiagnosticReason::LaunchPreparationFailed => "launch-preparation-failed",
        DiagnosticReason::SecretResolutionFailed => "secret-resolution-failed",
        DiagnosticReason::RandomGenerationFailed => "random-generation-failed",
        _ => unreachable!(),
    }
}

const fn diagnostic_configuration_reason(reason: DiagnosticReason) -> &'static str {
    match reason {
        DiagnosticReason::FilesystemOperationFailed => "filesystem-operation-failed",
        DiagnosticReason::SerializationFailed => "serialization-failed",
        DiagnosticReason::ConfigurationConflict => "configuration-conflict",
        DiagnosticReason::InvalidConfiguration => "invalid-configuration",
        DiagnosticReason::MissingDirectory => "missing-directory",
        _ => unreachable!(),
    }
}

const fn diagnostic_model_reason(reason: DiagnosticReason) -> &'static str {
    match reason {
        DiagnosticReason::ModelUnavailable => "model-unavailable",
        DiagnosticReason::ModelCatalogEmpty => "model-catalog-empty",
        _ => unreachable!(),
    }
}

const fn diagnostic_update_reason(reason: DiagnosticReason) -> &'static str {
    match reason {
        DiagnosticReason::UpdateVerificationFailed => "update-verification-failed",
        DiagnosticReason::UpdateReplacementFailed => "update-replacement-failed",
        DiagnosticReason::UserPromptFailed => "user-prompt-failed",
        _ => unreachable!(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DiagnosticDetails {
    General,
    Bridge {
        endpoint: BridgeEndpoint,
        #[serde(skip_serializing_if = "Option::is_none")]
        model_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        requested_reasoning: Option<ReasoningRequest>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model_policy: Option<ModelPolicy>,
    },
    Io {
        operation: DiagnosticOperation,
        error_kind: IoErrorKind,
    },
    Process {
        operation: DiagnosticOperation,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
    },
    Version {
        component: VersionComponent,
        #[serde(skip_serializing_if = "Option::is_none")]
        detected: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        expected: Option<String>,
    },
    Http {
        operation: DiagnosticOperation,
        status: u16,
    },
    Schema {
        document: DocumentKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        observed_version: Option<u16>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BridgeEndpoint {
    Models,
    Messages,
    CountTokens,
    Responses,
    Search,
    FxGateway,
}

impl BridgeEndpoint {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Models => "models",
            Self::Messages => "messages",
            Self::CountTokens => "count-tokens",
            Self::Responses => "responses",
            Self::Search => "search",
            Self::FxGateway => "fx-gateway",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningRequest {
    Auto,
    None,
    Low,
    Medium,
    High,
    Xhigh,
    Other,
}

impl ReasoningRequest {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ModelPolicy {
    Unsupported,
    Toggle,
    Effort,
    AlwaysOn,
    Unknown,
}

impl ModelPolicy {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::Toggle => "toggle",
            Self::Effort => "effort",
            Self::AlwaysOn => "always-on",
            Self::Unknown => "unknown",
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::{DiagnosticOperation, DiagnosticReason};

    #[test]
    fn diagnostic_reason_strings_cover_every_variant() {
        let cases = [
            (DiagnosticReason::Unclassified, "unclassified"),
            (DiagnosticReason::LegacyReport, "legacy-report"),
            (
                DiagnosticReason::AuthenticationRejected,
                "authentication-rejected",
            ),
            (DiagnosticReason::InvalidRequest, "invalid-request"),
            (
                DiagnosticReason::ReasoningPolicyMismatch,
                "reasoning-policy-mismatch",
            ),
            (
                DiagnosticReason::NetworkRequestFailed,
                "network-request-failed",
            ),
            (
                DiagnosticReason::HttpRequestRejected,
                "http-request-rejected",
            ),
            (DiagnosticReason::InvalidResponse, "invalid-response"),
            (DiagnosticReason::MissingExecutable, "missing-executable"),
            (DiagnosticReason::InvalidExecutable, "invalid-executable"),
            (DiagnosticReason::UnsupportedVersion, "unsupported-version"),
            (DiagnosticReason::UnparseableVersion, "unparseable-version"),
            (DiagnosticReason::InvalidManifest, "invalid-manifest"),
            (
                DiagnosticReason::MissingManifestEntry,
                "missing-manifest-entry",
            ),
            (DiagnosticReason::ProcessStartFailed, "process-start-failed"),
            (DiagnosticReason::ProcessExited, "process-exited"),
            (DiagnosticReason::ProcessWaitFailed, "process-wait-failed"),
            (
                DiagnosticReason::ProcessTerminationFailed,
                "process-termination-failed",
            ),
            (DiagnosticReason::BridgeExited, "bridge-exited"),
            (DiagnosticReason::InvalidLaunchPlan, "invalid-launch-plan"),
            (
                DiagnosticReason::LaunchPreparationFailed,
                "launch-preparation-failed",
            ),
            (
                DiagnosticReason::SecretResolutionFailed,
                "secret-resolution-failed",
            ),
            (
                DiagnosticReason::RandomGenerationFailed,
                "random-generation-failed",
            ),
            (
                DiagnosticReason::FilesystemOperationFailed,
                "filesystem-operation-failed",
            ),
            (
                DiagnosticReason::SerializationFailed,
                "serialization-failed",
            ),
            (
                DiagnosticReason::ConfigurationConflict,
                "configuration-conflict",
            ),
            (
                DiagnosticReason::InvalidConfiguration,
                "invalid-configuration",
            ),
            (DiagnosticReason::MissingDirectory, "missing-directory"),
            (DiagnosticReason::ModelUnavailable, "model-unavailable"),
            (DiagnosticReason::ModelCatalogEmpty, "model-catalog-empty"),
            (
                DiagnosticReason::UpdateVerificationFailed,
                "update-verification-failed",
            ),
            (
                DiagnosticReason::UpdateReplacementFailed,
                "update-replacement-failed",
            ),
            (DiagnosticReason::UserPromptFailed, "user-prompt-failed"),
            (DiagnosticReason::InternalInvariant, "internal-invariant"),
        ];
        for (reason, expected) in cases {
            assert_eq!(reason.as_str(), expected);
        }
    }

    #[test]
    fn diagnostic_operation_strings_cover_every_variant() {
        let cases = [
            (
                DiagnosticOperation::LoadCompatibilityManifest,
                "load-compatibility-manifest",
            ),
            (
                DiagnosticOperation::ReadWorkingDirectory,
                "read-working-directory",
            ),
            (DiagnosticOperation::ResolveExecutable, "resolve-executable"),
            (
                DiagnosticOperation::RunVersionCommand,
                "run-version-command",
            ),
            (DiagnosticOperation::DownloadInstaller, "download-installer"),
            (DiagnosticOperation::RunInstaller, "run-installer"),
            (
                DiagnosticOperation::RunPostInstallCheck,
                "run-post-install-check",
            ),
            (DiagnosticOperation::BindBridge, "bind-bridge"),
            (DiagnosticOperation::RunBridge, "run-bridge"),
            (DiagnosticOperation::PrepareLaunch, "prepare-launch"),
            (DiagnosticOperation::StartHarness, "start-harness"),
            (DiagnosticOperation::WaitForHarness, "wait-for-harness"),
            (DiagnosticOperation::StopHarness, "stop-harness"),
            (DiagnosticOperation::DiscoverModels, "discover-models"),
            (DiagnosticOperation::ReadConfiguration, "read-configuration"),
            (
                DiagnosticOperation::WriteConfiguration,
                "write-configuration",
            ),
            (
                DiagnosticOperation::RemoveConfiguration,
                "remove-configuration",
            ),
            (DiagnosticOperation::ReadCredential, "read-credential"),
            (DiagnosticOperation::WriteCredential, "write-credential"),
            (DiagnosticOperation::RemoveCredential, "remove-credential"),
            (
                DiagnosticOperation::FetchUpdateManifest,
                "fetch-update-manifest",
            ),
            (DiagnosticOperation::DownloadUpdate, "download-update"),
            (DiagnosticOperation::VerifyUpdate, "verify-update"),
            (DiagnosticOperation::ReplaceExecutable, "replace-executable"),
            (
                DiagnosticOperation::RemoveInstallation,
                "remove-installation",
            ),
            (
                DiagnosticOperation::ConfigureTelemetry,
                "configure-telemetry",
            ),
        ];
        for (operation, expected) in cases {
            assert_eq!(operation.as_str(), expected);
        }
    }
}
