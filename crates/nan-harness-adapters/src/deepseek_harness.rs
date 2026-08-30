use crate::direct::{
    DirectLaunch, PROVIDER_URL_ENVIRONMENT, build_direct_plan, provider_environment,
    validate_routing_arguments,
};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, BRIDGE_BASE_URL_PLACEHOLDER, DEEPSEEK_MODEL_CATALOG_PLACEHOLDER,
    NAN_SEARCH_BLOCK_BEGIN, NAN_SEARCH_BLOCK_END, TemporaryArtifact, TemporaryArtifactKind,
    TemporaryArtifactMode,
};
use nan_harness_core::{HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError};
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "NAN_API_KEY";
const PATCH_ARTIFACT_ID: &str = "deepseek-harness-patch";
const PATCH_PATH_PLACEHOLDER: &str = "{artifact:deepseek-harness-patch}";

#[derive(Debug, Default)]
pub struct DeepSeekHarnessAdapter;

impl HarnessAdapter for DeepSeekHarnessAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::DeepSeekHarness
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_routing_arguments(
            &context.user_arguments,
            &["--patch", "--dump-config", "--dump-default-config"],
        )?;
        let arguments = deepseek_arguments(&context.user_arguments)?;
        let mut public_environment = provider_environment();
        public_environment.insert("DSH_TELEMETRY_DISABLED".to_owned(), "1".to_owned());

        build_direct_plan(
            context,
            DirectLaunch {
                arguments,
                credential_target: CREDENTIAL_TARGET,
                public_environment,
                removed_environment: BTreeSet::new(),
                temporary_artifacts: vec![TemporaryArtifact {
                    id: PATCH_ARTIFACT_ID.to_owned(),
                    kind: TemporaryArtifactKind::File,
                    path_hint: "nan-provider.patch.yml".to_owned(),
                    mode: TemporaryArtifactMode::OwnerFile,
                    content_template: Some(provider_patch(&context.model.resolved_id)?),
                    lifecycle: ArtifactLifecycle::Launch,
                }],
                configuration_overlays: Vec::new(),
            },
        )
    }
}

fn deepseek_arguments(user_arguments: &[String]) -> Result<Vec<String>, PlanError> {
    if user_arguments
        .first()
        .is_some_and(|argument| argument == "--profile")
    {
        if user_arguments.get(1).is_none_or(String::is_empty) {
            return Err(PlanError::InvalidField {
                field: "process.arguments",
                message: "DeepSeek Harness --profile requires a profile name".to_owned(),
            });
        }
        let mut arguments = user_arguments[..2].to_vec();
        arguments.extend(["--patch".to_owned(), PATCH_PATH_PLACEHOLDER.to_owned()]);
        arguments.extend(user_arguments[2..].iter().cloned());
        return Ok(arguments);
    }
    let mut arguments = vec![
        "web".to_owned(),
        "--patch".to_owned(),
        PATCH_PATH_PLACEHOLDER.to_owned(),
    ];
    arguments.extend(user_arguments.iter().cloned());
    Ok(arguments)
}

fn provider_patch(model_id: &str) -> Result<String, PlanError> {
    let model_id = serde_json::to_string(model_id).map_err(|error| serialization_error(&error))?;
    Ok(format!(
        "- id: agent-default-model\n  config:\n    provider: nan-harness\n    model: {model_id}\n\n- id: llm-deepseek\n  disabled: true\n\n- id: llm-pi-ai\n  config:\n    providers:\n      nan-harness:\n        displayName: NaN\n        apiKeyEnv: NAN_API_KEY\n        api: openai-completions\n        baseURL: !!js process.env.{PROVIDER_URL_ENVIRONMENT}\n        models:\n{DEEPSEEK_MODEL_CATALOG_PLACEHOLDER}{NAN_SEARCH_BLOCK_BEGIN}\n- id: web-search-deepseek\n  disabled: false\n  config:\n    apiKeyEnv: NAN_API_KEY\n    baseURL: {BRIDGE_BASE_URL_PLACEHOLDER}/v1\n    model: {model_id}\n\n- id: tool-web\n  disabled: false\n  config:\n    fetch: false\n{NAN_SEARCH_BLOCK_END}\n"
    ))
}

fn serialization_error(error: &serde_json::Error) -> PlanError {
    PlanError::InvalidField {
        field: "temporaryArtifacts.contentTemplate",
        message: format!("could not serialize DeepSeek Harness model configuration: {error}"),
    }
}
