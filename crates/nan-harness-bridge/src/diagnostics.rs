use crate::error::ApiError;

/// A structured error diagnostic emitted by the bridge when an API handler
/// fails. The supervisor is expected to attach these to the launch report so
/// the CLI can surface them through telemetry when enabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeDiagnostic {
    pub code: &'static str,
    pub kind: &'static str,
    pub message: String,
    pub http_status: Option<u16>,
    pub endpoint: Option<String>,
}

impl BridgeDiagnostic {
    pub(crate) fn from_api_error(error: &ApiError, endpoint: Option<String>) -> Self {
        let http_status = match error {
            ApiError::UpstreamStatus { status, .. } => Some(status.as_u16()),
            _ => Some(error.status().as_u16()),
        };
        Self {
            code: error.code(),
            kind: error.anthropic_type(),
            message: error.to_string(),
            http_status,
            endpoint,
        }
    }
}
