use super::bridge_setup::{BoundBridgeEndpoint, BridgeLaunchOptions, generate_session_token};
use super::lifecycle::run_bridged_child;
use super::preparation::PreparedHarnessLaunch;
use super::report::{Completion, bridged_report, prepared_codex_selection};
use super::session::copy_secret;
use super::{ExecutionReport, RuntimeError};
use crate::config::ResolvedConfig;
use crate::prepared::BridgePreparation;
use crate::signals::CancellationToken;
use nan_harness_bridge::{CodexModelCatalog, ResponsesBridgeConfig, spawn_responses};
use nan_harness_core::launch_plan::ListenAddress;
use nan_harness_core::{LaunchPlan, SecretRef};
use std::sync::Arc;

pub(super) async fn execute_responses_bridge(
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
    let session_token = Arc::new(generate_session_token()?);
    let models =
        CodexModelCatalog::from_models(discovered_models.to_vec(), &plan.model.resolved_id)?;
    let launch = PreparedHarnessLaunch::prepare(
        plan,
        &config.provider_base_url,
        Some(BridgePreparation {
            base_url,
            client_base_url: None,
            chat_url: None,
            session_token_ref: session_token_ref.clone(),
            session_token: Arc::clone(&session_token),
            claude_available_models: Vec::new(),
            codex_model_catalog: Some(models.api_response().to_string()),
            web_search_enabled,
        }),
        Some(discovered_models),
    )?;
    let mut bridge = spawn_responses(
        listener,
        ResponsesBridgeConfig {
            provider_base_url: config.provider_base_url.clone(),
            models,
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
    let selected = matches!(execution.completion, Completion::Exited(status) if status.success())
        .then(|| prepared_codex_selection(&launch.prepared, discovered_models))
        .flatten();
    Ok(bridged_report(
        plan,
        execution,
        launch.temporary_root,
        selected,
    ))
}
