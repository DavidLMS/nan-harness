use serde::{Deserialize, Serialize};

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
    use super::{
        BridgeEndpoint, DiagnosticDetails, DiagnosticOperation, DocumentKind, IoErrorKind,
        ModelPolicy, ReasoningRequest, VersionComponent,
    };
    use serde::Serialize;
    use serde::de::DeserializeOwned;
    use serde_json::{Value, json};
    use std::fmt::Debug;

    #[test]
    fn diagnostic_details_serialization_covers_every_variant() {
        let cases = [
            (DiagnosticDetails::General, json!({ "kind": "general" })),
            (
                DiagnosticDetails::Bridge {
                    endpoint: BridgeEndpoint::Responses,
                    model_id: Some("model-alpha".to_owned()),
                    requested_reasoning: Some(ReasoningRequest::Xhigh),
                    model_policy: Some(ModelPolicy::AlwaysOn),
                },
                json!({
                    "kind": "bridge",
                    "endpoint": "responses",
                    "modelId": "model-alpha",
                    "requestedReasoning": "xhigh",
                    "modelPolicy": "always-on"
                }),
            ),
            (
                DiagnosticDetails::Io {
                    operation: DiagnosticOperation::ReadConfiguration,
                    error_kind: IoErrorKind::PermissionDenied,
                },
                json!({
                    "kind": "io",
                    "operation": "read-configuration",
                    "errorKind": "permission-denied"
                }),
            ),
            (
                DiagnosticDetails::Process {
                    operation: DiagnosticOperation::StartHarness,
                    exit_code: Some(17),
                },
                json!({
                    "kind": "process",
                    "operation": "start-harness",
                    "exitCode": 17
                }),
            ),
            (
                DiagnosticDetails::Version {
                    component: VersionComponent::ManifestSchema,
                    detected: Some("2".to_owned()),
                    expected: Some("1".to_owned()),
                },
                json!({
                    "kind": "version",
                    "component": "manifest-schema",
                    "detected": "2",
                    "expected": "1"
                }),
            ),
            (
                DiagnosticDetails::Http {
                    operation: DiagnosticOperation::FetchUpdateManifest,
                    status: 503,
                },
                json!({
                    "kind": "http",
                    "operation": "fetch-update-manifest",
                    "status": 503
                }),
            ),
            (
                DiagnosticDetails::Schema {
                    document: DocumentKind::TelemetrySettings,
                    observed_version: Some(3),
                },
                json!({
                    "kind": "schema",
                    "document": "telemetry-settings",
                    "observedVersion": 3
                }),
            ),
        ];

        for (details, expected) in cases {
            assert_eq!(
                serde_json::to_value(&details).expect("details should serialize"),
                expected
            );
            assert_eq!(
                serde_json::from_value::<DiagnosticDetails>(expected)
                    .expect("details should deserialize"),
                details
            );
        }
    }

    #[test]
    fn diagnostic_details_omit_absent_optional_fields() {
        let cases = [
            (
                DiagnosticDetails::Bridge {
                    endpoint: BridgeEndpoint::Models,
                    model_id: None,
                    requested_reasoning: None,
                    model_policy: None,
                },
                json!({ "kind": "bridge", "endpoint": "models" }),
            ),
            (
                DiagnosticDetails::Process {
                    operation: DiagnosticOperation::StartHarness,
                    exit_code: None,
                },
                json!({ "kind": "process", "operation": "start-harness" }),
            ),
            (
                DiagnosticDetails::Version {
                    component: VersionComponent::Runtime,
                    detected: None,
                    expected: None,
                },
                json!({ "kind": "version", "component": "runtime" }),
            ),
            (
                DiagnosticDetails::Schema {
                    document: DocumentKind::LaunchPlan,
                    observed_version: None,
                },
                json!({ "kind": "schema", "document": "launch-plan" }),
            ),
        ];

        for (details, expected) in cases {
            assert_eq!(
                serde_json::to_value(details).expect("details should serialize"),
                expected
            );
        }
    }

    #[test]
    fn diagnostic_operation_strings_and_serde_cover_every_variant() {
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
            assert_string_contract(operation, expected);
        }
    }

    #[test]
    fn typed_detail_values_preserve_their_serialized_contracts() {
        for (value, expected) in [
            (BridgeEndpoint::Models, "models"),
            (BridgeEndpoint::Messages, "messages"),
            (BridgeEndpoint::CountTokens, "count-tokens"),
            (BridgeEndpoint::Responses, "responses"),
            (BridgeEndpoint::Search, "search"),
            (BridgeEndpoint::FxGateway, "fx-gateway"),
        ] {
            assert_eq!(value.as_str(), expected);
            assert_string_contract(value, expected);
        }
        for (value, expected) in [
            (ReasoningRequest::Auto, "auto"),
            (ReasoningRequest::None, "none"),
            (ReasoningRequest::Low, "low"),
            (ReasoningRequest::Medium, "medium"),
            (ReasoningRequest::High, "high"),
            (ReasoningRequest::Xhigh, "xhigh"),
            (ReasoningRequest::Other, "other"),
        ] {
            assert_eq!(value.as_str(), expected);
            assert_string_contract(value, expected);
        }
        for (value, expected) in [
            (ModelPolicy::Unsupported, "unsupported"),
            (ModelPolicy::Toggle, "toggle"),
            (ModelPolicy::Effort, "effort"),
            (ModelPolicy::AlwaysOn, "always-on"),
            (ModelPolicy::Unknown, "unknown"),
        ] {
            assert_eq!(value.as_str(), expected);
            assert_string_contract(value, expected);
        }
        for (value, expected) in [
            (IoErrorKind::NotFound, "not-found"),
            (IoErrorKind::PermissionDenied, "permission-denied"),
            (IoErrorKind::TimedOut, "timed-out"),
            (IoErrorKind::ConnectionRefused, "connection-refused"),
            (IoErrorKind::ConnectionReset, "connection-reset"),
            (IoErrorKind::ConnectionAborted, "connection-aborted"),
            (IoErrorKind::NotConnected, "not-connected"),
            (IoErrorKind::AddressInUse, "address-in-use"),
            (IoErrorKind::AddressUnavailable, "address-unavailable"),
            (IoErrorKind::BrokenPipe, "broken-pipe"),
            (IoErrorKind::InvalidData, "invalid-data"),
            (IoErrorKind::InvalidInput, "invalid-input"),
            (IoErrorKind::UnexpectedEof, "unexpected-eof"),
            (IoErrorKind::Other, "other"),
        ] {
            assert_eq!(value.as_str(), expected);
            assert_string_contract(value, expected);
        }
        for (value, expected) in [
            (VersionComponent::Application, "application"),
            (VersionComponent::Harness, "harness"),
            (VersionComponent::Runtime, "runtime"),
            (VersionComponent::ManifestSchema, "manifest-schema"),
            (VersionComponent::StateSchema, "state-schema"),
            (VersionComponent::UpdateCandidate, "update-candidate"),
        ] {
            assert_string_contract(value, expected);
        }
        for (value, expected) in [
            (
                DocumentKind::CompatibilityManifest,
                "compatibility-manifest",
            ),
            (DocumentKind::ModelCatalog, "model-catalog"),
            (DocumentKind::LaunchPlan, "launch-plan"),
            (DocumentKind::HarnessConfiguration, "harness-configuration"),
            (DocumentKind::IntegrationState, "integration-state"),
            (DocumentKind::CredentialReceipt, "credential-receipt"),
            (DocumentKind::UpdateManifest, "update-manifest"),
            (DocumentKind::UpdateState, "update-state"),
            (DocumentKind::InstallationReceipt, "installation-receipt"),
            (DocumentKind::TelemetrySettings, "telemetry-settings"),
        ] {
            assert_string_contract(value, expected);
        }
    }

    fn assert_string_contract<T>(value: T, expected: &str)
    where
        T: Copy + Debug + DeserializeOwned + Eq + Serialize,
    {
        assert_eq!(
            serde_json::to_value(value).expect("typed detail should serialize"),
            Value::String(expected.to_owned())
        );
        assert_eq!(
            serde_json::from_value::<T>(Value::String(expected.to_owned()))
                .expect("typed detail should deserialize"),
            value
        );
    }
}
