#![forbid(unsafe_code)]

mod anthropic;
mod auth;
mod chat_completions;
mod diagnostics;
mod error;
mod fx_gateway;
mod models;
mod responses;
mod responses_server;
mod server;
mod timeouts;
mod upstream;
mod usage;

use nan_harness_core::SecretValue;
use std::fmt;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub use diagnostics::{
    BridgeDiagnostic, BridgeDiagnosticReason, BridgeEndpoint, BridgeModelPolicy,
    BridgeReasoningRequest,
};
pub use error::BridgeError;
pub use fx_gateway::{FxGatewayConfig, FxModelCatalog};
pub use models::{ClaudeModel, ClaudeModelCatalog, discover_coding_models};
pub use responses::models::CodexModelCatalog;

pub use chat_completions::ChatCompletionsBridgeConfig;
pub use usage::{ModelUsageSnapshot, ProviderUsageSnapshot};

pub(crate) type DiagnosticSender = mpsc::UnboundedSender<BridgeDiagnostic>;

pub struct BridgeConfig {
    pub provider_base_url: String,
    pub models: ClaudeModelCatalog,
    pub provider_api_key: Arc<SecretValue>,
    pub session_token: Arc<SecretValue>,
}

pub struct ResponsesBridgeConfig {
    pub provider_base_url: String,
    pub models: CodexModelCatalog,
    pub provider_api_key: Arc<SecretValue>,
    pub session_token: Arc<SecretValue>,
}

impl fmt::Debug for ResponsesBridgeConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResponsesBridgeConfig")
            .field("provider_base_url", &self.provider_base_url)
            .field("models", &self.models)
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
    diagnostics: mpsc::UnboundedReceiver<BridgeDiagnostic>,
    usage: usage::SharedUsage,
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

    /// Takes the queue of diagnostics emitted while the server is running.
    #[must_use]
    pub fn take_diagnostics(&mut self) -> mpsc::UnboundedReceiver<BridgeDiagnostic> {
        let (_, replacement) = mpsc::unbounded_channel();
        std::mem::replace(&mut self.diagnostics, replacement)
    }

    /// Returns the provider-reported usage observed by this bridge.
    #[must_use]
    pub fn usage(&self) -> ProviderUsageSnapshot {
        usage::snapshot(&self.usage)
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
    let usage = usage::new_usage();
    let router_usage = usage.clone();
    spawn_with_diagnostics(
        listener,
        |diagnostics| server::router(config, diagnostics, router_usage),
        usage,
    )
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
    let usage = usage::new_usage();
    let router_usage = usage.clone();
    spawn_with_diagnostics(
        listener,
        |diagnostics| responses_server::router(config, diagnostics, router_usage),
        usage,
    )
}

/// Starts an authenticated `fx` AI Gateway-compatible bridge.
///
/// # Errors
///
/// Returns [`BridgeError`] when the router or loopback listener cannot be created.
pub fn spawn_fx_gateway(
    listener: TcpListener,
    config: FxGatewayConfig,
) -> Result<RunningBridge, BridgeError> {
    let usage = usage::new_usage();
    let router_usage = usage.clone();
    spawn_with_diagnostics(
        listener,
        |diagnostics| fx_gateway::router(config, diagnostics, router_usage),
        usage,
    )
}

/// Starts an authenticated, transparent Chat Completions pass-through on a
/// pre-bound loopback listener.
///
/// The child-facing bearer token is replaced with the provider credential
/// only inside the bridge. Responses are forwarded without buffering so the
/// bridge can observe usage while preserving streaming behavior.
///
/// # Errors
///
/// Returns [`BridgeError`] when the listener address or HTTP client is invalid.
pub fn spawn_chat_completions(
    listener: TcpListener,
    config: ChatCompletionsBridgeConfig,
) -> Result<RunningBridge, BridgeError> {
    let usage = usage::new_usage();
    let router_usage = usage.clone();
    spawn_with_diagnostics(
        listener,
        |diagnostics| chat_completions::router(config, diagnostics, router_usage),
        usage,
    )
}

fn spawn_with_diagnostics(
    listener: TcpListener,
    build_router: impl FnOnce(DiagnosticSender) -> Result<axum::Router, BridgeError>,
    usage: usage::SharedUsage,
) -> Result<RunningBridge, BridgeError> {
    let address = listener
        .local_addr()
        .map_err(BridgeError::ListenerAddress)?;
    if !address.ip().is_loopback() {
        return Err(BridgeError::NonLoopbackAddress(address));
    }
    let (diagnostics_tx, diagnostics) = mpsc::unbounded_channel();
    let app = build_router(diagnostics_tx)?;
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
        diagnostics,
        usage,
    })
}
