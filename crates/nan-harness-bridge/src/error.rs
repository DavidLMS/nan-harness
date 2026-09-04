use crate::diagnostics::{BridgeModelPolicy, BridgeReasoningRequest};
use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use std::fmt;
use std::net::SocketAddr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("could not read the bridge listener address: {0}")]
    ListenerAddress(std::io::Error),
    #[error("bridge listener must use loopback, received {0}")]
    NonLoopbackAddress(SocketAddr),
    #[error("could not build the NaN HTTP client: {0}")]
    BuildClient(reqwest::Error),
    #[error(transparent)]
    Coordinator(#[from] nan_harness_coordinator::CoordinatorError),
    #[error("could not discover models from NaN: {0}")]
    ModelDiscoveryTransport(reqwest::Error),
    #[error("NaN model discovery returned HTTP {status}: {message}")]
    ModelDiscoveryStatus { status: StatusCode, message: String },
    #[error("NaN returned a model catalog larger than the supported limit")]
    ModelDiscoveryTooLarge,
    #[error("NaN returned an invalid model catalog: {0}")]
    InvalidModelDiscoveryResponse(serde_json::Error),
    #[error("this credential has no compatible conversational models")]
    NoCompatibleModels,
    #[error(
        "model '{model}' is not available for this credential; {details}",
        details = AvailableModels(.available)
    )]
    SelectedModelUnavailable {
        model: String,
        available: Vec<String>,
    },
    #[error("bridge server failed: {0}")]
    Serve(std::io::Error),
    #[error("bridge task failed: {0}")]
    TaskJoin(tokio::task::JoinError),
}

struct AvailableModels<'a>(&'a [String]);

impl fmt::Display for AvailableModels<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return formatter.write_str("no models are available");
        }
        formatter.write_str("available models: ")?;
        for (index, model) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str(", ")?;
            }
            write!(formatter, "'{model}'")?;
        }
        Ok(())
    }
}

impl BridgeError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ListenerAddress(_) | Self::NonLoopbackAddress(_) => "NH-BRIDGE-001",
            Self::BuildClient(_) => "NH-BRIDGE-002",
            Self::Coordinator(_) => "NH-BRIDGE-006",
            Self::Serve(_) | Self::TaskJoin(_) => "NH-BRIDGE-003",
            Self::ModelDiscoveryTransport(_)
            | Self::ModelDiscoveryStatus { .. }
            | Self::ModelDiscoveryTooLarge
            | Self::InvalidModelDiscoveryResponse(_) => "NH-BRIDGE-004",
            Self::NoCompatibleModels | Self::SelectedModelUnavailable { .. } => "NH-BRIDGE-005",
        }
    }

    #[must_use]
    pub fn http_status(&self) -> Option<u16> {
        match self {
            Self::ModelDiscoveryStatus { status, .. } => Some(status.as_u16()),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::ModelDiscoveryTransport(error) if error.is_timeout())
    }

    #[must_use]
    pub const fn is_invalid_response(&self) -> bool {
        matches!(
            self,
            Self::ModelDiscoveryTooLarge | Self::InvalidModelDiscoveryResponse(_)
        )
    }
}

#[derive(Debug, Error)]
pub(crate) enum ApiError {
    #[error("local bridge authentication failed")]
    Unauthorized,
    #[error("invalid bridge request: {0}")]
    InvalidRequest(String),
    #[error("NaN web search is disabled for this launch")]
    SearchDisabled,
    #[error("invalid bridge request: {message}")]
    ReasoningPolicyMismatch {
        model_id: String,
        requested: BridgeReasoningRequest,
        policy: BridgeModelPolicy,
        message: String,
    },
    #[error("NaN request failed before a response was received")]
    UpstreamTransport(#[source] reqwest::Error),
    #[error("NaN upstream request timed out during {0}")]
    UpstreamTimeout(UpstreamTimeoutPhase),
    #[error("NaN returned HTTP {status}: {message}")]
    UpstreamStatus { status: StatusCode, message: String },
    #[error("NaN returned an invalid response: {0}")]
    InvalidUpstream(String),
    #[error("local request coordination is unavailable: {0}")]
    CoordinatorUnavailable(String),
    #[error("timed out waiting for coordinated provider capacity")]
    CoordinatorQueueTimeout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpstreamTimeoutPhase {
    InitialResponse,
    Inactivity,
}

impl fmt::Display for UpstreamTimeoutPhase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InitialResponse => "the initial response",
            Self::Inactivity => "an inactive response stream",
        })
    }
}

impl ApiError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized => "NH-BRIDGE-101",
            Self::InvalidRequest(_) | Self::ReasoningPolicyMismatch { .. } => "NH-BRIDGE-102",
            Self::SearchDisabled => "NH-BRIDGE-106",
            Self::UpstreamTransport(_) | Self::UpstreamTimeout(_) => "NH-BRIDGE-103",
            Self::UpstreamStatus { .. } => "NH-BRIDGE-104",
            Self::InvalidUpstream(_) => "NH-BRIDGE-105",
            Self::CoordinatorUnavailable(_) => "NH-BRIDGE-107",
            Self::CoordinatorQueueTimeout => "NH-BRIDGE-108",
        }
    }

    pub(crate) fn status(&self) -> StatusCode {
        match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::InvalidRequest(_) | Self::ReasoningPolicyMismatch { .. } => {
                StatusCode::BAD_REQUEST
            }
            Self::SearchDisabled => StatusCode::NOT_FOUND,
            Self::UpstreamTimeout(_) => StatusCode::GATEWAY_TIMEOUT,
            Self::CoordinatorUnavailable(_) | Self::CoordinatorQueueTimeout => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            Self::UpstreamStatus { status, .. } if status.as_u16() == 429 => {
                StatusCode::TOO_MANY_REQUESTS
            }
            Self::UpstreamStatus { status, .. } if status.is_client_error() => {
                StatusCode::BAD_REQUEST
            }
            Self::UpstreamTransport(_) | Self::InvalidUpstream(_) | Self::UpstreamStatus { .. } => {
                StatusCode::BAD_GATEWAY
            }
        }
    }

    pub(crate) const fn anthropic_type(&self) -> &'static str {
        match self {
            Self::Unauthorized => "authentication_error",
            Self::InvalidRequest(_) | Self::ReasoningPolicyMismatch { .. } => {
                "invalid_request_error"
            }
            Self::SearchDisabled => "not_found_error",
            Self::UpstreamStatus { status, .. } if status.as_u16() == 429 => "rate_limit_error",
            Self::CoordinatorUnavailable(_)
            | Self::CoordinatorQueueTimeout
            | Self::UpstreamTransport(_)
            | Self::UpstreamTimeout(_)
            | Self::UpstreamStatus { .. }
            | Self::InvalidUpstream(_) => "api_error",
        }
    }

    pub(crate) fn event_data(&self) -> serde_json::Value {
        json!({
            "type": "error",
            "error": {
                "type": self.anthropic_type(),
                "message": format!("{} [{}]", self, self.code())
            }
        })
    }
}

impl From<nan_harness_coordinator::CoordinatorError> for ApiError {
    fn from(error: nan_harness_coordinator::CoordinatorError) -> Self {
        if matches!(
            error,
            nan_harness_coordinator::CoordinatorError::QueueTimeout
        ) {
            Self::CoordinatorQueueTimeout
        } else {
            Self::CoordinatorUnavailable(error.to_string())
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status(), Json(self.event_data())).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::{ApiError, BridgeError};
    use nan_harness_coordinator::CoordinatorError;

    #[test]
    fn unavailable_model_display_is_stable_for_every_catalog_shape() {
        let cases = [
            (
                Vec::new(),
                "model 'old-model' is not available for this credential; no models are available",
            ),
            (
                vec!["glm5.3-flash".to_owned()],
                "model 'old-model' is not available for this credential; available models: 'glm5.3-flash'",
            ),
            (
                vec!["glm5.3-flash".to_owned(), "qwen3.6".to_owned()],
                "model 'old-model' is not available for this credential; available models: 'glm5.3-flash', 'qwen3.6'",
            ),
        ];
        for (available, expected) in cases {
            assert_eq!(
                BridgeError::SelectedModelUnavailable {
                    model: "old-model".to_owned(),
                    available,
                }
                .to_string(),
                expected
            );
        }
    }

    #[test]
    fn coordinator_queue_timeout_has_a_distinct_bridge_contract() {
        let error = ApiError::from(CoordinatorError::QueueTimeout);
        assert!(matches!(error, ApiError::CoordinatorQueueTimeout));
        assert_eq!(error.code(), "NH-BRIDGE-108");
        assert_eq!(error.status(), axum::http::StatusCode::SERVICE_UNAVAILABLE);
    }
}
