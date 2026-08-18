use crate::direct::{
    DirectLaunch, build_direct_plan, provider_environment, validate_routing_arguments,
};
use nan_harness_core::launch_plan::{
    AIDER_MODEL_METADATA_PLACEHOLDER, AIDER_MODEL_SETTINGS_PLACEHOLDER, ArtifactLifecycle,
    PROVIDER_BASE_URL_PLACEHOLDER, TemporaryArtifact, TemporaryArtifactKind, TemporaryArtifactMode,
};
use nan_harness_core::{HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError};
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "AIDER_OPENAI_API_KEY";

#[derive(Debug, Default)]
pub struct AiderAdapter;

#[derive(Debug, Default)]
pub struct PersistentAiderAdapter;

impl HarnessAdapter for AiderAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Aider
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_routing_arguments(
            &context.user_arguments,
            &[
                "--model",
                "-m",
                "--weak-model",
                "--editor-model",
                "--openai-api-key",
                "--openai-api-base",
                "--api-key",
                "--set-env",
                "--env-file",
                "--config",
                "-c",
                "--model-settings-file",
                "--model-metadata-file",
            ],
        )?;
        let model = format!("openai/{}", context.model.resolved_id);
        let mut arguments = vec![
            "--model".to_owned(),
            model.clone(),
            "--weak-model".to_owned(),
            model.clone(),
            "--editor-model".to_owned(),
            model,
            "--model-settings-file".to_owned(),
            "{artifact:aider-model-settings}".to_owned(),
            "--model-metadata-file".to_owned(),
            "{artifact:aider-model-metadata}".to_owned(),
        ];
        arguments.extend(context.user_arguments.iter().cloned());
        let mut public_environment = provider_environment();
        public_environment.insert(
            "AIDER_OPENAI_API_BASE".to_owned(),
            PROVIDER_BASE_URL_PLACEHOLDER.to_owned(),
        );

        build_direct_plan(
            context,
            DirectLaunch {
                arguments,
                credential_target: CREDENTIAL_TARGET,
                public_environment,
                removed_environment: BTreeSet::from([
                    "AIDER_API_KEY".to_owned(),
                    "OPENAI_API_BASE".to_owned(),
                    "OPENAI_API_KEY".to_owned(),
                    "OPENAI_BASE_URL".to_owned(),
                ]),
                temporary_artifacts: vec![
                    TemporaryArtifact {
                        id: "aider-model-settings".to_owned(),
                        kind: TemporaryArtifactKind::File,
                        path_hint: "aider-model-settings.json".to_owned(),
                        mode: TemporaryArtifactMode::OwnerFile,
                        content_template: Some(AIDER_MODEL_SETTINGS_PLACEHOLDER.to_owned()),
                        lifecycle: ArtifactLifecycle::Launch,
                    },
                    TemporaryArtifact {
                        id: "aider-model-metadata".to_owned(),
                        kind: TemporaryArtifactKind::File,
                        path_hint: "aider-model-metadata.json".to_owned(),
                        mode: TemporaryArtifactMode::OwnerFile,
                        content_template: Some(AIDER_MODEL_METADATA_PLACEHOLDER.to_owned()),
                        lifecycle: ArtifactLifecycle::Launch,
                    },
                ],
                configuration_overlays: Vec::new(),
            },
        )
    }
}

impl HarnessAdapter for PersistentAiderAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Aider
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_routing_arguments(
            &context.user_arguments,
            &[
                "--model",
                "-m",
                "--weak-model",
                "--editor-model",
                "--openai-api-key",
                "--openai-api-base",
                "--api-key",
                "--set-env",
                "--env-file",
                "--config",
                "-c",
                "--model-settings-file",
                "--model-metadata-file",
            ],
        )?;
        let model = format!("nan/{}", context.model.resolved_id);
        let mut arguments = vec![
            "--model".to_owned(),
            model.clone(),
            "--weak-model".to_owned(),
            model.clone(),
            "--editor-model".to_owned(),
            model,
        ];
        arguments.extend(context.user_arguments.iter().cloned());

        build_direct_plan(
            context,
            DirectLaunch {
                arguments,
                credential_target: "NAN_API_KEY",
                public_environment: provider_environment(),
                removed_environment: BTreeSet::from([
                    "AIDER_API_KEY".to_owned(),
                    "AIDER_OPENAI_API_BASE".to_owned(),
                    "AIDER_OPENAI_API_KEY".to_owned(),
                    "OPENAI_API_BASE".to_owned(),
                    "OPENAI_API_KEY".to_owned(),
                    "OPENAI_BASE_URL".to_owned(),
                ]),
                temporary_artifacts: Vec::new(),
                configuration_overlays: Vec::new(),
            },
        )
    }
}
