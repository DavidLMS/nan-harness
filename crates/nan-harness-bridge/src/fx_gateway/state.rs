use super::models::FxModelCatalog;
use crate::DiagnosticSender;
use crate::error::BridgeError;
use crate::upstream::NanClient;
use crate::usage::SharedUsage;
use nan_harness_core::SecretValue;
use std::sync::Arc;

#[derive(Debug)]
pub struct FxGatewayConfig {
    pub provider_base_url: String,
    pub models: FxModelCatalog,
    pub selected_model_id: String,
    pub provider_api_key: Arc<SecretValue>,
    pub session_token: Arc<SecretValue>,
    pub web_search_enabled: bool,
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
}

impl AppState {
    pub(super) fn new(
        config: FxGatewayConfig,
        diagnostics: DiagnosticSender,
        usage: SharedUsage,
    ) -> Result<Self, BridgeError> {
        Ok(Self {
            upstream: NanClient::new(&config.provider_base_url, config.provider_api_key)?,
            models: config.models,
            selected_model_id: config.selected_model_id,
            session_token: config.session_token,
            diagnostics,
            usage,
            web_search_enabled: config.web_search_enabled,
        })
    }
}
