use crate::direct::{
    DirectLaunch, build_direct_plan, provider_environment, validate_routing_arguments,
};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, ConfigurationOverlay, OverlayFile, OverlayFilePolicy,
    PROVIDER_BASE_URL_PLACEHOLDER, QWEN_CODE_MODEL_CATALOG_PLACEHOLDER, TemporaryArtifactMode,
    USER_HOME_PLACEHOLDER,
};
use nan_harness_core::{HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError};
use serde_json::json;
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "OPENAI_API_KEY";
const CONFIG_OVERLAY_ID: &str = "qwen-config";
const CONFIG_PATH_PLACEHOLDER: &str = "{artifact:qwen-config}";

#[derive(Debug, Default)]
pub struct QwenCodeAdapter;

#[derive(Debug, Default)]
pub struct PersistentQwenCodeAdapter;

impl HarnessAdapter for QwenCodeAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::QwenCode
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_routing_arguments(
            &context.user_arguments,
            &["--model", "-m", "--fallback-model"],
        )?;
        let model_id = &context.model.resolved_id;
        let mut public_environment = provider_environment();
        public_environment.insert(
            "OPENAI_BASE_URL".to_owned(),
            PROVIDER_BASE_URL_PLACEHOLDER.to_owned(),
        );
        public_environment.insert("OPENAI_MODEL".to_owned(), model_id.clone());
        public_environment.insert("QWEN_HOME".to_owned(), CONFIG_PATH_PLACEHOLDER.to_owned());
        let settings = serde_json::to_string(&json!({
            "model": {"name": model_id},
            "modelProviders": {"openai": QWEN_CODE_MODEL_CATALOG_PLACEHOLDER},
            "security": {"auth": {"selectedType": "openai"}}
        }))
        .map_err(|error| PlanError::InvalidField {
            field: "configurationOverlays.files.contentTemplate",
            message: format!("could not serialize Qwen Code settings: {error}"),
        })?;
        let mut arguments = vec!["--model".to_owned(), model_id.clone()];
        arguments.extend(context.user_arguments.iter().cloned());

        build_direct_plan(
            context,
            DirectLaunch {
                arguments,
                credential_target: CREDENTIAL_TARGET,
                public_environment,
                removed_environment: BTreeSet::from([
                    "DASHSCOPE_API_KEY".to_owned(),
                    "QWEN_API_KEY".to_owned(),
                    "QWEN_BASE_URL".to_owned(),
                    "QWEN_MODEL".to_owned(),
                ]),
                temporary_artifacts: Vec::new(),
                configuration_overlays: vec![ConfigurationOverlay {
                    id: CONFIG_OVERLAY_ID.to_owned(),
                    path_hint: "qwen".to_owned(),
                    source_path: format!("{USER_HOME_PLACEHOLDER}/.qwen"),
                    files: vec![OverlayFile {
                        path: "settings.json".to_owned(),
                        mode: TemporaryArtifactMode::OwnerFile,
                        content_template: settings,
                        policy: OverlayFilePolicy::MergeJson,
                    }],
                    lifecycle: ArtifactLifecycle::Launch,
                }],
            },
        )
    }
}

impl HarnessAdapter for PersistentQwenCodeAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::QwenCode
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_routing_arguments(
            &context.user_arguments,
            &["--model", "-m", "--fallback-model"],
        )?;
        let mut arguments = vec![
            "--auth-type".to_owned(),
            "openai".to_owned(),
            "--model".to_owned(),
            context.model.resolved_id.clone(),
        ];
        arguments.extend(context.user_arguments.iter().cloned());

        build_direct_plan(
            context,
            DirectLaunch {
                arguments,
                credential_target: "NAN_API_KEY",
                public_environment: provider_environment(),
                removed_environment: BTreeSet::from([
                    "DASHSCOPE_API_KEY".to_owned(),
                    "OPENAI_API_KEY".to_owned(),
                    "QWEN_API_KEY".to_owned(),
                    "QWEN_BASE_URL".to_owned(),
                    "QWEN_MODEL".to_owned(),
                ]),
                temporary_artifacts: Vec::new(),
                configuration_overlays: Vec::new(),
            },
        )
    }
}
