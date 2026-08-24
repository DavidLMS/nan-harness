use crate::error::ApiError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeDiagnosticReason {
    AuthenticationRejected,
    InvalidRequest,
    ReasoningPolicyMismatch,
    UpstreamTransport,
    UpstreamStatus,
    InvalidUpstreamResponse,
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
            ApiError::InvalidRequest(_) => {
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
            ApiError::UpstreamTransport(_) | ApiError::UpstreamTimeout(_) => {
                (BridgeDiagnosticReason::UpstreamTransport, None, None, None)
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
        };
        let http_status = match error {
            ApiError::UpstreamStatus { status, .. } => Some(status.as_u16()),
            ApiError::Unauthorized
            | ApiError::InvalidRequest(_)
            | ApiError::ReasoningPolicyMismatch { .. }
            | ApiError::UpstreamTransport(_)
            | ApiError::UpstreamTimeout(_)
            | ApiError::InvalidUpstream(_) => None,
        };
        Self {
            code: error.code(),
            reason,
            http_status,
            endpoint,
            model_id,
            requested_reasoning,
            model_policy,
        }
    }
}
