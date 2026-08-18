use crate::direct::{
    DirectLaunch, build_direct_plan, provider_environment, validate_routing_arguments,
};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, ConfigurationOverlay, OPENCLAW_MODEL_ALIASES_PLACEHOLDER,
    OPENCLAW_MODEL_CATALOG_PLACEHOLDER, OverlayFile, OverlayFilePolicy,
    PROVIDER_BASE_URL_PLACEHOLDER, TemporaryArtifactMode, USER_HOME_PLACEHOLDER,
};
use nan_harness_core::{HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError};
use serde_json::json;
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "NAN_API_KEY";
const CONFIG_OVERLAY_ID: &str = "openclaw-config";
const CONFIG_PATH: &str = "{artifact:openclaw-config}/nan-harness.json";

#[derive(Debug, Default)]
pub struct OpenClawAdapter;

impl HarnessAdapter for OpenClawAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::OpenClaw
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_routing_arguments(
            &context.user_arguments,
            &["--model", "--profile", "--dev", "--container"],
        )?;
        let model_id = &context.model.resolved_id;
        let model_reference = format!("nan/{model_id}");
        let config = serde_json::to_string(&json!({
            "$include": "./openclaw.json",
            "agents": {
                "defaults": {
                    "model": {"primary": model_reference},
                    "models": OPENCLAW_MODEL_ALIASES_PLACEHOLDER
                }
            },
            "models": {
                "mode": "merge",
                "providers": {
                    "nan": {
                        "api": "openai-completions",
                        "apiKey": {
                            "id": CREDENTIAL_TARGET,
                            "provider": "default",
                            "source": "env"
                        },
                        "baseUrl": PROVIDER_BASE_URL_PLACEHOLDER,
                        "models": OPENCLAW_MODEL_CATALOG_PLACEHOLDER
                    }
                }
            }
        }))
        .map_err(|error| PlanError::InvalidField {
            field: "configurationOverlays.files.contentTemplate",
            message: format!("could not serialize OpenClaw configuration: {error}"),
        })?;
        let mut public_environment = provider_environment();
        public_environment.insert("OPENCLAW_CONFIG_PATH".to_owned(), CONFIG_PATH.to_owned());
        public_environment.insert(
            "OPENCLAW_INCLUDE_ROOTS".to_owned(),
            USER_HOME_PLACEHOLDER.to_owned(),
        );
        let arguments = if context.user_arguments.is_empty() {
            vec!["chat".to_owned()]
        } else {
            context.user_arguments.clone()
        };

        build_direct_plan(
            context,
            DirectLaunch {
                arguments,
                credential_target: CREDENTIAL_TARGET,
                public_environment,
                removed_environment: BTreeSet::new(),
                temporary_artifacts: Vec::new(),
                configuration_overlays: vec![ConfigurationOverlay {
                    id: CONFIG_OVERLAY_ID.to_owned(),
                    path_hint: "openclaw".to_owned(),
                    source_path: format!("{USER_HOME_PLACEHOLDER}/.openclaw"),
                    files: vec![
                        OverlayFile {
                            path: "openclaw.json".to_owned(),
                            mode: TemporaryArtifactMode::OwnerFile,
                            content_template: "{}".to_owned(),
                            policy: OverlayFilePolicy::Preserve,
                        },
                        OverlayFile {
                            path: "nan-harness.json".to_owned(),
                            mode: TemporaryArtifactMode::OwnerFile,
                            content_template: config,
                            policy: OverlayFilePolicy::Replace,
                        },
                    ],
                    lifecycle: ArtifactLifecycle::Launch,
                }],
            },
        )
    }
}
