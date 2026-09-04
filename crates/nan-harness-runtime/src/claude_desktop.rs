use crate::ResolvedConfig;
use nan_harness_bridge::{
    BridgeActivity, BridgeConfig, BridgeDiagnostic, BridgeError, ClaudeModelCatalog, RunningBridge,
    discover_coding_models,
};
use nan_harness_core::{CodingModelProfile, SecretError, SecretValue};
use reqwest::StatusCode;
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::net::TcpListener;

const HEALTH_ATTEMPTS: usize = 20;
const HEALTH_RETRY_DELAY: Duration = Duration::from_millis(25);

/// A ready, authenticated Anthropic bridge configured for Claude Desktop.
pub struct RunningClaudeDesktopBridge {
    bridge: RunningBridge,
    session_token: Arc<SecretValue>,
    selected_model: String,
}

impl RunningClaudeDesktopBridge {
    #[must_use]
    pub fn base_url(&self) -> &str {
        self.bridge.base_url()
    }

    pub fn with_session_token<T>(&self, operation: impl FnOnce(&str) -> T) -> T {
        self.session_token.with_secret(operation)
    }

    /// Subscribes to bridge activity for this Claude Desktop session.
    #[must_use]
    pub fn subscribe_activities(&self) -> tokio::sync::broadcast::Receiver<BridgeActivity> {
        self.bridge.subscribe_activities()
    }

    #[must_use]
    pub fn selected_model(&self) -> &str {
        &self.selected_model
    }

    /// Stops the bridge and returns diagnostics emitted during its session.
    ///
    /// # Errors
    ///
    /// Returns [`ClaudeDesktopBridgeError`] if the bridge task fails while stopping.
    pub async fn shutdown(self) -> Result<Vec<BridgeDiagnostic>, ClaudeDesktopBridgeError> {
        self.shutdown_with_usage()
            .await
            .map(|(diagnostics, _usage)| diagnostics)
    }

    /// Stops the bridge, waits for in-flight requests, and then snapshots usage.
    ///
    /// # Errors
    ///
    /// Returns [`ClaudeDesktopBridgeError`] if the bridge task fails while stopping.
    pub async fn shutdown_with_usage(
        mut self,
    ) -> Result<
        (
            Vec<BridgeDiagnostic>,
            nan_harness_bridge::ProviderUsageSnapshot,
        ),
        ClaudeDesktopBridgeError,
    > {
        let mut diagnostics = self.bridge.take_diagnostics();
        self.bridge.shutdown();
        self.bridge.wait().await?;
        let usage = self.bridge.usage();
        let mut collected = Vec::new();
        while let Ok(diagnostic) = diagnostics.try_recv() {
            collected.push(diagnostic);
        }
        Ok((collected, usage))
    }
}

impl std::fmt::Debug for RunningClaudeDesktopBridge {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RunningClaudeDesktopBridge")
            .field("base_url", &self.base_url())
            .field("session_token", &"[REDACTED]")
            .finish()
    }
}

/// Discovers the selected model, starts an authenticated loopback bridge, and
/// verifies its health before returning it to the caller.
///
/// # Errors
///
/// Returns [`ClaudeDesktopBridgeError`] for missing secrets, model discovery,
/// listener, bridge startup, or health-check failures.
pub async fn start_claude_desktop_bridge(
    config: &ResolvedConfig,
    discovered_models: Option<Vec<CodingModelProfile>>,
    selected_model: Option<&str>,
    auto_mode_traces: bool,
    web_search_enabled: bool,
) -> Result<RunningClaudeDesktopBridge, ClaudeDesktopBridgeError> {
    let provider_api_key = config
        .secrets
        .with_secret(&config.provider_credential_ref, |value| {
            SecretValue::new(value.to_owned())
        })??;
    let provider_api_key = Arc::new(provider_api_key);
    let discovered = if let Some(models) = discovered_models {
        models
    } else {
        discover_coding_models(&config.provider_base_url, Arc::clone(&provider_api_key)).await?
    };
    let models = ClaudeModelCatalog::for_desktop(discovered, selected_model)?;
    let selected_model = models.default_model().provider_id().to_owned();
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(ClaudeDesktopBridgeError::Bind)?;
    let session_token = Arc::new(generate_session_token()?);
    let mut bridge = nan_harness_bridge::spawn(
        listener,
        BridgeConfig {
            launch_id: format!("claude_desktop_{}", std::process::id()),
            provider_base_url: config.provider_base_url.clone(),
            models,
            provider_api_key,
            session_token: Arc::clone(&session_token),
            web_search_enabled,
            auto_mode_traces,
        },
    )?;
    if let Err(error) = probe_health(bridge.base_url()).await {
        bridge.shutdown();
        let _ = bridge.wait().await;
        return Err(error);
    }
    Ok(RunningClaudeDesktopBridge {
        bridge,
        session_token,
        selected_model,
    })
}

async fn probe_health(base_url: &str) -> Result<(), ClaudeDesktopBridgeError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(ClaudeDesktopBridgeError::HealthTransport)?;
    let endpoint = format!("{}/api/hello", base_url.trim_end_matches('/'));
    let mut attempts_remaining = HEALTH_ATTEMPTS;
    loop {
        match client.head(&endpoint).send().await {
            Ok(response) if response.status().is_success() => return Ok(()),
            Ok(response) => {
                return Err(ClaudeDesktopBridgeError::HealthStatus(response.status()));
            }
            Err(error) => {
                attempts_remaining = attempts_remaining.saturating_sub(1);
                if attempts_remaining == 0 {
                    return Err(ClaudeDesktopBridgeError::HealthTransport(error));
                }
            }
        }
        tokio::time::sleep(HEALTH_RETRY_DELAY).await;
    }
}

fn generate_session_token() -> Result<SecretValue, ClaudeDesktopBridgeError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(ClaudeDesktopBridgeError::Random)?;
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut token, "{byte:02x}");
    }
    SecretValue::new(token).map_err(ClaudeDesktopBridgeError::Secret)
}

#[derive(Debug, Error)]
pub enum ClaudeDesktopBridgeError {
    #[error(transparent)]
    Secret(#[from] SecretError),
    #[error("could not generate a private Claude Desktop bridge token: {0}")]
    Random(getrandom::Error),
    #[error(transparent)]
    Bridge(#[from] BridgeError),
    #[error("could not bind the Claude Desktop bridge to loopback: {0}")]
    Bind(std::io::Error),
    #[error("Claude Desktop bridge health check failed: {0}")]
    HealthTransport(reqwest::Error),
    #[error("Claude Desktop bridge health check returned HTTP {0}")]
    HealthStatus(StatusCode),
}

impl ClaudeDesktopBridgeError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Secret(_) | Self::Random(_) => "NH-DESKTOP-BRIDGE-001",
            Self::Bridge(error) => error.code(),
            Self::Bind(_) => "NH-DESKTOP-BRIDGE-002",
            Self::HealthTransport(_) | Self::HealthStatus(_) => "NH-DESKTOP-BRIDGE-003",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ClaudeDesktopBridgeError, start_claude_desktop_bridge};
    use crate::{ConfigOverrides, ConfigResolver, EnvironmentSource, ResolvedConfig};
    use axum::{Json, Router, routing::get};
    use nan_harness_core::{SecretRef, SecretStore, SecretValue};
    use serde_json::json;
    use tokio::net::TcpListener;

    struct EmptyEnvironment;

    impl EnvironmentSource for EmptyEnvironment {
        fn value(&self, _name: &str) -> Option<String> {
            None
        }
    }

    async fn provider_config(api_key: &str) -> (ResolvedConfig, String) {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("provider listener should bind");
        let address = listener.local_addr().expect("provider address");
        let router = Router::new().route(
            "/v1/models",
            get(|| async { Json(json!({"data": [{"id": "qwen3.6"}]})) }),
        );
        tokio::spawn(axum::serve(listener, router).into_future());
        let base_url = format!("http://{address}/v1");
        let config = ConfigResolver::resolve(
            &EmptyEnvironment,
            ConfigOverrides {
                provider_base_url: Some(base_url),
                nan_api_key: Some(SecretValue::new(api_key).expect("test key")),
            },
        )
        .expect("config should resolve");
        (config, api_key.to_owned())
    }

    #[tokio::test]
    async fn desktop_bridge_is_loopback_and_requires_its_session_token() {
        let (config, _) = provider_config("provider-secret").await;
        let bridge = start_claude_desktop_bridge(&config, None, None, false, true)
            .await
            .expect("desktop bridge should start");
        assert!(bridge.base_url().starts_with("http://127.0.0.1:"));

        let client = reqwest::Client::new();
        let endpoint = format!("{}/v1/models", bridge.base_url());
        let unauthorized = client
            .get(&endpoint)
            .send()
            .await
            .expect("unauthorized request should complete");
        assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

        let authorized =
            bridge.with_session_token(|token| client.get(&endpoint).bearer_auth(token).send());
        let authorized = authorized
            .await
            .expect("authorized request should complete");
        assert!(authorized.status().is_success());
        let payload: serde_json::Value = authorized.json().await.expect("model JSON");
        assert!(payload["data"][0]["id"].as_str().is_some_and(|id| {
            id.starts_with("claude-nan-") && id.len() == 75 && !id.contains("qwen3.6")
        }));

        bridge.shutdown().await.expect("bridge should stop");
    }

    #[tokio::test]
    async fn desktop_bridge_requires_the_provider_credential_reference() {
        let config = ResolvedConfig {
            provider_base_url: "http://127.0.0.1:9/v1".to_owned(),
            provider_credential_ref: SecretRef::new("nan_api_key").expect("reference"),
            secrets: SecretStore::new(),
        };

        let error = start_claude_desktop_bridge(&config, None, Some("qwen3.6"), false, true)
            .await
            .expect_err("missing credential should fail");
        assert!(matches!(error, ClaudeDesktopBridgeError::Secret(_)));
    }

    #[tokio::test]
    async fn desktop_bridge_debug_never_contains_the_provider_key() {
        let (config, api_key) = provider_config("never-print-this-provider-key").await;
        let bridge = start_claude_desktop_bridge(&config, None, Some("qwen3.6"), false, true)
            .await
            .expect("desktop bridge should start");

        assert!(!format!("{bridge:?}").contains(&api_key));
        bridge.shutdown().await.expect("bridge should stop");
    }
}
