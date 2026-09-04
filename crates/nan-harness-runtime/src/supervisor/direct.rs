use super::bridge_setup::{BoundBridgeEndpoint, generate_session_token};
use super::lifecycle::{run_bridged_child, wait_for_child};
use super::preparation::PreparedHarnessLaunch;
use super::report::{bridged_report, report};
use super::session::copy_secret;
use super::{ExecutionReport, RuntimeError};
use crate::config::ResolvedConfig;
use crate::prepared::BridgePreparation;
use crate::process::spawn_child;
use crate::signals::CancellationToken;
use nan_harness_bridge::{ChatCompletionsBridgeConfig, spawn_chat_completions};
use nan_harness_core::launch_plan::Transport;
use nan_harness_core::{CodingModelProfile, LaunchPlan, PlanError};
use std::sync::Arc;

pub(super) async fn execute_direct_with_gateway(
    plan: &LaunchPlan,
    config: &ResolvedConfig,
    cancellation: &CancellationToken,
    discovered_models: Option<&[CodingModelProfile]>,
    web_search_enabled: bool,
) -> Result<ExecutionReport, RuntimeError> {
    let provider_api_key = copy_secret(&config.secrets, &config.provider_credential_ref)?;
    let BoundBridgeEndpoint { listener, base_url } =
        BoundBridgeEndpoint::bind_direct_chat_gateway().await?;
    let client_base_url = format!("{}/v1", base_url.trim_end_matches('/'));
    let session_token = Arc::new(generate_session_token()?);
    let session_token_ref = match &plan.transport {
        Transport::DirectChat {
            credential_target, ..
        } => plan
            .environment
            .secrets
            .get(credential_target)
            .cloned()
            .ok_or_else(|| {
                RuntimeError::InvalidPlan(PlanError::MissingSecretReference {
                    reference: credential_target.clone(),
                })
            })?,
        _ => unreachable!("execute_direct requires DirectChat"),
    };
    let launch = PreparedHarnessLaunch::prepare(
        plan,
        &config.provider_base_url,
        Some(BridgePreparation {
            base_url: base_url.clone(),
            client_base_url: Some(client_base_url),
            chat_url: None,
            session_token_ref,
            session_token: Arc::clone(&session_token),
            claude_available_models: Vec::new(),
            codex_model_catalog: None,
            web_search_enabled,
        }),
        discovered_models,
    )?;
    let mut bridge = spawn_chat_completions(
        listener,
        ChatCompletionsBridgeConfig {
            launch_id: plan.launch_id.to_string(),
            provider_base_url: config.provider_base_url.clone(),
            model_id: plan.model.resolved_id.clone(),
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

pub(super) async fn execute_direct_without_gateway(
    plan: &LaunchPlan,
    config: &ResolvedConfig,
    cancellation: &CancellationToken,
    discovered_models: Option<&[CodingModelProfile]>,
) -> Result<ExecutionReport, RuntimeError> {
    let launch =
        PreparedHarnessLaunch::prepare(plan, &config.provider_base_url, None, discovered_models)?;
    let mut child = spawn_child(plan, &launch.prepared, &config.secrets)?;
    let completion = wait_for_child(&mut child, plan, cancellation).await?;
    Ok(report(
        plan,
        completion,
        launch.temporary_root,
        None,
        Vec::new(),
        None,
    ))
}
