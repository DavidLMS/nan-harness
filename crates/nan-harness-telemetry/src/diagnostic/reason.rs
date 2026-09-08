use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticReason {
    Unclassified,
    LegacyReport,
    AuthenticationRejected,
    InvalidRequest,
    ReasoningPolicyMismatch,
    NetworkRequestFailed,
    UpstreamTimeout,
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
    DiscoveryProbeTimeout,
    DiscoveryProbeOutputLimit,
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
            Self::NetworkRequestFailed
            | Self::UpstreamTimeout
            | Self::HttpRequestRejected
            | Self::InvalidResponse => diagnostic_transport_reason(self),
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
            | Self::DiscoveryProbeTimeout
            | Self::DiscoveryProbeOutputLimit
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
        DiagnosticReason::UpstreamTimeout => "upstream-timeout",
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
        DiagnosticReason::DiscoveryProbeTimeout => "discovery-probe-timeout",
        DiagnosticReason::DiscoveryProbeOutputLimit => "discovery-probe-output-limit",
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

#[cfg(test)]
mod tests {
    use super::DiagnosticReason as Reason;
    use serde_json::Value;

    #[test]
    fn diagnostic_reason_strings_and_serde_cover_every_variant() {
        let cases = [
            (Reason::DiscoveryProbeTimeout, "discovery-probe-timeout"),
            (
                Reason::DiscoveryProbeOutputLimit,
                "discovery-probe-output-limit",
            ),
            (Reason::Unclassified, "unclassified"),
            (Reason::LegacyReport, "legacy-report"),
            (Reason::AuthenticationRejected, "authentication-rejected"),
            (Reason::InvalidRequest, "invalid-request"),
            (Reason::ReasoningPolicyMismatch, "reasoning-policy-mismatch"),
            (Reason::NetworkRequestFailed, "network-request-failed"),
            (Reason::UpstreamTimeout, "upstream-timeout"),
            (Reason::HttpRequestRejected, "http-request-rejected"),
            (Reason::InvalidResponse, "invalid-response"),
            (Reason::MissingExecutable, "missing-executable"),
            (Reason::InvalidExecutable, "invalid-executable"),
            (Reason::UnsupportedVersion, "unsupported-version"),
            (Reason::UnparseableVersion, "unparseable-version"),
            (Reason::InvalidManifest, "invalid-manifest"),
            (Reason::MissingManifestEntry, "missing-manifest-entry"),
            (Reason::ProcessStartFailed, "process-start-failed"),
            (Reason::ProcessExited, "process-exited"),
            (Reason::ProcessWaitFailed, "process-wait-failed"),
            (
                Reason::ProcessTerminationFailed,
                "process-termination-failed",
            ),
            (Reason::BridgeExited, "bridge-exited"),
            (Reason::InvalidLaunchPlan, "invalid-launch-plan"),
            (Reason::LaunchPreparationFailed, "launch-preparation-failed"),
            (Reason::SecretResolutionFailed, "secret-resolution-failed"),
            (Reason::RandomGenerationFailed, "random-generation-failed"),
            (
                Reason::FilesystemOperationFailed,
                "filesystem-operation-failed",
            ),
            (Reason::SerializationFailed, "serialization-failed"),
            (Reason::ConfigurationConflict, "configuration-conflict"),
            (Reason::InvalidConfiguration, "invalid-configuration"),
            (Reason::MissingDirectory, "missing-directory"),
            (Reason::ModelUnavailable, "model-unavailable"),
            (Reason::ModelCatalogEmpty, "model-catalog-empty"),
            (
                Reason::UpdateVerificationFailed,
                "update-verification-failed",
            ),
            (Reason::UpdateReplacementFailed, "update-replacement-failed"),
            (Reason::UserPromptFailed, "user-prompt-failed"),
            (Reason::InternalInvariant, "internal-invariant"),
        ];

        for (reason, expected) in cases {
            assert_eq!(reason.as_str(), expected);
            assert_eq!(
                serde_json::to_value(reason).expect("reason should serialize"),
                Value::String(expected.to_owned())
            );
            assert_eq!(
                serde_json::from_value::<Reason>(Value::String(expected.to_owned()))
                    .expect("reason should deserialize"),
                reason
            );
        }
    }
}
