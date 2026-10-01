use crate::direct::{
    DirectLaunch, PROVIDER_URL_ENVIRONMENT, build_direct_plan, provider_environment,
    validate_routing_arguments,
};
use crate::search::{NAN_SEARCH_MCP_ID, nan_search_mcp_command};
use nan_harness_core::launch_plan::{
    NAN_SEARCH_BLOCK_BEGIN, NAN_SEARCH_BLOCK_END, OPENCODE_MODEL_CATALOG_PLACEHOLDER,
};
use nan_harness_core::{
    HarnessAdapter, HarnessKind, LaunchPlan, NativeContextLimit, PlanContext, PlanError,
};
use nan_harness_i18n::DiagnosticText;
use nan_harness_i18n::messages as detail_messages;
use serde_json::json;
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "NAN_API_KEY";

#[derive(Debug, Default)]
pub struct MimoCodeAdapter;

impl HarnessAdapter for MimoCodeAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::MimoCode
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_routing_arguments(&context.user_arguments, &["--model", "-m", "--attach"])?;
        let model_id = &context.model.resolved_id;
        let compaction = context
            .context_limit
            .as_ref()
            .and_then(|limit| match limit.native {
                NativeContextLimit::MimoContext { max_context_tokens } => Some(json!({
                    "auto": true,
                    "max_context": max_context_tokens
                })),
                _ => None,
            });
        // Built-in tiers and small-model helpers must stay on the selected NaN model.
        // Restricting enabled providers also fails closed for user-defined external references.
        let mut config_value = json!({
            "enabled_providers": ["nan"],
            "disabled_providers": [],
            "model": format!("nan/{model_id}"),
            "small_model": format!("nan/{model_id}"),
            "vision_model": format!("nan/{model_id}"),
            "model_groups": {
                "ultra": format!("nan/{model_id}"),
                "standard": format!("nan/{model_id}"),
                "lite": format!("nan/{model_id}")
            },
            "provider": {
                "nan": {
                    "npm": "@ai-sdk/openai-compatible",
                    "name": "NaN",
                    "only_configured_models": true,
                    "options": {
                        "apiKey": "{env:NAN_API_KEY}",
                        "baseURL": format!("{{env:{PROVIDER_URL_ENVIRONMENT}}}")
                    },
                    "models": OPENCODE_MODEL_CATALOG_PLACEHOLDER
                }
            }
        });
        if let Some(compaction) = compaction {
            config_value["compaction"] = compaction;
        }
        let mut config =
            serde_json::to_string(&config_value).map_err(|error| PlanError::InvalidField {
                field: "environment.public.MIMOCODE_CONFIG_CONTENT",
                message: DiagnosticText::new(|locale| {
                    detail_messages::detail_serialize_mimo_configuration_failed(locale, &(error))
                }),
            })?;
        config.pop();
        let search = serde_json::to_string(&json!({
            NAN_SEARCH_MCP_ID: {
                "type": "local",
                "command": nan_search_mcp_command(CREDENTIAL_TARGET),
                "enabled": true
            }
        }))
        .map_err(|error| PlanError::InvalidField {
            field: "environment.public.MIMOCODE_CONFIG_CONTENT",
            message: DiagnosticText::new(|locale| {
                detail_messages::detail_serialize_mimo_configuration_failed(locale, &(error))
            }),
        })?;
        config.push_str(NAN_SEARCH_BLOCK_BEGIN);
        config.push_str(",\"mcp\":");
        config.push_str(&search);
        config.push_str(NAN_SEARCH_BLOCK_END);
        config.push('}');
        let mut public_environment = provider_environment();
        public_environment.insert("MIMOCODE_CONFIG_CONTENT".to_owned(), config);
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
                configuration_overlays: Vec::new(),
            },
        )
    }
}
