use crate::direct::{
    DirectLaunch, build_direct_plan, provider_environment, validate_routing_arguments,
};
use nan_harness_core::launch_plan::PROVIDER_BASE_URL_PLACEHOLDER;
use nan_harness_core::{HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError};
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "OPENAI_API_KEY";

#[derive(Debug, Default)]
pub struct HermesAdapter;

impl HarnessAdapter for HermesAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Hermes
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_routing_arguments(&context.user_arguments, &["--model", "-m", "--provider"])?;
        let mut public_environment = provider_environment();
        public_environment.insert(
            "CUSTOM_BASE_URL".to_owned(),
            PROVIDER_BASE_URL_PLACEHOLDER.to_owned(),
        );
        let mut arguments = vec![
            "--provider".to_owned(),
            "custom".to_owned(),
            "--model".to_owned(),
            context.model.resolved_id.clone(),
        ];
        arguments.extend(context.user_arguments.iter().cloned());

        build_direct_plan(
            context,
            DirectLaunch {
                arguments,
                credential_target: CREDENTIAL_TARGET,
                public_environment,
                removed_environment: BTreeSet::from([
                    "HERMES_INFERENCE_MODEL".to_owned(),
                    "HERMES_INFERENCE_PROVIDER".to_owned(),
                    "OPENAI_BASE_URL".to_owned(),
                ]),
                temporary_artifacts: Vec::new(),
                configuration_overlays: Vec::new(),
            },
        )
    }
}
