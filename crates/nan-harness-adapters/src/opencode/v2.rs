use std::collections::BTreeSet;

use nan_harness_core::launch_plan::{
    NAN_SEARCH_BLOCK_BEGIN, NAN_SEARCH_BLOCK_END, OPENCODE_V2_MODEL_CATALOG_PLACEHOLDER,
};
use nan_harness_core::{LaunchPlan, NativeContextLimit, PlanContext, PlanError};
use nan_harness_i18n::{DiagnosticText, messages};
use serde_json::json;

use super::CREDENTIAL_TARGET;
use crate::direct::{
    DirectLaunch, PROVIDER_URL_ENVIRONMENT, build_direct_plan, provider_environment,
    validate_routing_arguments,
};
use crate::search::{NAN_SEARCH_MCP_ID, nan_search_mcp_command};

pub(super) fn plan(context: &PlanContext) -> Result<LaunchPlan, PlanError> {
    validate_routing_arguments(
        &context.user_arguments,
        &[
            "--model",
            "-m",
            "--server",
            "--pure",
            "--variant",
            "--standalone",
        ],
    )?;
    let mut config = json!({
        "model": format!("nan/{}", context.model.resolved_id),
        "enabled_providers": ["nan"],
        "providers": {"nan": {
            "name": "NaN",
            "env": [CREDENTIAL_TARGET],
            "package": "@opencode/ai/providers/openai-compatible",
            "settings": {
                "apiKey": "{env:NAN_API_KEY}",
                "baseURL": format!("{{env:{PROVIDER_URL_ENVIRONMENT}}}")
            },
            "models": OPENCODE_V2_MODEL_CATALOG_PLACEHOLDER
        }}
    });
    if let Some(limit) = &context.context_limit
        && let NativeContextLimit::OpenCodeBuffer { buffer_tokens } = limit.native
    {
        config["compaction"] = json!({"auto": true, "buffer": buffer_tokens});
    }
    let mut config = encode(&config)?;
    config.pop();
    let search = encode(&json!({"servers": {NAN_SEARCH_MCP_ID: {
        "type": "local", "command": nan_search_mcp_command(CREDENTIAL_TARGET), "disabled": false
    }}}))?;
    config.push_str(NAN_SEARCH_BLOCK_BEGIN);
    config.push_str(",\"mcp\":");
    config.push_str(&search);
    config.push_str(NAN_SEARCH_BLOCK_END);
    config.push('}');
    let mut public_environment = provider_environment();
    public_environment.insert("OPENCODE_CONFIG_CONTENT".to_owned(), config);
    build_direct_plan(
        context,
        DirectLaunch {
            arguments: arguments(context),
            credential_target: CREDENTIAL_TARGET,
            public_environment,
            removed_environment: BTreeSet::new(),
            temporary_artifacts: Vec::new(),
            configuration_overlays: Vec::new(),
        },
    )
}

fn arguments(context: &PlanContext) -> Vec<String> {
    let mut arguments = context.user_arguments.clone();
    // Run/mini own their flags; TUI uses the root configuration for its model.
    let offset = usize::from(
        arguments
            .first()
            .is_some_and(|arg| matches!(arg.as_str(), "run" | "mini")),
    );
    let mut managed = vec!["--standalone".to_owned()];
    if offset == 1 {
        managed.extend([
            "--model".to_owned(),
            format!("nan/{}", context.model.resolved_id),
        ]);
    }
    arguments.splice(offset..offset, managed);
    arguments
}

fn encode(value: &serde_json::Value) -> Result<String, PlanError> {
    serde_json::to_string(value).map_err(|error| PlanError::InvalidField {
        field: "environment.public.OPENCODE_CONFIG_CONTENT",
        message: DiagnosticText::new(|locale| {
            messages::detail_serialize_opencode_configuration_failed(locale, &error)
        }),
    })
}
