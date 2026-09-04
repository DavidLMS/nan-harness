mod proxy;
mod request;
mod routing;
mod state;
mod usage_observer;

use nan_harness_core::SecretValue;
use std::sync::Arc;

pub(crate) use routing::router;

#[derive(Debug)]
pub struct ChatCompletionsBridgeConfig {
    pub launch_id: String,
    pub provider_base_url: String,
    pub model_id: String,
    pub provider_api_key: Arc<SecretValue>,
    pub session_token: Arc<SecretValue>,
    pub web_search_enabled: bool,
}
