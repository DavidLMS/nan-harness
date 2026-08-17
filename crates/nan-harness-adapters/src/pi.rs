use crate::direct::{
    DirectLaunch, PROVIDER_URL_ENVIRONMENT, build_direct_plan, describe_model,
    provider_environment, validate_routing_arguments,
};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, TemporaryArtifact, TemporaryArtifactKind, TemporaryArtifactMode,
};
use nan_harness_core::{HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError};
use serde_json::json;
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "NAN_API_KEY";
const EXTENSION_ARTIFACT_ID: &str = "pi-provider-extension";
const EXTENSION_PATH_PLACEHOLDER: &str = "{artifact:pi-provider-extension}";

#[derive(Debug, Default)]
pub struct PiAdapter;

impl HarnessAdapter for PiAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Pi
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        pi_family_plan(context)
    }
}

#[derive(Debug, Default)]
pub struct PrimeAgentAdapter;

impl HarnessAdapter for PrimeAgentAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::PrimeAgent
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        pi_family_plan(context)
    }
}

fn pi_family_plan(context: &PlanContext) -> Result<LaunchPlan, PlanError> {
    validate_routing_arguments(
        &context.user_arguments,
        &["--model", "--provider", "--api-key", "--models"],
    )?;
    let extension = provider_extension(&context.model.resolved_id)?;
    let mut arguments = vec![
        "--extension".to_owned(),
        EXTENSION_PATH_PLACEHOLDER.to_owned(),
        "--provider".to_owned(),
        "nan".to_owned(),
        "--model".to_owned(),
        context.model.resolved_id.clone(),
        "--models".to_owned(),
        "nan/*".to_owned(),
    ];
    arguments.extend(context.user_arguments.iter().cloned());

    build_direct_plan(
        context,
        DirectLaunch {
            arguments,
            credential_target: CREDENTIAL_TARGET,
            public_environment: provider_environment(),
            removed_environment: BTreeSet::new(),
            temporary_artifacts: vec![TemporaryArtifact {
                id: EXTENSION_ARTIFACT_ID.to_owned(),
                kind: TemporaryArtifactKind::File,
                path_hint: "nan-provider.mjs".to_owned(),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template: Some(extension),
                lifecycle: ArtifactLifecycle::Launch,
            }],
        },
    )
}

fn provider_extension(model_id: &str) -> Result<String, PlanError> {
    let model = describe_model(model_id);
    let input = if model.image_input {
        vec!["text", "image"]
    } else {
        vec!["text"]
    };
    let model = serde_json::to_string(&json!({
        "id": model_id,
        "name": model.display_name,
        "reasoning": false,
        "input": input,
        "cost": {
            "input": 0,
            "output": 0,
            "cacheRead": 0,
            "cacheWrite": 0
        },
        "contextWindow": model.context_window,
        "maxTokens": model.max_tokens,
        "compat": {
            "supportsDeveloperRole": false,
            "supportsReasoningEffort": false,
            "maxTokensField": "max_tokens"
        }
    }))
    .map_err(|error| PlanError::InvalidField {
        field: "temporaryArtifacts.contentTemplate",
        message: format!("could not serialize Pi model configuration: {error}"),
    })?;
    Ok(format!(
        "export default function registerNan(pi) {{\n  pi.registerProvider(\"nan\", {{\n    baseUrl: process.env.{PROVIDER_URL_ENVIRONMENT},\n    apiKey: \"$NAN_API_KEY\",\n    authHeader: true,\n    api: \"openai-completions\",\n    models: [{model}]\n  }});\n}}\n"
    ))
}
