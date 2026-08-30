use crate::direct::{
    DirectLaunch, build_direct_plan, provider_environment, validate_routing_arguments,
};
use crate::search::nan_search_mcp_overlay;
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, ConfigurationOverlay, KIMI_CODE_MODEL_CATALOG_PLACEHOLDER, OverlayFile,
    OverlayFilePolicy, PROVIDER_BASE_URL_PLACEHOLDER, SELECTED_MODEL_CAPABILITIES_PLACEHOLDER,
    SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER, SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER,
    SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER, TemporaryArtifactMode, USER_HOME_PLACEHOLDER,
};
use nan_harness_core::{HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError};
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "KIMI_MODEL_API_KEY";
const CONFIG_OVERLAY_ID: &str = "kimi-code-home";
const CONFIG_PATH: &str = "{artifact:kimi-code-home}";

#[derive(Debug, Default)]
pub struct KimiCodeAdapter;

impl HarnessAdapter for KimiCodeAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::KimiCode
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_routing_arguments(&context.user_arguments, &["--model", "-m"])?;
        let mut public_environment = provider_environment();
        public_environment.insert("KIMI_CODE_HOME".to_owned(), CONFIG_PATH.to_owned());
        public_environment.insert(
            "KIMI_MODEL_NAME".to_owned(),
            context.model.resolved_id.clone(),
        );
        public_environment.insert("KIMI_MODEL_PROVIDER_TYPE".to_owned(), "openai".to_owned());
        public_environment.insert(
            "KIMI_MODEL_BASE_URL".to_owned(),
            PROVIDER_BASE_URL_PLACEHOLDER.to_owned(),
        );
        public_environment.insert(
            "KIMI_MODEL_DISPLAY_NAME".to_owned(),
            SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER.to_owned(),
        );
        public_environment.insert(
            "KIMI_MODEL_MAX_CONTEXT_SIZE".to_owned(),
            SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER.to_owned(),
        );
        public_environment.insert(
            "KIMI_MODEL_MAX_OUTPUT_SIZE".to_owned(),
            SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER.to_owned(),
        );
        public_environment.insert(
            "KIMI_MODEL_CAPABILITIES".to_owned(),
            SELECTED_MODEL_CAPABILITIES_PLACEHOLDER.to_owned(),
        );

        build_direct_plan(
            context,
            DirectLaunch {
                arguments: context.user_arguments.clone(),
                credential_target: CREDENTIAL_TARGET,
                public_environment,
                removed_environment: BTreeSet::from([
                    "KIMI_API_KEY".to_owned(),
                    "KIMI_BASE_URL".to_owned(),
                    "OPENAI_API_KEY".to_owned(),
                    "OPENAI_BASE_URL".to_owned(),
                ]),
                temporary_artifacts: Vec::new(),
                configuration_overlays: vec![ConfigurationOverlay {
                    id: CONFIG_OVERLAY_ID.to_owned(),
                    path_hint: "kimi-code".to_owned(),
                    source_path: format!("{USER_HOME_PLACEHOLDER}/.kimi-code"),
                    files: vec![
                        OverlayFile {
                            path: "config.toml".to_owned(),
                            mode: TemporaryArtifactMode::OwnerFile,
                            content_template: KIMI_CODE_MODEL_CATALOG_PLACEHOLDER.to_owned(),
                            policy: OverlayFilePolicy::MergeToml,
                        },
                        OverlayFile {
                            path: "mcp.json".to_owned(),
                            mode: TemporaryArtifactMode::OwnerFile,
                            content_template: nan_search_mcp_overlay(CREDENTIAL_TARGET),
                            policy: OverlayFilePolicy::MergeJson,
                        },
                    ],
                    lifecycle: ArtifactLifecycle::Launch,
                }],
            },
        )
    }
}
