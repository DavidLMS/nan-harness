use crate::direct::{
    DirectLaunch, build_direct_plan, provider_environment, validate_routing_arguments,
};
use nan_harness_core::launch_plan::PROVIDER_BASE_URL_PLACEHOLDER;
use nan_harness_core::{HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError};
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "AIDER_OPENAI_API_KEY";

#[derive(Debug, Default)]
pub struct AiderAdapter;

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
                temporary_artifacts: Vec::new(),
                configuration_overlays: Vec::new(),
            },
        )
    }
}
