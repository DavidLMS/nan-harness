use crate::ResolvedConfig;
use nan_harness_bridge::{
    BridgeDiagnostic, ChatCompletionsBridgeConfig, ProviderUsageSnapshot, RunningBridge,
};
use nan_harness_core::{SecretError, SecretValue};
use std::fmt::Write as _;
use std::sync::Arc;
use thiserror::Error;
use tokio::net::TcpListener;

/// A launch-scoped, authenticated Chat Completions gateway whose lifetime is
/// controlled independently from a harness child process.
pub struct RunningChatCompletionsGateway {
    bridge: RunningBridge,
    session_token: Arc<SecretValue>,
}

impl RunningChatCompletionsGateway {
    /// Returns the child-facing OpenAI-compatible base URL, including `/v1`.
    #[must_use]
    pub fn client_base_url(&self) -> String {
        format!("{}/v1", self.bridge.base_url().trim_end_matches('/'))
    }

    /// Runs an operation with the launch-scoped gateway token.
    pub fn with_session_token<T>(&self, operation: impl FnOnce(&str) -> T) -> T {
        self.session_token.with_secret(operation)
    }

    /// Takes the safe diagnostic stream emitted by the gateway.
    #[must_use]
    pub fn take_diagnostics(&mut self) -> tokio::sync::mpsc::UnboundedReceiver<BridgeDiagnostic> {
        self.bridge.take_diagnostics()
    }

    /// Returns the usage observed so far.
    #[must_use]
    pub fn usage(&self) -> Option<ProviderUsageSnapshot> {
        Some(self.bridge.usage())
    }

    /// Waits for an unexpected or requested gateway exit.
    ///
    /// # Errors
    ///
    /// Returns [`ChatGatewayError`] if the server task fails.
    pub async fn wait(&mut self) -> Result<(), ChatGatewayError> {
        self.bridge.wait().await.map_err(ChatGatewayError::Bridge)
    }

    /// Stops the gateway and returns any diagnostics still queued.
    ///
    /// # Errors
    ///
    /// Returns [`ChatGatewayError`] if the server task fails while stopping.
    pub async fn shutdown(self) -> Result<Vec<BridgeDiagnostic>, ChatGatewayError> {
        self.shutdown_with_usage()
            .await
            .map(|(diagnostics, _usage)| diagnostics)
    }

    /// Stops the gateway and takes its usage snapshot after all requests finish.
    ///
    /// # Errors
    ///
    /// Returns [`ChatGatewayError`] if the server task fails while stopping.
    pub async fn shutdown_with_usage(
        mut self,
    ) -> Result<(Vec<BridgeDiagnostic>, ProviderUsageSnapshot), ChatGatewayError> {
        let mut diagnostics = self.bridge.take_diagnostics();
        self.bridge.shutdown();
        self.bridge.wait().await?;
        let usage = self.bridge.usage();
        let mut collected = Vec::new();
        while let Ok(diagnostic) = diagnostics.try_recv() {
            if !collected.contains(&diagnostic) {
                collected.push(diagnostic);
            }
        }
        Ok((collected, usage))
    }
}

impl std::fmt::Debug for RunningChatCompletionsGateway {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RunningChatCompletionsGateway")
            .field("client_base_url", &self.client_base_url())
            .field("session_token", &"[REDACTED]")
            .finish()
    }
}

/// Starts an authenticated Chat Completions gateway on a pre-bound loopback
/// listener. Accepting an already-bound listener lets callers reserve and
/// durably record a stable port without a time-of-check/time-of-use race.
///
/// # Errors
///
/// Returns [`ChatGatewayError`] for missing credentials, token generation, or
/// bridge startup failures.
pub fn start_chat_completions_gateway(
    config: &ResolvedConfig,
    listener: TcpListener,
    model_id: &str,
    web_search_enabled: bool,
) -> Result<RunningChatCompletionsGateway, ChatGatewayError> {
    let provider_api_key = config
        .secrets
        .with_secret(&config.provider_credential_ref, |value| {
            SecretValue::new(value.to_owned())
        })??;
    let provider_api_key = Arc::new(provider_api_key);
    let session_token = Arc::new(generate_session_token()?);
    let bridge = nan_harness_bridge::spawn_chat_completions(
        listener,
        ChatCompletionsBridgeConfig {
            launch_id: format!("desktop_{}", std::process::id()),
            provider_base_url: config.provider_base_url.clone(),
            model_id: model_id.to_owned(),
            provider_api_key,
            session_token: Arc::clone(&session_token),
            web_search_enabled,
        },
    )?;
    Ok(RunningChatCompletionsGateway {
        bridge,
        session_token,
    })
}

fn generate_session_token() -> Result<SecretValue, ChatGatewayError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(ChatGatewayError::Random)?;
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut token, "{byte:02x}");
    }
    SecretValue::new(token).map_err(ChatGatewayError::Secret)
}

#[derive(Debug, Error)]
pub enum ChatGatewayError {
    #[error(transparent)]
    Secret(#[from] SecretError),
    #[error("could not generate a private Chat Completions gateway token: {0}")]
    Random(getrandom::Error),
    #[error(transparent)]
    Bridge(#[from] nan_harness_bridge::BridgeError),
}

impl ChatGatewayError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Secret(_) | Self::Random(_) => "NH-RUNTIME-006",
            Self::Bridge(error) => error.code(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::start_chat_completions_gateway;
    use crate::{ConfigOverrides, ConfigResolver, EnvironmentSource};
    use nan_harness_core::SecretValue;
    use tokio::net::TcpListener;

    struct EmptyEnvironment;

    impl EnvironmentSource for EmptyEnvironment {
        fn value(&self, _name: &str) -> Option<String> {
            None
        }
    }

    #[tokio::test]
    async fn gateway_uses_the_prebound_loopback_port_and_private_token() {
        let config = ConfigResolver::resolve(
            &EmptyEnvironment,
            ConfigOverrides {
                provider_base_url: Some("http://127.0.0.1:9/v1".to_owned()),
                nan_api_key: Some(SecretValue::new("provider-secret").expect("test key")),
            },
        )
        .expect("config should resolve");
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("listener should bind");
        let port = listener.local_addr().expect("address").port();
        let gateway = start_chat_completions_gateway(&config, listener, "qwen3.6", true)
            .expect("gateway should start");

        assert_eq!(
            gateway.client_base_url(),
            format!("http://127.0.0.1:{port}/v1")
        );
        assert!(!format!("{gateway:?}").contains("provider-secret"));

        gateway.shutdown().await.expect("gateway should stop");
    }
}
