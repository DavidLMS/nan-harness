use super::context::{HarnessIdentitySource, enrich_telemetry_context};
use crate::app::Cli;
use nan_harness_runtime::{
    BridgeAttemptBucket as RuntimeAttemptBucket, BridgeDiagnostic, BridgeDiagnosticReason,
    BridgeEndpoint as RuntimeBridgeEndpoint, BridgeModelPolicy as RuntimeModelPolicy,
    BridgeReasoningRequest as RuntimeReasoningRequest,
    BridgeRecoveryOutcome as RuntimeRecoveryOutcome,
    BridgeRequestPriority as RuntimeRequestPriority, BridgeTimeoutPhase as RuntimeTimeoutPhase,
};
use nan_harness_telemetry::diagnostic::{
    AttemptBucket, BridgeEndpoint, Diagnostic, DiagnosticDetails, DiagnosticReason, ModelPolicy,
    ReasoningRequest, RecoveryOutcome, RequestPriority, TimeoutPhase,
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
        BridgeDiagnosticReason::UpstreamTimeout
        | BridgeDiagnosticReason::CoordinatorQueueTimeout => DiagnosticReason::UpstreamTimeout,
        BridgeDiagnosticReason::UpstreamStatus => DiagnosticReason::HttpRequestRejected,
        BridgeDiagnosticReason::InvalidUpstreamResponse => DiagnosticReason::InvalidResponse,
        BridgeDiagnosticReason::CoordinatorUnavailable => DiagnosticReason::UnsupportedVersion,
    };
    Diagnostic::new(
        reason,
        DiagnosticDetails::Bridge {
            endpoint: bridge_endpoint(diagnostic.endpoint),
            model_id: diagnostic.model_id.clone(),
            requested_reasoning: diagnostic.requested_reasoning.map(reasoning_request),
            model_policy: diagnostic.model_policy.map(model_policy),
            timeout_phase: diagnostic.timeout_phase.map(timeout_phase),
            recovery_outcome: diagnostic.recovery_outcome.map(recovery_outcome),
            attempt: diagnostic.attempt.map(attempt_bucket),
            priority: diagnostic.priority.map(request_priority),
            cache_replay_detected: diagnostic.cache_replay_detected,
            cache_bypass_attempted: diagnostic.cache_bypass_attempted,
        },
    )
}

const fn timeout_phase(phase: RuntimeTimeoutPhase) -> TimeoutPhase {
    match phase {
        RuntimeTimeoutPhase::InitialResponse => TimeoutPhase::InitialResponse,
        RuntimeTimeoutPhase::Inactivity => TimeoutPhase::Inactivity,
        RuntimeTimeoutPhase::CoordinatorQueue => TimeoutPhase::CoordinatorQueue,
    }
}

const fn recovery_outcome(outcome: RuntimeRecoveryOutcome) -> RecoveryOutcome {
    match outcome {
        RuntimeRecoveryOutcome::Retrying => RecoveryOutcome::Retrying,
        RuntimeRecoveryOutcome::Exhausted => RecoveryOutcome::Exhausted,
    }
}

const fn attempt_bucket(attempt: RuntimeAttemptBucket) -> AttemptBucket {
    match attempt {
        RuntimeAttemptBucket::First => AttemptBucket::First,
        RuntimeAttemptBucket::Second => AttemptBucket::Second,
        RuntimeAttemptBucket::Later => AttemptBucket::Later,
    }
}

const fn request_priority(priority: RuntimeRequestPriority) -> RequestPriority {
    match priority {
        RuntimeRequestPriority::Foreground => RequestPriority::Foreground,
        RuntimeRequestPriority::Background => RequestPriority::Background,
    }
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
        .is_some_and(|status| matches!(status, 408 | 425 | 429 | 500 | 502..=504));
    match diagnostic.reason {
        BridgeDiagnosticReason::UpstreamTransport => (
            FailureCategory::Provider,
            FailureStage::HarnessExecution,
            FailureCause::Network,
            true,
        ),
        BridgeDiagnosticReason::UpstreamTimeout => (
            FailureCategory::Provider,
            FailureStage::HarnessExecution,
            FailureCause::Timeout,
            true,
        ),
        BridgeDiagnosticReason::CoordinatorQueueTimeout => (
            FailureCategory::Bridge,
            FailureStage::HarnessExecution,
            FailureCause::Timeout,
            true,
        ),
        BridgeDiagnosticReason::UpstreamStatus => (
            FailureCategory::Provider,
            FailureStage::HarnessExecution,
            FailureCause::HttpStatus,
            retryable_http,
        ),
        BridgeDiagnosticReason::InvalidUpstreamResponse => (
            FailureCategory::Provider,
            FailureStage::HarnessExecution,
            FailureCause::InvalidResponse,
            true,
        ),
        BridgeDiagnosticReason::AuthenticationRejected
        | BridgeDiagnosticReason::CoordinatorUnavailable => (
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
