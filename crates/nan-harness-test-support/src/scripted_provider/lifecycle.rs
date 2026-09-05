use axum::Router;
use axum::routing::{get, post};
use serde_json::Value;
use std::sync::Arc;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::Duration;

use super::ProviderScenario;
use super::protocol::{chat_completions, fixture, models, search};
use super::state::ProviderState;

const PROVIDER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub struct ScriptedProvider {
    base_url: String,
    state: Arc<ProviderState>,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), std::io::Error>>>,
}

impl ScriptedProvider {
    /// Starts an isolated HTTP server that implements the NaN endpoints used by the bridge.
    ///
    /// # Errors
    ///
    /// Returns [`ScriptedProviderError`] when the listener cannot be created or inspected.
    pub async fn start(scenario: ProviderScenario) -> Result<Self, ScriptedProviderError> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(ScriptedProviderError::Bind)?;
        let address = listener
            .local_addr()
            .map_err(ScriptedProviderError::InspectAddress)?;
        let state = Arc::new(ProviderState::new(
            scenario,
            format!("http://{address}/fixture"),
        ));
        let app = Router::new()
            .route("/v1/models", get(models))
            .route("/v1/chat/completions", post(chat_completions))
            .route("/v1/search", post(search))
            .route("/fixture", get(fixture))
            .with_state(Arc::clone(&state));
        let (shutdown, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
        });
        Ok(Self {
            base_url: format!("http://{address}/v1"),
            state,
            shutdown: Some(shutdown),
            task: Some(task),
        })
    }

    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    #[must_use]
    pub fn fixture_url(&self) -> String {
        self.base_url.trim_end_matches("/v1").to_owned() + "/fixture"
    }

    #[must_use]
    pub fn chat_requests(&self) -> Vec<Value> {
        self.state.chat_requests()
    }

    #[must_use]
    pub fn search_requests(&self) -> Vec<Value> {
        self.state.search_requests()
    }

    #[must_use]
    pub fn model_requests(&self) -> usize {
        self.state.model_requests()
    }

    /// Returns whether the scripted exchange reached its final response.
    #[must_use]
    pub fn completed(&self) -> bool {
        self.state.completed()
    }

    /// Returns false when request retention was capped, which makes strict assertions fail
    /// instead of silently validating an incomplete provider transcript.
    #[must_use]
    pub fn recording_bounded(&self) -> bool {
        self.state.recording_bounded()
    }

    /// Stops the HTTP server and waits for its task to finish.
    ///
    /// # Errors
    ///
    /// Returns [`ScriptedProviderError`] when the server task panics or exits with an I/O error.
    pub async fn shutdown(mut self) -> Result<(), ScriptedProviderError> {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let mut task = self.task.take().ok_or(ScriptedProviderError::MissingTask)?;
        if let Ok(result) = tokio::time::timeout(PROVIDER_SHUTDOWN_TIMEOUT, &mut task).await {
            result
                .map_err(ScriptedProviderError::Join)?
                .map_err(ScriptedProviderError::Serve)
        } else {
            task.abort();
            let _ = tokio::time::timeout(PROVIDER_SHUTDOWN_TIMEOUT, task).await;
            Err(ScriptedProviderError::ShutdownTimeout)
        }
    }
}

impl Drop for ScriptedProvider {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[derive(Debug, Error)]
pub enum ScriptedProviderError {
    #[error("could not bind the scripted provider: {0}")]
    Bind(std::io::Error),
    #[error("could not inspect the scripted provider address: {0}")]
    InspectAddress(std::io::Error),
    #[error("the scripted provider task failed: {0}")]
    Join(tokio::task::JoinError),
    #[error("the scripted provider exited with an I/O error: {0}")]
    Serve(std::io::Error),
    #[error("the scripted provider task is unavailable")]
    MissingTask,
    #[error("the scripted provider did not shut down within its bounded timeout")]
    ShutdownTimeout,
}
