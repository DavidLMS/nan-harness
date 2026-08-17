#![forbid(unsafe_code)]

mod anthropic;
mod auth;
mod error;
mod models;
mod responses;
mod responses_server;
mod server;
mod upstream;

use nan_harness_core::SecretValue;
use std::fmt;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub use error::BridgeError;
pub use models::{ClaudeModel, ClaudeModelCatalog};
pub use responses::models::catalog as codex_model_catalog;

pub struct BridgeConfig {
    pub provider_base_url: String,
    pub models: ClaudeModelCatalog,
    pub provider_api_key: Arc<SecretValue>,
    pub session_token: Arc<SecretValue>,
}

pub struct ResponsesBridgeConfig {
    pub provider_base_url: String,
    pub provider_model: String,
    pub provider_api_key: Arc<SecretValue>,
    pub session_token: Arc<SecretValue>,
}

impl fmt::Debug for ResponsesBridgeConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResponsesBridgeConfig")
            .field("provider_base_url", &self.provider_base_url)
            .field("provider_model", &self.provider_model)
            .field("provider_api_key", &"[REDACTED]")
            .field("session_token", &"[REDACTED]")
            .finish()
    }
}

impl fmt::Debug for BridgeConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BridgeConfig")
            .field("provider_base_url", &self.provider_base_url)
            .field("models", &self.models)
            .field("provider_api_key", &"[REDACTED]")
            .field("session_token", &"[REDACTED]")
            .finish()
    }
}

pub struct RunningBridge {
    base_url: String,
    shutdown: CancellationToken,
    task: JoinHandle<Result<(), BridgeError>>,
}

impl RunningBridge {
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn shutdown(&self) {
        self.shutdown.cancel();
    }

    /// Waits for the bridge server task to finish.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError`] when the server or its task fails.
    pub async fn wait(&mut self) -> Result<(), BridgeError> {
        (&mut self.task).await.map_err(BridgeError::TaskJoin)?
    }
}

impl Drop for RunningBridge {
    fn drop(&mut self) {
        self.shutdown.cancel();
        if !self.task.is_finished() {
            self.task.abort();
        }
    }
}

/// Starts an authenticated bridge on a pre-bound loopback listener.
///
/// # Errors
///
/// Returns [`BridgeError`] when the listener address or HTTP client is invalid.
pub fn spawn(listener: TcpListener, config: BridgeConfig) -> Result<RunningBridge, BridgeError> {
    spawn_router(listener, server::router(config)?)
}

/// Starts an authenticated `OpenAI` Responses bridge on a pre-bound loopback listener.
///
/// # Errors
///
/// Returns [`BridgeError`] when the listener address or HTTP client is invalid.
pub fn spawn_responses(
    listener: TcpListener,
    config: ResponsesBridgeConfig,
) -> Result<RunningBridge, BridgeError> {
    spawn_router(listener, responses_server::router(config)?)
}

fn spawn_router(listener: TcpListener, app: axum::Router) -> Result<RunningBridge, BridgeError> {
    let address = listener
        .local_addr()
        .map_err(BridgeError::ListenerAddress)?;
    if !address.ip().is_loopback() {
        return Err(BridgeError::NonLoopbackAddress(address));
    }
    let shutdown = CancellationToken::new();
    let server_shutdown = shutdown.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(server_shutdown.cancelled_owned())
            .await
            .map_err(BridgeError::Serve)
    });

    Ok(RunningBridge {
        base_url: format!("http://{address}"),
        shutdown,
        task,
    })
}
