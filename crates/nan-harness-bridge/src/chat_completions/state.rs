use super::ChatCompletionsBridgeConfig;
use crate::DiagnosticSender;
use crate::error::BridgeError;
use crate::timeouts::STREAM_INACTIVITY_TIMEOUT;
use crate::upstream::NanClient;
use crate::usage::SharedUsage;
use nan_harness_core::SecretValue;
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub(super) struct AppState {
    pub(super) client: Client,
    pub(super) provider_base_url: String,
    pub(super) fallback_model_id: String,
    pub(super) provider_api_key: Arc<SecretValue>,
    pub(super) session_token: Arc<SecretValue>,
    pub(super) usage: SharedUsage,
    pub(super) diagnostics: DiagnosticSender,
    pub(super) search_upstream: NanClient,
    pub(super) web_search_enabled: bool,
}

impl AppState {
    pub(super) fn new(
        config: ChatCompletionsBridgeConfig,
        diagnostics: DiagnosticSender,
        usage: SharedUsage,
    ) -> Result<Self, BridgeError> {
        let search_upstream = NanClient::new(
            &config.provider_base_url,
            Arc::clone(&config.provider_api_key),
        )?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(STREAM_INACTIVITY_TIMEOUT)
            .build()
            .map_err(BridgeError::BuildClient)?;
        Ok(Self {
            client,
            provider_base_url: config.provider_base_url.trim_end_matches('/').to_owned(),
            fallback_model_id: config.model_id,
            provider_api_key: config.provider_api_key,
            session_token: config.session_token,
            usage,
            diagnostics,
            search_upstream,
            web_search_enabled: config.web_search_enabled,
        })
    }
}
