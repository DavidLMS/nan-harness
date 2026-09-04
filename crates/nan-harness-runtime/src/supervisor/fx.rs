use super::bridge_setup::{BoundBridgeEndpoint, BridgeLaunchOptions, generate_session_token};
use super::lifecycle::run_bridged_child;
use super::preparation::PreparedHarnessLaunch;
use super::report::bridged_report;
use super::session::copy_secret;
use super::{ExecutionReport, RuntimeError};
use crate::config::ResolvedConfig;
use crate::prepared::BridgePreparation;
use crate::signals::CancellationToken;
use nan_harness_bridge::{FxGatewayConfig, FxModelCatalog, spawn_fx_gateway};
use nan_harness_core::launch_plan::ListenAddress;
use nan_harness_core::{LaunchPlan, SecretRef};
use std::sync::Arc;

pub(super) async fn execute_fx_gateway(
    plan: &LaunchPlan,
    config: &ResolvedConfig,
    cancellation: &CancellationToken,
    listen: &ListenAddress,
    provider_credential_ref: &SecretRef,
    session_token_ref: &SecretRef,
    options: BridgeLaunchOptions<'_>,
) -> Result<ExecutionReport, RuntimeError> {
    let BridgeLaunchOptions {
        discovered_models,
        web_search_enabled,
    } = options;
    let provider_api_key = copy_secret(&config.secrets, provider_credential_ref)?;
    let BoundBridgeEndpoint { listener, base_url } =
        BoundBridgeEndpoint::bind_transport(listen).await?;
    let chat_url = format!("{base_url}/v3/ai/language-model");
    let session_token = Arc::new(generate_session_token()?);
    let models = FxModelCatalog::from_models(discovered_models.to_vec())?;
    let launch = PreparedHarnessLaunch::prepare(
        plan,
        &config.provider_base_url,
        Some(BridgePreparation {
            base_url,
            client_base_url: None,
            chat_url: Some(chat_url),
            session_token_ref: session_token_ref.clone(),
            session_token: Arc::clone(&session_token),
            claude_available_models: Vec::new(),
            codex_model_catalog: None,
            web_search_enabled,
        }),
        Some(discovered_models),
    )?;
    let mut bridge = spawn_fx_gateway(
        listener,
        FxGatewayConfig {
            launch_id: plan.launch_id.to_string(),
            provider_base_url: config.provider_base_url.clone(),
            models,
            selected_model_id: plan.model.resolved_id.clone(),
            provider_api_key,
            session_token,
            web_search_enabled,
        },
    )?;
    let execution = run_bridged_child(
        plan,
        &launch.prepared,
        &config.secrets,
        cancellation,
        &mut bridge,
    )
    .await?;
    Ok(bridged_report(plan, execution, launch.temporary_root, None))
}
