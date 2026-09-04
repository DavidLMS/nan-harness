use crate::error::ApiError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeTimeoutPhase {
    InitialResponse,
    Inactivity,
    CoordinatorQueue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeRecoveryOutcome {
    Retrying,
    Exhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeAttemptBucket {
    First,
    Second,
    Later,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeRequestPriority {
    Foreground,
    Background,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeDiagnosticReason {
    AuthenticationRejected,
    InvalidRequest,
    ReasoningPolicyMismatch,
    UpstreamTransport,
    UpstreamTimeout,
    UpstreamStatus,
    InvalidUpstreamResponse,
    CoordinatorUnavailable,
    CoordinatorQueueTimeout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeEndpoint {
    Models,
    Messages,
    CountTokens,
    Responses,
    Search,
    FxGateway,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeReasoningRequest {
    Auto,
    None,
    Low,
    Medium,
    High,
    Xhigh,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeModelPolicy {
    Unsupported,
    Toggle,
    Effort,
    AlwaysOn,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeDiagnostic {
    pub code: &'static str,
    pub reason: BridgeDiagnosticReason,
    pub http_status: Option<u16>,
    pub endpoint: BridgeEndpoint,
    pub model_id: Option<String>,
    pub requested_reasoning: Option<BridgeReasoningRequest>,
    pub model_policy: Option<BridgeModelPolicy>,
    pub timeout_phase: Option<BridgeTimeoutPhase>,
    pub recovery_outcome: Option<BridgeRecoveryOutcome>,
    pub attempt: Option<BridgeAttemptBucket>,
    pub priority: Option<BridgeRequestPriority>,
    pub cache_replay_detected: Option<bool>,
    pub cache_bypass_attempted: Option<bool>,
}

impl BridgeDiagnostic {
    pub(crate) fn from_api_error(error: &ApiError, endpoint: BridgeEndpoint) -> Self {
        let (reason, model_id, requested_reasoning, model_policy) = match error {
            ApiError::Unauthorized => (
                BridgeDiagnosticReason::AuthenticationRejected,
                None,
                None,
                None,
            ),
            ApiError::InvalidRequest(_) | ApiError::SearchDisabled => {
                (BridgeDiagnosticReason::InvalidRequest, None, None, None)
            }
            ApiError::ReasoningPolicyMismatch {
                model_id,
                requested,
                policy,
                ..
            } => (
                BridgeDiagnosticReason::ReasoningPolicyMismatch,
                Some(model_id.clone()),
                Some(*requested),
                Some(*policy),
            ),
            ApiError::UpstreamTransport(_) => {
                (BridgeDiagnosticReason::UpstreamTransport, None, None, None)
            }
            ApiError::UpstreamTimeout(_) => {
                (BridgeDiagnosticReason::UpstreamTimeout, None, None, None)
            }
            ApiError::UpstreamStatus { .. } => {
                (BridgeDiagnosticReason::UpstreamStatus, None, None, None)
            }
            ApiError::InvalidUpstream(_) => (
                BridgeDiagnosticReason::InvalidUpstreamResponse,
                None,
                None,
                None,
            ),
            ApiError::CoordinatorUnavailable(_) => (
                BridgeDiagnosticReason::CoordinatorUnavailable,
                None,
                None,
                None,
            ),
            ApiError::CoordinatorQueueTimeout => (
                BridgeDiagnosticReason::CoordinatorQueueTimeout,
                None,
                None,
                None,
            ),
        };
        let http_status = match error {
            ApiError::UpstreamStatus { status, .. } => Some(status.as_u16()),
            ApiError::Unauthorized
            | ApiError::InvalidRequest(_)
            | ApiError::SearchDisabled
            | ApiError::ReasoningPolicyMismatch { .. }
            | ApiError::UpstreamTransport(_)
            | ApiError::UpstreamTimeout(_)
            | ApiError::InvalidUpstream(_)
            | ApiError::CoordinatorUnavailable(_)
            | ApiError::CoordinatorQueueTimeout => None,
        };
        Self {
            code: error.code(),
            reason,
            http_status,
            endpoint,
            model_id,
            requested_reasoning,
            model_policy,
            timeout_phase: match error {
                ApiError::UpstreamTimeout(crate::error::UpstreamTimeoutPhase::InitialResponse) => {
                    Some(BridgeTimeoutPhase::InitialResponse)
                }
                ApiError::UpstreamTimeout(crate::error::UpstreamTimeoutPhase::Inactivity) => {
                    Some(BridgeTimeoutPhase::Inactivity)
                }
                ApiError::CoordinatorQueueTimeout => Some(BridgeTimeoutPhase::CoordinatorQueue),
                _ => None,
            },
            recovery_outcome: None,
            attempt: None,
            priority: None,
            cache_replay_detected: None,
            cache_bypass_attempted: None,
        }
    }

    pub(crate) fn with_recovery(
        mut self,
        outcome: BridgeRecoveryOutcome,
        attempt: BridgeAttemptBucket,
        priority: BridgeRequestPriority,
    ) -> Self {
        self.recovery_outcome = Some(outcome);
        self.attempt = Some(attempt);
        self.priority = Some(priority);
        self
    }

    pub(crate) fn with_cache_recovery(mut self, replay_detected: bool, bypass: bool) -> Self {
        self.cache_replay_detected = replay_detected.then_some(true);
        self.cache_bypass_attempted = bypass.then_some(true);
        self
    }
}
