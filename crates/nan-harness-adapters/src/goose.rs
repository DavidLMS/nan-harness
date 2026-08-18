use crate::direct::{
    DirectLaunch, build_direct_plan, provider_environment, validate_routing_arguments,
};
use nan_harness_core::launch_plan::{
    GOOSE_MODEL_CATALOG_PLACEHOLDER, PROVIDER_BASE_URL_PLACEHOLDER,
};
use nan_harness_core::{HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError};
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "OPENAI_API_KEY";

#[derive(Debug, Default)]
pub struct GooseAdapter;

impl HarnessAdapter for GooseAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Goose
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_routing_arguments(&context.user_arguments, &["--provider", "--model"])?;
        let arguments = if context.user_arguments.is_empty() {
            vec!["session".to_owned()]
        } else {
            context.user_arguments.clone()
        };
        let mut public_environment = provider_environment();
        public_environment.insert(
            "OPENAI_BASE_URL".to_owned(),
            PROVIDER_BASE_URL_PLACEHOLDER.to_owned(),
        );
        public_environment.insert("GOOSE_PROVIDER".to_owned(), "openai".to_owned());
        public_environment.insert("GOOSE_MODEL".to_owned(), context.model.resolved_id.clone());
        public_environment.insert(
            "GOOSE_PREDEFINED_MODELS".to_owned(),
            GOOSE_MODEL_CATALOG_PLACEHOLDER.to_owned(),
        );

        build_direct_plan(
            context,
            DirectLaunch {
                arguments,
                credential_target: CREDENTIAL_TARGET,
                public_environment,
                removed_environment: BTreeSet::from([
                    "OPENAI_BASE_PATH".to_owned(),
                    "OPENAI_HOST".to_owned(),
                    "OPENAI_ORGANIZATION".to_owned(),
                    "OPENAI_PROJECT".to_owned(),
                ]),
                temporary_artifacts: Vec::new(),
                configuration_overlays: Vec::new(),
            },
        )
    }
}
