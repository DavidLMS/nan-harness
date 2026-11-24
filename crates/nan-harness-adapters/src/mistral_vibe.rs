use crate::direct::{
    DirectLaunch, PROVIDER_CREDENTIAL_REFERENCE, build_direct_plan, provider_environment,
    validate_routing_arguments,
};
use crate::search::{NAN_SEARCH_MCP_BINARY, NAN_SEARCH_MCP_ID, nan_search_endpoint};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, ConfigurationOverlay, MISTRAL_VIBE_MODEL_CATALOG_PLACEHOLDER,
    NAN_SEARCH_BLOCK_BEGIN, NAN_SEARCH_BLOCK_END, OverlayFile, OverlayFilePolicy,
    PROVIDER_BASE_URL_PLACEHOLDER, TemporaryArtifactMode, VIBE_HOME_PLACEHOLDER,
};
use nan_harness_core::{
    ContextLimit, HarnessAdapter, HarnessKind, LaunchPlan, NativeContextLimit, PlanContext,
    PlanError,
};
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "NAN_VIBE_API_KEY";
const CONFIG_OVERLAY_ID: &str = "vibe-home";
const CONFIG_PATH: &str = "{artifact:vibe-home}";

/// Environment overrides Vibe reads on top of `config.toml`, plus the provider
/// credentials and endpoints that could route a launch away from NaN.
const REMOVED_ENVIRONMENT: [&str; 11] = [
    "MISTRAL_API_KEY",
    "OPENAI_API_KEY",
    "OPENAI_BASE_URL",
    "VIBE_ACTIVE_MODEL",
    "VIBE_PROVIDERS",
    "VIBE_MODELS",
    "VIBE_MCP_SERVERS",
    "VIBE_ENABLE_TELEMETRY",
    "VIBE_ENABLE_UPDATE_CHECKS",
    "VIBE_ENABLE_AUTO_UPDATE",
    "VIBE_EXPERIMENTAL_ENABLE_MODEL_CATALOG",
];

#[derive(Debug, Default)]
pub struct MistralVibeAdapter;

impl HarnessAdapter for MistralVibeAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::MistralVibe
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        // Upstream `--model` only applies to headless sessions; routing owns the
        // model for every launch through `active_model`.
        validate_routing_arguments(&context.user_arguments, &["--model"])?;
        let mut public_environment = provider_environment();
        public_environment.insert("VIBE_HOME".to_owned(), CONFIG_PATH.to_owned());

        build_direct_plan(
            context,
            DirectLaunch {
                arguments: context.user_arguments.clone(),
                credential_target: CREDENTIAL_TARGET,
                public_environment,
                removed_environment: REMOVED_ENVIRONMENT
                    .iter()
                    .map(|name| (*name).to_owned())
                    .collect::<BTreeSet<_>>(),
                temporary_artifacts: Vec::new(),
                configuration_overlays: vec![ConfigurationOverlay {
                    id: CONFIG_OVERLAY_ID.to_owned(),
                    path_hint: "vibe-home".to_owned(),
                    source_path: VIBE_HOME_PLACEHOLDER.to_owned(),
                    files: vec![OverlayFile {
                        path: "config.toml".to_owned(),
                        mode: TemporaryArtifactMode::OwnerFile,
                        content_template: vibe_config(
                            &context.model.resolved_id,
                            context.context_limit.as_ref(),
                        ),
                        policy: OverlayFilePolicy::MergeTomlMcpServers,
                    }],
                    lifecycle: ArtifactLifecycle::Launch,
                }],
            },
        )
    }
}

fn vibe_config(model_id: &str, context_limit: Option<&ContextLimit>) -> String {
    // A global threshold applies to every model without its own; the managed
    // catalog never sets per-model thresholds.
    let compaction = match context_limit.map(|limit| &limit.native) {
        Some(NativeContextLimit::VibeThreshold { threshold_tokens }) => {
            format!("auto_compact_threshold = {threshold_tokens}\n")
        }
        _ => String::new(),
    };
    let active_model = toml_string(model_id);
    let search_endpoint = toml_string(&nan_search_endpoint());
    let search_server = toml_string(NAN_SEARCH_MCP_ID);
    let search_command = toml_string(NAN_SEARCH_MCP_BINARY);
    let token_secret = toml_string(&format!("{{secret:{PROVIDER_CREDENTIAL_REFERENCE}}}"));
    // Vibe starts MCP servers with a reduced environment, so the helper receives
    // its token through the server's own `env` table rather than inheritance.
    format!(
        "active_model = {active_model}\n\
         enable_telemetry = false\n\
         enable_update_checks = false\n\
         enable_auto_update = false\n\
         experimental_enable_model_catalog = false\n\
         {compaction}\
         \n\
         [[providers]]\n\
         name = \"nan\"\n\
         api_base = {base_url}\n\
         api_key_env_var = \"{CREDENTIAL_TARGET}\"\n\
         api_style = \"openai\"\n\
         backend = \"generic\"\n\
         reasoning_field_name = \"reasoning_content\"\n\
         \n\
         {MISTRAL_VIBE_MODEL_CATALOG_PLACEHOLDER}\
         {NAN_SEARCH_BLOCK_BEGIN}\n\
         [[mcp_servers]]\n\
         name = {search_server}\n\
         transport = \"stdio\"\n\
         command = {search_command}\n\
         args = [\"__search-mcp\", \"--endpoint\", {search_endpoint}, \"--token-env\", \"{CREDENTIAL_TARGET}\"]\n\
         env = {{ {CREDENTIAL_TARGET} = {token_secret} }}\n\
         {NAN_SEARCH_BLOCK_END}",
        base_url = toml_string(PROVIDER_BASE_URL_PLACEHOLDER),
    )
}

/// Quotes a value as a TOML basic string. JSON string escapes are a subset of
/// TOML's, so the JSON encoder also covers quotes, backslashes and controls.
fn toml_string(value: &str) -> String {
    serde_json::Value::String(value.to_owned()).to_string()
}
