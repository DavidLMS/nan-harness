mod bridge;
mod operation;
mod schema;

pub use bridge::{BridgeEndpoint, ModelPolicy, ReasoningRequest};
pub use operation::{DiagnosticOperation, DocumentKind, IoErrorKind, VersionComponent};
pub use schema::DiagnosticDetails;

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
