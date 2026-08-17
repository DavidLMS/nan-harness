use crate::direct::{
    DirectLaunch, build_direct_plan, provider_environment, validate_routing_arguments,
};
use nan_harness_core::launch_plan::PROVIDER_BASE_URL_PLACEHOLDER;
use nan_harness_core::{HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError};
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "OPENAI_API_KEY";

#[derive(Debug, Default)]
pub struct QwenCodeAdapter;

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
                configuration_overlays: Vec::new(),
            },
        )
    }
}
