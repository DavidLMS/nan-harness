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
mod search_http;
mod search_service;
mod server;
mod stream_common;
mod timeouts;
mod upstream;
mod usage;

use nan_harness_core::SecretValue;
use std::fmt;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
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
pub(crate) type ActivitySender = broadcast::Sender<BridgeActivity>;

#[derive(Clone, PartialEq, Eq)]
pub struct ClaudeAutoModeTracePayload(String);

impl ClaudeAutoModeTracePayload {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn with_contents<T>(&self, operation: impl FnOnce(&str) -> T) -> T {
        operation(&self.0)
    }
}

impl fmt::Debug for ClaudeAutoModeTracePayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ClaudeAutoModeTracePayload([REDACTED])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeAutoModeReviewStage {
    Initial,
    FollowUp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeActivity {
    AuthenticatedClient,
    ClaudeAutoModeReview {
        review_id: u64,
        stage: ClaudeAutoModeReviewStage,
        model_id: String,
        request: ClaudeAutoModeTracePayload,
    },
    ClaudeAutoModeReviewResponse {
        review_id: u64,
        status: u16,
        response: ClaudeAutoModeTracePayload,
    },
    ClaudeAutoModeReviewFailed {
        review_id: u64,
        error_code: &'static str,
    },
}

pub struct BridgeConfig {
    pub provider_base_url: String,
    pub models: ClaudeModelCatalog,
    pub provider_api_key: Arc<SecretValue>,
    pub session_token: Arc<SecretValue>,
    pub web_search_enabled: bool,
    pub auto_mode_traces: bool,
}

pub struct ResponsesBridgeConfig {
    pub provider_base_url: String,
    pub models: CodexModelCatalog,
    pub provider_api_key: Arc<SecretValue>,
    pub session_token: Arc<SecretValue>,
    pub web_search_enabled: bool,
}

impl fmt::Debug for ResponsesBridgeConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResponsesBridgeConfig")
            .field("provider_base_url", &self.provider_base_url)
            .field("models", &self.models)
            .field("provider_api_key", &"[REDACTED]")
            .field("session_token", &"[REDACTED]")
            .field("web_search_enabled", &self.web_search_enabled)
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
            .field("web_search_enabled", &self.web_search_enabled)
            .field("auto_mode_traces", &self.auto_mode_traces)
            .finish()
    }
}

pub struct RunningBridge {
    base_url: String,
    shutdown: CancellationToken,
    task: JoinHandle<Result<(), BridgeError>>,
    diagnostics: mpsc::UnboundedReceiver<BridgeDiagnostic>,
    usage: usage::SharedUsage,
    activities: ActivitySender,
}

impl RunningBridge {
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Returns whether the bridge server task has finished.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
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
        (&mut self.task).await.map_err(BridgeError::TaskJoin)??;
        usage::wait_until_idle(&self.usage).await;
        Ok(())
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

    /// Subscribes to explicitly enabled, user-facing bridge activity.
    #[must_use]
    pub fn subscribe_activities(&self) -> broadcast::Receiver<BridgeActivity> {
        self.activities.subscribe()
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
        |diagnostics, activities| server::router(config, diagnostics, activities, router_usage),
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
        |diagnostics, activities| {
            responses_server::router(config, diagnostics, activities, router_usage)
        },
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
        |diagnostics, _activities| fx_gateway::router(config, diagnostics, router_usage),
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
        |diagnostics, _activities| chat_completions::router(config, diagnostics, router_usage),
        usage,
    )
}

fn spawn_with_diagnostics(
    listener: TcpListener,
    build_router: impl FnOnce(DiagnosticSender, ActivitySender) -> Result<axum::Router, BridgeError>,
    usage: usage::SharedUsage,
) -> Result<RunningBridge, BridgeError> {
    let address = listener
        .local_addr()
        .map_err(BridgeError::ListenerAddress)?;
    if !address.ip().is_loopback() {
        return Err(BridgeError::NonLoopbackAddress(address));
    }
    let (diagnostics_tx, diagnostics) = mpsc::unbounded_channel();
    let (activities, _) = broadcast::channel(32);
    let app = build_router(diagnostics_tx, activities.clone())?;
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
        activities,
    })
}
