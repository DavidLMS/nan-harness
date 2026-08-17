use crate::direct::{
    DirectLaunch, PROVIDER_URL_ENVIRONMENT, build_direct_plan, describe_model,
    provider_environment, validate_routing_arguments,
};
use nan_harness_core::{HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError};
use serde_json::json;
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "NAN_API_KEY";

#[derive(Debug, Default)]
pub struct OpenCodeAdapter;

impl HarnessAdapter for OpenCodeAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::OpenCode
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_routing_arguments(&context.user_arguments, &["--model", "-m"])?;
        let model_id = &context.model.resolved_id;
        let model = describe_model(model_id);
        let config = serde_json::to_string(&json!({
            "enabled_providers": ["nan"],
            "provider": {
                "nan": {
                    "npm": "@ai-sdk/openai-compatible",
                    "name": "NaN",
                    "options": {
                        "apiKey": "{env:NAN_API_KEY}",
                        "baseURL": format!("{{env:{PROVIDER_URL_ENVIRONMENT}}}")
                    },
                    "models": {
                        (model_id): {
                            "name": model.display_name
                        }
                    }
                }
            }
        }))
        .map_err(|error| PlanError::InvalidField {
            field: "environment.public.OPENCODE_CONFIG_CONTENT",
            message: format!("could not serialize OpenCode configuration: {error}"),
        })?;
        let mut public_environment = provider_environment();
        public_environment.insert("OPENCODE_CONFIG_CONTENT".to_owned(), config);
        let mut arguments = vec!["--model".to_owned(), format!("nan/{model_id}")];
        arguments.extend(context.user_arguments.iter().cloned());

        build_direct_plan(
            context,
            DirectLaunch {
                arguments,
                credential_target: CREDENTIAL_TARGET,
                public_environment,
                removed_environment: BTreeSet::new(),
                temporary_artifacts: Vec::new(),
            },
        )
    }
}
