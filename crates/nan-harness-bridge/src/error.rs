use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
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
    #[error("could not discover models from NaN: {0}")]
    ModelDiscoveryTransport(reqwest::Error),
    #[error("NaN model discovery returned HTTP {status}: {message}")]
    ModelDiscoveryStatus { status: StatusCode, message: String },
    #[error("NaN returned an invalid model catalog: {0}")]
    InvalidModelDiscoveryResponse(reqwest::Error),
    #[error("this credential has no models compatible with Claude Code tools")]
    NoCompatibleModels,
    #[error("model '{model}' is not available for this credential; choose one of: {available:?}")]
    SelectedModelUnavailable {
        model: String,
        available: Vec<String>,
    },
    #[error("bridge server failed: {0}")]
    Serve(std::io::Error),
    #[error("bridge task failed: {0}")]
    TaskJoin(tokio::task::JoinError),
}

impl BridgeError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ListenerAddress(_) | Self::NonLoopbackAddress(_) => "NH-BRIDGE-001",
            Self::BuildClient(_) => "NH-BRIDGE-002",
            Self::Serve(_) | Self::TaskJoin(_) => "NH-BRIDGE-003",
            Self::ModelDiscoveryTransport(_)
            | Self::ModelDiscoveryStatus { .. }
            | Self::InvalidModelDiscoveryResponse(_) => "NH-BRIDGE-004",
            Self::NoCompatibleModels | Self::SelectedModelUnavailable { .. } => "NH-BRIDGE-005",
        }
    }
}

#[derive(Debug, Error)]
pub(crate) enum ApiError {
    #[error("local bridge authentication failed")]
    Unauthorized,
    #[error("invalid Anthropic Messages request: {0}")]
    InvalidRequest(String),
    #[error("NaN request failed before a response was received")]
    UpstreamTransport(#[source] reqwest::Error),
    #[error("NaN returned HTTP {status}: {message}")]
    UpstreamStatus { status: StatusCode, message: String },
    #[error("NaN returned an invalid response: {0}")]
    InvalidUpstream(String),
}

impl ApiError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized => "NH-BRIDGE-101",
            Self::InvalidRequest(_) => "NH-BRIDGE-102",
            Self::UpstreamTransport(_) => "NH-BRIDGE-103",
            Self::UpstreamStatus { .. } => "NH-BRIDGE-104",
            Self::InvalidUpstream(_) => "NH-BRIDGE-105",
        }
    }

    pub(crate) fn status(&self) -> StatusCode {
        match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::InvalidRequest(_) => StatusCode::BAD_REQUEST,
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
            Self::InvalidRequest(_) => "invalid_request_error",
            Self::UpstreamStatus { status, .. } if status.as_u16() == 429 => "rate_limit_error",
            Self::UpstreamTransport(_) | Self::UpstreamStatus { .. } | Self::InvalidUpstream(_) => {
                "api_error"
            }
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

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status(), Json(self.event_data())).into_response()
    }
}
