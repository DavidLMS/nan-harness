use crate::temporary::{TemporaryError, TemporaryWorkspace};
use nan_harness_core::launch_plan::{
    BRIDGE_BASE_URL_PLACEHOLDER, CLAUDE_AVAILABLE_MODELS_PLACEHOLDER,
    CODEX_MODEL_CATALOG_PLACEHOLDER, PROVIDER_BASE_URL_PLACEHOLDER,
};
use nan_harness_core::{CodingModelProfile, LaunchPlan};
use std::collections::BTreeMap;

use super::{BridgePreparation, PreparedError, PreparedLaunch, catalogs, values};

pub(super) fn prepare(
    plan: &LaunchPlan,
    provider_base_url: &str,
    bridge: Option<BridgePreparation>,
    model_catalog: Option<&[CodingModelProfile]>,
) -> Result<PreparedLaunch, PreparedError> {
    let bridge_base_url = bridge.as_ref().map(|values| values.base_url.as_str());
    let client_base_url = bridge
        .as_ref()
        .and_then(|values| values.client_base_url.as_deref())
        .unwrap_or(provider_base_url);
    let selected_reasoning_effort = model_catalog
        .map(|models| {
            catalogs::selected_model_reasoning_effort(
                &plan.model.resolved_id,
                plan.model.reasoning_selection,
                models,
            )
        })
        .transpose()
        .map_err(PreparedError::ModelCatalog)?;
    let runtime_values = values::RuntimeRenderValues {
        provider_base_url: client_base_url,
        bridge_base_url,
        bridge_chat_url: bridge
            .as_ref()
            .and_then(|values| values.chat_url.as_deref()),
        selected_reasoning_effort: selected_reasoning_effort.as_deref(),
        web_search_enabled: bridge
            .as_ref()
            .is_some_and(|values| values.web_search_enabled),
    };
    let workspace = TemporaryWorkspace::materialize_with(
        &plan.temporary_artifacts,
        &plan.configuration_overlays,
        &plan.launch_scoped_files,
        |resource_id, template| {
            render_template(
                template,
                client_base_url,
                &plan.model.resolved_id,
                selected_reasoning_effort.as_deref(),
                bridge.as_ref(),
                model_catalog,
            )
            .map_err(|reason| TemporaryError::InvalidArtifact {
                artifact_id: resource_id.to_owned(),
                reason,
            })
        },
    )?;
    let arguments = plan
        .process
        .arguments
        .iter()
        .map(|argument| {
            values::resolve_argument(argument, &workspace).and_then(|argument| {
                let argument = catalogs::render_model_catalogs(
                    &argument,
                    client_base_url,
                    &plan.model.resolved_id,
                    model_catalog,
                )
                .map_err(PreparedError::ModelCatalog)?;
                values::render_runtime_value(&argument, &runtime_values)
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let public_environment = plan
        .environment
        .public
        .iter()
        .map(|(name, value)| {
            values::resolve_argument(value, &workspace)
                .and_then(|value| {
                    values::render_public_value(
                        &value,
                        &runtime_values,
                        workspace.user_home(),
                        &plan.model.resolved_id,
                        model_catalog,
                    )
                })
                .map(|value| (name.clone(), value))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let runtime_secrets = bridge
        .map(|values| BTreeMap::from([(values.session_token_ref, values.session_token)]))
        .unwrap_or_default();

    Ok(PreparedLaunch {
        arguments,
        public_environment,
        runtime_secrets,
        workspace,
    })
}

fn render_template(
    template: &str,
    provider_base_url: &str,
    selected_model_id: &str,
    selected_reasoning_effort: Option<&str>,
    bridge: Option<&BridgePreparation>,
    model_catalog: Option<&[CodingModelProfile]>,
) -> Result<String, String> {
    let rendered = values::render_nan_search_blocks(
        template,
        bridge.is_some_and(|values| values.web_search_enabled),
    )?;
    let rendered = rendered.replace(PROVIDER_BASE_URL_PLACEHOLDER, provider_base_url);
    let rendered = catalogs::render_model_catalogs(
        &rendered,
        provider_base_url,
        selected_model_id,
        model_catalog,
    )?;
    let rendered = catalogs::render_reasoning_effort(&rendered, selected_reasoning_effort)?;
    let Some(bridge) = bridge else {
        if rendered.contains("{runtime:") || rendered.contains("{secret:") {
            return Err("runtime placeholders require a bridge preparation".to_owned());
        }
        return Ok(rendered);
    };
    let rendered = rendered.replace(BRIDGE_BASE_URL_PLACEHOLDER, &bridge.base_url);
    let available_models = serde_json::to_string(&bridge.claude_available_models)
        .map_err(|error| format!("could not serialize Claude model IDs: {error}"))?;
    let quoted_placeholder = format!("\"{CLAUDE_AVAILABLE_MODELS_PLACEHOLDER}\"");
    let rendered = rendered.replace(&quoted_placeholder, &available_models);
    let rendered = match bridge.codex_model_catalog.as_deref() {
        Some(catalog) => rendered.replace(CODEX_MODEL_CATALOG_PLACEHOLDER, catalog),
        None => rendered,
    };
    let placeholder = format!("{{secret:{}}}", bridge.session_token_ref.as_str());
    let rendered = bridge
        .session_token
        .with_secret(|token| rendered.replace(&placeholder, token));
    if rendered.contains("{runtime:") || rendered.contains("{secret:") {
        Err("content contains an unresolved runtime placeholder".to_owned())
    } else {
        Ok(rendered)
    }
}

pub(crate) fn requires_model_catalog(plan: &LaunchPlan) -> bool {
    plan.temporary_artifacts
        .iter()
        .filter_map(|artifact| artifact.content_template.as_deref())
        .chain(plan.configuration_overlays.iter().flat_map(|overlay| {
            overlay
                .files
                .iter()
                .map(|file| file.content_template.as_str())
        }))
        .chain(
            plan.launch_scoped_files
                .iter()
                .map(|file| file.content_template.as_str()),
        )
        .chain(plan.environment.public.values().map(String::as_str))
        .chain(plan.process.arguments.iter().map(String::as_str))
        .any(catalogs::contains_model_catalog_placeholder)
}
