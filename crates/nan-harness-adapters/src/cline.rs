use crate::direct::{
    DirectLaunch, build_direct_plan, provider_environment, validate_routing_arguments,
};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, ConfigurationOverlay, OverlayFile, OverlayFilePolicy,
    PROVIDER_BASE_URL_PLACEHOLDER, TemporaryArtifactMode, USER_HOME_PLACEHOLDER,
};
use nan_harness_core::{HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError};
use serde_json::json;
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "OPENAI_API_KEY";
const CONFIG_OVERLAY_ID: &str = "cline-config";
const CONFIG_PATH: &str = "{artifact:cline-config}";

#[derive(Debug, Default)]
pub struct ClineAdapter;

impl HarnessAdapter for ClineAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Cline
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_routing_arguments(
            &context.user_arguments,
            &[
                "--config",
                "--data-dir",
                "--provider",
                "-P",
                "--model",
                "-m",
                "--key",
                "-k",
            ],
        )?;
        let model_id = &context.model.resolved_id;
        let provider_settings = serde_json::to_string(&json!({
            "lastUsedProvider": "openai-compatible",
            "providers": {
                "openai-compatible": {
                    "settings": {
                        "baseUrl": PROVIDER_BASE_URL_PLACEHOLDER,
                        "model": model_id,
                        "provider": "openai-compatible"
                    },
                    "tokenSource": "manual",
                    "updatedAt": "1970-01-01T00:00:00.000Z"
                }
            },
            "version": 1
        }))
        .map_err(|error| PlanError::InvalidField {
            field: "configurationOverlays.files.contentTemplate",
            message: format!("could not serialize Cline provider settings: {error}"),
        })?;
        let mut arguments = vec![
            "--config".to_owned(),
            CONFIG_PATH.to_owned(),
            "--provider".to_owned(),
            "openai-compatible".to_owned(),
            "--model".to_owned(),
            model_id.clone(),
        ];
        arguments.extend(context.user_arguments.iter().cloned());

        build_direct_plan(
            context,
            DirectLaunch {
                arguments,
                credential_target: CREDENTIAL_TARGET,
                public_environment: provider_environment(),
                removed_environment: BTreeSet::from([
                    "CLINE_DEFAULT_MODEL_ID".to_owned(),
                    "CLINE_MODEL".to_owned(),
                    "CLINE_PROVIDER".to_owned(),
                    "OPENAI_BASE_URL".to_owned(),
                ]),
                temporary_artifacts: Vec::new(),
                configuration_overlays: vec![ConfigurationOverlay {
                    id: CONFIG_OVERLAY_ID.to_owned(),
                    path_hint: "cline".to_owned(),
                    source_path: format!("{USER_HOME_PLACEHOLDER}/.cline"),
                    files: vec![OverlayFile {
                        path: "data/settings/providers.json".to_owned(),
                        mode: TemporaryArtifactMode::OwnerFile,
                        content_template: provider_settings,
                        policy: OverlayFilePolicy::Replace,
                    }],
                    lifecycle: ArtifactLifecycle::Launch,
                }],
            },
        )
    }
}
