mod anthropic;
mod bridge_setup;
mod direct;
mod error;
mod fx;
mod lifecycle;
mod preparation;
mod report;
mod responses;
mod session;

pub use error::RuntimeError;
pub use report::{ExecutionOutcome, ExecutionReport};
pub use session::LaunchSession;

use crate::config::ResolvedConfig;
use crate::prepared::requires_model_catalog;
use crate::search_policy::{SearchBackend, bridge_search_values, resolve_runtime_config};
use crate::search_supervisor::SearchSupervisor;
use crate::signals::CancellationToken;
use anthropic::execute_anthropic_bridge;
use bridge_setup::BridgeLaunchOptions;
use direct::{execute_direct_with_gateway, execute_direct_without_gateway};
use fx::execute_fx_gateway;
use nan_harness_core::launch_plan::Transport;
use nan_harness_core::{LaunchPlan, LaunchPlanValidator};
use responses::execute_responses_bridge;
use session::validate_selected_model;

#[derive(Debug)]
pub struct Supervisor {
    direct_chat_gateway: bool,
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl Supervisor {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            direct_chat_gateway: true,
        }
    }

    #[must_use]
    pub const fn without_direct_chat_gateway(mut self) -> Self {
        self.direct_chat_gateway = false;
        self
    }

    /// Validates, prepares, and supervises one harness launch to completion.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] when validation, setup, process control, or cleanup fails.
    pub async fn execute(
        &self,
        plan: &LaunchPlan,
        config: &ResolvedConfig,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionReport, RuntimeError> {
        let session = LaunchSession::new(config);
        self.execute_in_session(plan, &session, cancellation).await
    }

    /// Validates, prepares, and supervises one launch while reusing its model catalog.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] when validation, model discovery, setup, process control, or
    /// cleanup fails.
    pub async fn execute_in_session(
        &self,
        plan: &LaunchPlan,
        session: &LaunchSession<'_>,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionReport, RuntimeError> {
        LaunchPlanValidator::validate(plan).map_err(RuntimeError::InvalidPlan)?;
        let search_configuration = resolve_runtime_config(plan, self.direct_chat_gateway)?;
        let (web_search_enabled, search_config) = bridge_search_values(&search_configuration);
        let search_supervisor = managed_search_supervisor(&search_configuration);
        let model_catalog_required = !matches!(&plan.transport, Transport::DirectChat { .. })
            || requires_model_catalog(plan);
        let model_catalog = if model_catalog_required {
            let models = session.model_catalog().await?;
            validate_selected_model(models, &plan.model.resolved_id)?;
            Some(models)
        } else {
            None
        };
        let config = session.config;
        match &plan.transport {
            Transport::DirectChat { .. } if self.direct_chat_gateway => {
                execute_direct_with_gateway(
                    plan,
                    config,
                    cancellation,
                    model_catalog,
                    web_search_enabled,
                    search_config.clone(),
                    search_supervisor.clone(),
                )
                .await
            }
            Transport::DirectChat { .. } => {
                execute_direct_without_gateway(plan, config, cancellation, model_catalog).await
            }
            Transport::AnthropicBridge {
                listen,
                provider_credential_ref,
                session_token_ref,
                ..
            } => {
                execute_anthropic_bridge(
                    plan,
                    config,
                    cancellation,
                    listen,
                    provider_credential_ref,
                    session_token_ref,
                    BridgeLaunchOptions {
                        discovered_models: model_catalog.unwrap_or_default(),
                        web_search_enabled,
                        search_config: search_config.clone(),
                        search_supervisor: search_supervisor.clone(),
                    },
                )
                .await
            }
            Transport::ResponsesBridge {
                listen,
                provider_credential_ref,
                session_token_ref,
                ..
            } => {
                execute_responses_bridge(
                    plan,
                    config,
                    cancellation,
                    listen,
                    provider_credential_ref,
                    session_token_ref,
                    BridgeLaunchOptions {
                        discovered_models: model_catalog.unwrap_or_default(),
                        web_search_enabled,
                        search_config: search_config.clone(),
                        search_supervisor: search_supervisor.clone(),
                    },
                )
                .await
            }
            Transport::FxGatewayBridge {
                listen,
                provider_credential_ref,
                session_token_ref,
            } => {
                execute_fx_gateway(
                    plan,
                    config,
                    cancellation,
                    listen,
                    provider_credential_ref,
                    session_token_ref,
                    BridgeLaunchOptions {
                        discovered_models: model_catalog.unwrap_or_default(),
                        web_search_enabled,
                        search_config,
                        search_supervisor,
                    },
                )
                .await
            }
        }
    }
}

fn managed_search_supervisor(
    configuration: &crate::search_policy::SearchRuntimeConfig,
) -> Option<SearchSupervisor> {
    let SearchBackend::Searxng(endpoint) = &configuration.backend else {
        return None;
    };
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    SearchSupervisor::from_standalone_install(endpoint, home.as_deref())
        .ok()
        .flatten()
}

#[cfg(test)]
mod tests;
