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
            Self::Unclassified => "unclassified",
            Self::LegacyReport => "legacy-report",
            Self::AuthenticationRejected => "authentication-rejected",
            Self::InvalidRequest => "invalid-request",
            Self::ReasoningPolicyMismatch => "reasoning-policy-mismatch",
            Self::NetworkRequestFailed => "network-request-failed",
            Self::HttpRequestRejected => "http-request-rejected",
            Self::InvalidResponse => "invalid-response",
            Self::MissingExecutable => "missing-executable",
            Self::InvalidExecutable => "invalid-executable",
            Self::UnsupportedVersion => "unsupported-version",
            Self::UnparseableVersion => "unparseable-version",
            Self::InvalidManifest => "invalid-manifest",
            Self::MissingManifestEntry => "missing-manifest-entry",
            Self::ProcessStartFailed => "process-start-failed",
            Self::ProcessExited => "process-exited",
            Self::ProcessWaitFailed => "process-wait-failed",
            Self::ProcessTerminationFailed => "process-termination-failed",
            Self::BridgeExited => "bridge-exited",
            Self::InvalidLaunchPlan => "invalid-launch-plan",
            Self::LaunchPreparationFailed => "launch-preparation-failed",
            Self::SecretResolutionFailed => "secret-resolution-failed",
            Self::RandomGenerationFailed => "random-generation-failed",
            Self::FilesystemOperationFailed => "filesystem-operation-failed",
            Self::SerializationFailed => "serialization-failed",
            Self::ConfigurationConflict => "configuration-conflict",
            Self::InvalidConfiguration => "invalid-configuration",
            Self::MissingDirectory => "missing-directory",
            Self::ModelUnavailable => "model-unavailable",
            Self::ModelCatalogEmpty => "model-catalog-empty",
            Self::UpdateVerificationFailed => "update-verification-failed",
            Self::UpdateReplacementFailed => "update-replacement-failed",
            Self::UserPromptFailed => "user-prompt-failed",
            Self::InternalInvariant => "internal-invariant",
        }
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
            Self::LoadCompatibilityManifest => "load-compatibility-manifest",
            Self::ReadWorkingDirectory => "read-working-directory",
            Self::ResolveExecutable => "resolve-executable",
            Self::RunVersionCommand => "run-version-command",
            Self::DownloadInstaller => "download-installer",
            Self::RunInstaller => "run-installer",
            Self::RunPostInstallCheck => "run-post-install-check",
            Self::BindBridge => "bind-bridge",
            Self::RunBridge => "run-bridge",
            Self::PrepareLaunch => "prepare-launch",
            Self::StartHarness => "start-harness",
            Self::WaitForHarness => "wait-for-harness",
            Self::StopHarness => "stop-harness",
            Self::DiscoverModels => "discover-models",
            Self::ReadConfiguration => "read-configuration",
            Self::WriteConfiguration => "write-configuration",
            Self::RemoveConfiguration => "remove-configuration",
            Self::ReadCredential => "read-credential",
            Self::WriteCredential => "write-credential",
            Self::RemoveCredential => "remove-credential",
            Self::FetchUpdateManifest => "fetch-update-manifest",
            Self::DownloadUpdate => "download-update",
            Self::VerifyUpdate => "verify-update",
            Self::ReplaceExecutable => "replace-executable",
            Self::RemoveInstallation => "remove-installation",
            Self::ConfigureTelemetry => "configure-telemetry",
        }
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
