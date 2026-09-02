use super::context::{HarnessIdentitySource, enrich_telemetry_context};
use crate::app::Cli;
use nan_harness_runtime::{
    BridgeDiagnostic, BridgeDiagnosticReason, BridgeEndpoint as RuntimeBridgeEndpoint,
    BridgeModelPolicy as RuntimeModelPolicy, BridgeReasoningRequest as RuntimeReasoningRequest,
};
use nan_harness_telemetry::diagnostic::{
    BridgeEndpoint, Diagnostic, DiagnosticDetails, DiagnosticReason, ModelPolicy, ReasoningRequest,
};
use nan_harness_telemetry::event::{
    ErrorReportContext, Failure, FailureCategory, FailureCause, FailureStage,
};

pub(crate) fn bridge_diagnostic_contexts(
    diagnostics: &[BridgeDiagnostic],
    cli: &Cli,
    interactive: bool,
) -> Vec<ErrorReportContext> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            let (category, stage, cause, retryable) = bridge_diagnostic_classification(diagnostic);
            let mut failure = Failure::new(diagnostic.code.to_owned(), category, stage, retryable)
                .with_cause(cause);
            if let Some(status) = diagnostic.http_status {
                failure = failure.with_http_status(status);
            }
            enrich_telemetry_context(
                ErrorReportContext::new(failure, interactive)
                    .with_diagnostic(bridge_diagnostic(diagnostic)),
                cli,
                HarnessIdentitySource::KindOnly,
            )
        })
        .collect()
}

fn bridge_diagnostic(diagnostic: &BridgeDiagnostic) -> Diagnostic {
    let reason = match diagnostic.reason {
        BridgeDiagnosticReason::AuthenticationRejected => DiagnosticReason::AuthenticationRejected,
        BridgeDiagnosticReason::InvalidRequest => DiagnosticReason::InvalidRequest,
        BridgeDiagnosticReason::ReasoningPolicyMismatch => {
            DiagnosticReason::ReasoningPolicyMismatch
        }
        BridgeDiagnosticReason::UpstreamTransport => DiagnosticReason::NetworkRequestFailed,
        BridgeDiagnosticReason::UpstreamStatus => DiagnosticReason::HttpRequestRejected,
        BridgeDiagnosticReason::InvalidUpstreamResponse => DiagnosticReason::InvalidResponse,
    };
    Diagnostic::new(
        reason,
        DiagnosticDetails::Bridge {
            endpoint: bridge_endpoint(diagnostic.endpoint),
            model_id: diagnostic.model_id.clone(),
            requested_reasoning: diagnostic.requested_reasoning.map(reasoning_request),
            model_policy: diagnostic.model_policy.map(model_policy),
        },
    )
}

const fn bridge_endpoint(endpoint: RuntimeBridgeEndpoint) -> BridgeEndpoint {
    match endpoint {
        RuntimeBridgeEndpoint::Models => BridgeEndpoint::Models,
        RuntimeBridgeEndpoint::Messages => BridgeEndpoint::Messages,
        RuntimeBridgeEndpoint::CountTokens => BridgeEndpoint::CountTokens,
        RuntimeBridgeEndpoint::Responses => BridgeEndpoint::Responses,
        RuntimeBridgeEndpoint::Search => BridgeEndpoint::Search,
        RuntimeBridgeEndpoint::FxGateway => BridgeEndpoint::FxGateway,
    }
}

const fn reasoning_request(request: RuntimeReasoningRequest) -> ReasoningRequest {
    match request {
        RuntimeReasoningRequest::Auto => ReasoningRequest::Auto,
        RuntimeReasoningRequest::None => ReasoningRequest::None,
        RuntimeReasoningRequest::Low => ReasoningRequest::Low,
        RuntimeReasoningRequest::Medium => ReasoningRequest::Medium,
        RuntimeReasoningRequest::High => ReasoningRequest::High,
        RuntimeReasoningRequest::Xhigh => ReasoningRequest::Xhigh,
        RuntimeReasoningRequest::Other => ReasoningRequest::Other,
    }
}

const fn model_policy(policy: RuntimeModelPolicy) -> ModelPolicy {
    match policy {
        RuntimeModelPolicy::Unsupported => ModelPolicy::Unsupported,
        RuntimeModelPolicy::Toggle => ModelPolicy::Toggle,
        RuntimeModelPolicy::Effort => ModelPolicy::Effort,
        RuntimeModelPolicy::AlwaysOn => ModelPolicy::AlwaysOn,
        RuntimeModelPolicy::Unknown => ModelPolicy::Unknown,
    }
}

fn bridge_diagnostic_classification(
    diagnostic: &BridgeDiagnostic,
) -> (FailureCategory, FailureStage, FailureCause, bool) {
    let retryable_http = diagnostic
        .http_status
        .is_some_and(|status| matches!(status, 502..=504));
    match diagnostic.reason {
        BridgeDiagnosticReason::UpstreamTransport => (
            FailureCategory::Bridge,
            FailureStage::HarnessExecution,
            FailureCause::Network,
            true,
        ),
        BridgeDiagnosticReason::UpstreamStatus => (
            FailureCategory::Bridge,
            FailureStage::HarnessExecution,
            FailureCause::HttpStatus,
            retryable_http,
        ),
        BridgeDiagnosticReason::InvalidUpstreamResponse => (
            FailureCategory::Bridge,
            FailureStage::HarnessExecution,
            FailureCause::InvalidResponse,
            true,
        ),
        BridgeDiagnosticReason::AuthenticationRejected => (
            FailureCategory::Bridge,
            FailureStage::HarnessExecution,
            FailureCause::InvalidConfiguration,
            false,
        ),
        BridgeDiagnosticReason::InvalidRequest
        | BridgeDiagnosticReason::ReasoningPolicyMismatch => (
            FailureCategory::Bridge,
            FailureStage::HarnessExecution,
            FailureCause::InvalidData,
            false,
        ),
    }
}
