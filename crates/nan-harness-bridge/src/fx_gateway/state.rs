use super::models::FxModelCatalog;
use crate::DiagnosticSender;
use crate::error::BridgeError;
use crate::upstream::NanClient;
use crate::usage::SharedUsage;
use nan_harness_core::SecretValue;
use nan_harness_search::SearxngConfig;
use std::sync::Arc;

#[derive(Debug)]
pub struct FxGatewayConfig {
    pub launch_id: String,
    pub provider_base_url: String,
    pub models: FxModelCatalog,
    pub selected_model_id: String,
    pub provider_api_key: Arc<SecretValue>,
    pub session_token: Arc<SecretValue>,
    pub web_search_enabled: bool,
    /// Validated `SearXNG` configuration for managed search. `None` means that
    /// search is enabled by policy but has not been configured yet.
    pub search_config: Option<SearxngConfig>,
    pub session_max_tokens: Option<u64>,
}

#[derive(Clone)]
pub(super) struct AppState {
    pub(super) upstream: NanClient,
    pub(super) models: FxModelCatalog,
    pub(super) selected_model_id: String,
    pub(super) session_token: Arc<SecretValue>,
    pub(super) diagnostics: DiagnosticSender,
    pub(super) usage: SharedUsage,
    pub(super) web_search_enabled: bool,
    pub(super) search_client: Option<nan_harness_search::SearxngClient>,
}

impl AppState {
    pub(super) fn new(
        config: FxGatewayConfig,
        diagnostics: DiagnosticSender,
        usage: SharedUsage,
    ) -> Result<Self, BridgeError> {
        let search_client = config
            .web_search_enabled
            .then_some(config.search_config)
            .flatten()
            .map(nan_harness_search::SearxngClient::new)
            .transpose()
            .map_err(|_| BridgeError::BuildSearchClient)?;
        Ok(Self {
            upstream: NanClient::new_with_budget(
                &config.provider_base_url,
                config.provider_api_key,
                &config.launch_id,
                config.session_max_tokens,
            )?,
            models: config.models,
            selected_model_id: config.selected_model_id,
            session_token: config.session_token,
            diagnostics,
            usage,
            web_search_enabled: config.web_search_enabled,
            search_client,
        })
    }
}
