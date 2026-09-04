use crate::ResolvedConfig;
use nan_harness_bridge::{
    BridgeDiagnostic, BridgeError, CodexModelCatalog, ResponsesBridgeConfig, RunningBridge,
    discover_coding_models,
};
use nan_harness_core::{CodingModelProfile, SecretError, SecretValue};
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::net::TcpListener;

const DEFAULT_MODEL_ID: &str = "qwen3.6";
const DESKTOP_AUXILIARY_ALIAS: &str = "gpt-5.6-luna";

#[derive(Debug, Error)]
pub enum CodexDesktopBridgeError {
    #[error(transparent)]
    Bridge(#[from] BridgeError),
    #[error("could not bind the desktop bridge to loopback: {0}")]
    Bind(std::io::Error),
    #[error("could not generate the desktop bridge credential: {0}")]
    Random(getrandom::Error),
    #[error("could not resolve the NaN credential: {0}")]
    Secret(SecretError),
    #[error("the desktop bridge health check failed")]
    HealthCheck,
}

impl CodexDesktopBridgeError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Bridge(error) => error.code(),
            Self::Bind(_) => "NH-DESKTOP-101",
            Self::Random(_) | Self::Secret(_) => "NH-DESKTOP-102",
            Self::HealthCheck => "NH-DESKTOP-103",
        }
    }
}

pub struct RunningCodexDesktopBridge {
    bridge: RunningBridge,
    session_token: Arc<SecretValue>,
    model_catalog_json: String,
    selected_model: String,
    auxiliary_model: String,
    available_models: Vec<String>,
}

impl RunningCodexDesktopBridge {
    #[must_use]
    pub fn base_url(&self) -> &str {
        self.bridge.base_url()
    }

    #[must_use]
    pub fn model_catalog_json(&self) -> &str {
        &self.model_catalog_json
    }

    #[must_use]
    pub fn selected_model(&self) -> &str {
        &self.selected_model
    }

    #[must_use]
    pub fn auxiliary_model(&self) -> &str {
        &self.auxiliary_model
    }

    #[must_use]
    pub fn available_models(&self) -> &[String] {
        &self.available_models
    }

    pub fn with_session_token<T>(&self, operation: impl FnOnce(&str) -> T) -> T {
        self.session_token.with_secret(operation)
    }

    pub fn take_diagnostics(&mut self) -> tokio::sync::mpsc::UnboundedReceiver<BridgeDiagnostic> {
        self.bridge.take_diagnostics()
    }

    #[must_use]
    pub fn subscribe_activities(
        &self,
    ) -> tokio::sync::broadcast::Receiver<nan_harness_bridge::BridgeActivity> {
        self.bridge.subscribe_activities()
    }

    pub fn shutdown(&self) {
        self.bridge.shutdown();
    }

    /// Waits for the desktop bridge to stop.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError`] when the server task fails.
    pub async fn wait(&mut self) -> Result<(), BridgeError> {
        self.bridge.wait().await
    }

    #[must_use]
    pub fn usage(&self) -> nan_harness_bridge::ProviderUsageSnapshot {
        self.bridge.usage()
    }
}

/// Discovers entitled models and starts an authenticated Responses bridge for
/// one `ChatGPT` Desktop session.
///
/// # Errors
///
/// Returns [`CodexDesktopBridgeError`] when credentials, model discovery,
/// listener setup, or the bridge readiness check fails.
pub async fn start_codex_desktop_bridge(
    config: &ResolvedConfig,
    discovered_models: Option<Vec<CodingModelProfile>>,
    selected_model: Option<&str>,
    auxiliary_model: Option<&str>,
    web_search_enabled: bool,
) -> Result<RunningCodexDesktopBridge, CodexDesktopBridgeError> {
    let provider_api_key = config
        .secrets
        .with_secret(&config.provider_credential_ref, |value| {
            SecretValue::new(value.to_owned())
        })
        .map_err(CodexDesktopBridgeError::Secret)?
        .map(Arc::new)
        .map_err(CodexDesktopBridgeError::Secret)?;
    let models = if let Some(models) = discovered_models {
        models
    } else {
        discover_coding_models(&config.provider_base_url, Arc::clone(&provider_api_key)).await?
    };
    let selected_model = select_model(&models, selected_model)?;
    let auxiliary_model = select_model(&models, auxiliary_model.or(Some(&selected_model)))?;
    let aliases = vec![(DESKTOP_AUXILIARY_ALIAS.to_owned(), auxiliary_model.clone())];
    let catalog =
        CodexModelCatalog::from_models_with_aliases(models.clone(), &selected_model, &aliases)?;
    let model_catalog_json = catalog.api_response().to_string();
    let available_models = models.into_iter().map(|model| model.id).collect();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(CodexDesktopBridgeError::Bind)?;
    let session_token = Arc::new(generate_session_token()?);
    let bridge = nan_harness_bridge::spawn_responses(
        listener,
        ResponsesBridgeConfig {
            launch_id: format!("chatgpt_desktop_{}", std::process::id()),
            provider_base_url: config.provider_base_url.clone(),
            models: catalog,
            provider_api_key,
            session_token: Arc::clone(&session_token),
            web_search_enabled,
        },
    )?;
    health_check(bridge.base_url()).await?;
    Ok(RunningCodexDesktopBridge {
        bridge,
        session_token,
        model_catalog_json,
        selected_model,
        auxiliary_model,
        available_models,
    })
}

fn select_model(
    models: &[CodingModelProfile],
    requested: Option<&str>,
) -> Result<String, BridgeError> {
    let selected = requested.map_or_else(
        || {
            models
                .iter()
                .find(|model| model.id == DEFAULT_MODEL_ID)
                .or_else(|| models.first())
                .map(|model| model.id.clone())
        },
        |requested| Some(requested.to_owned()),
    );
    let Some(selected) = selected else {
        return Err(BridgeError::NoCompatibleModels);
    };
    if models.iter().any(|model| model.id == selected) {
        Ok(selected)
    } else {
        Err(BridgeError::SelectedModelUnavailable {
            model: selected,
            available: models.iter().map(|model| model.id.clone()).collect(),
        })
    }
}

fn generate_session_token() -> Result<SecretValue, CodexDesktopBridgeError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(CodexDesktopBridgeError::Random)?;
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut token, "{byte:02x}");
    }
    SecretValue::new(token).map_err(CodexDesktopBridgeError::Secret)
}

async fn health_check(base_url: &str) -> Result<(), CodexDesktopBridgeError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|_| CodexDesktopBridgeError::HealthCheck)?;
    let response = client
        .head(format!("{base_url}/api/hello"))
        .send()
        .await
        .map_err(|_| CodexDesktopBridgeError::HealthCheck)?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(CodexDesktopBridgeError::HealthCheck)
    }
}

#[cfg(test)]
mod tests {
    use super::{select_model, start_codex_desktop_bridge};
    use axum::Json;
    use axum::http::{HeaderMap, StatusCode, header};
    use axum::response::{IntoResponse, Response};
    use axum::routing::get;
    use nan_harness_core::{SecretRef, SecretStore, SecretValue, coding_models_from_provider_ids};

    #[test]
    fn desktop_model_selection_prefers_qwen_and_rejects_unknown_explicit_models() {
        let models =
            coding_models_from_provider_ids(["glm5.3-flash".to_owned(), "qwen3.6".to_owned()]);
        assert_eq!(
            select_model(&models, None).expect("default should resolve"),
            "qwen3.6"
        );
        assert_eq!(
            select_model(&models, Some("glm5.3-flash")).expect("explicit should resolve"),
            "glm5.3-flash"
        );
        assert!(select_model(&models, Some("not-entitled")).is_err());
    }

    #[tokio::test]
    async fn desktop_bridge_uses_live_models_and_hides_the_background_alias() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("provider should bind");
        let address = listener.local_addr().expect("provider address");
        let provider = tokio::spawn(async move {
            axum::serve(
                listener,
                axum::Router::new().route("/v1/models", get(fake_models)),
            )
            .await
            .expect("provider should serve");
        });
        let reference = SecretRef::new("nan_api_key").expect("valid reference");
        let mut secrets = SecretStore::new();
        secrets.insert(
            reference.clone(),
            SecretValue::new("test-key").expect("valid secret"),
        );
        let config = crate::ResolvedConfig {
            provider_base_url: format!("http://{address}/v1"),
            provider_credential_ref: reference,
            secrets,
        };

        let mut bridge =
            start_codex_desktop_bridge(&config, None, Some("qwen3.6"), Some("glm5.3-flash"), true)
                .await
                .expect("desktop bridge should start");
        assert_eq!(bridge.selected_model(), "qwen3.6");
        assert_eq!(bridge.auxiliary_model(), "glm5.3-flash");
        assert!(!bridge.model_catalog_json().contains("gpt-5.6-luna"));
        assert!(bridge.base_url().starts_with("http://127.0.0.1:"));
        assert!(bridge.with_session_token(|token| !token.is_empty()));

        bridge.shutdown();
        bridge.wait().await.expect("bridge should stop");
        provider.abort();
    }

    async fn fake_models(headers: HeaderMap) -> Response {
        if headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            != Some("Bearer test-key")
        {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        Json(serde_json::json!({
            "object": "list",
            "data": [
                {"id": "qwen3.6", "object": "model"},
                {"id": "glm5.3-flash", "object": "model"}
            ]
        }))
        .into_response()
    }
}
