use crate::direct::{
    DirectLaunch, build_direct_plan, provider_environment, validate_routing_arguments,
};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, BRIDGE_BASE_URL_PLACEHOLDER, NAN_SEARCH_BLOCK_BEGIN, NAN_SEARCH_BLOCK_END,
    PI_MODEL_CATALOG_PLACEHOLDER, PROVIDER_BASE_URL_PLACEHOLDER, TemporaryArtifact,
    TemporaryArtifactKind, TemporaryArtifactMode,
};
use nan_harness_core::{
    HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError, WebSearchPolicy,
};
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "NAN_API_KEY";
const EXTENSION_ARTIFACT_ID: &str = "pi-provider-extension";
const EXTENSION_PATH_PLACEHOLDER: &str = "{artifact:pi-provider-extension}";

/// Selects whether the Pi search extension acts as a fallback or an override.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PiSearchMode {
    /// Register NaN search only when no loaded extension provides `web_search`.
    Auto,
    /// Register NaN search even when another extension provides `web_search`.
    Force,
}

#[derive(Debug, Default)]
pub struct PiAdapter;

impl HarnessAdapter for PiAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Pi
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        pi_family_plan(context)
    }
}

#[derive(Debug, Default)]
pub struct PrimeAgentAdapter;

impl HarnessAdapter for PrimeAgentAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::PrimeAgent
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        pi_family_plan(context)
    }
}

fn pi_family_plan(context: &PlanContext) -> Result<LaunchPlan, PlanError> {
    validate_routing_arguments(
        &context.user_arguments,
        &["--model", "--provider", "--api-key", "--models"],
    )?;
    let mut arguments = vec![
        "--extension".to_owned(),
        EXTENSION_PATH_PLACEHOLDER.to_owned(),
        "--provider".to_owned(),
        "nan".to_owned(),
        "--model".to_owned(),
        context.model.resolved_id.clone(),
        "--models".to_owned(),
        "nan/*".to_owned(),
    ];
    arguments.extend(context.user_arguments.iter().cloned());

    build_direct_plan(
        context,
        DirectLaunch {
            arguments,
            credential_target: CREDENTIAL_TARGET,
            public_environment: provider_environment(),
            removed_environment: BTreeSet::new(),
            temporary_artifacts: vec![TemporaryArtifact {
                id: EXTENSION_ARTIFACT_ID.to_owned(),
                kind: TemporaryArtifactKind::File,
                path_hint: "nan-provider.mjs".to_owned(),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template: Some(provider_extension(context.web_search_policy)),
                lifecycle: ArtifactLifecycle::Launch,
            }],
            configuration_overlays: Vec::new(),
        },
    )
}

fn provider_extension(search_policy: WebSearchPolicy) -> String {
    let search_registration = search_registration(
        &serde_json::Value::String(format!("{BRIDGE_BASE_URL_PLACEHOLDER}/v1/search")).to_string(),
        "apiKey",
        if search_policy == WebSearchPolicy::Force {
            PiSearchMode::Force
        } else {
            PiSearchMode::Auto
        },
    );
    format!(
        r#"import {{ Type }} from "@earendil-works/pi-ai";

const baseUrl = "{PROVIDER_BASE_URL_PLACEHOLDER}".replace(/\/+$/, "");
const profiles = {PI_MODEL_CATALOG_PLACEHOLDER};

export default function registerNan(pi) {{
  const apiKey = process.env.NAN_API_KEY;
  if (!apiKey) throw new Error("NAN_API_KEY is required for the NaN provider");

  const models = Object.entries(profiles).map(([id, profile]) => ({{
    id,
    name: profile.name,
    reasoning: profile.reasoningPolicy.kind !== "unsupported" && profile.reasoningPolicy.kind !== "unknown",
    input: profile.input,
    cost: {{ input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }},
    contextWindow: profile.contextWindow,
    maxTokens: profile.maxTokens,
    compat: {{
      supportsDeveloperRole: false,
      supportsReasoningEffort: profile.reasoningPolicy.kind === "effort",
      maxTokensField: "max_tokens",
      ...(profile.reasoningPolicy.kind === "effort" ? {{ thinkingLevelMap: {{ low: "low", medium: "medium", high: "high" }} }} : {{}})
    }}
  }}));

  pi.registerProvider("nan", {{
    baseUrl,
    apiKey,
    authHeader: true,
    api: "openai-completions",
    models
  }});
{NAN_SEARCH_BLOCK_BEGIN}
{search_registration}
{NAN_SEARCH_BLOCK_END}
}}
"#
    )
}

/// Renders the standalone Pi/Prime Agent extension used by native configuration.
#[must_use]
pub fn render_pi_search_extension(base_url: &str, mode: PiSearchMode) -> String {
    let endpoint = format!("{}/search", base_url.trim_end_matches('/'));
    let endpoint = serde_json::Value::String(endpoint).to_string();
    let registration = search_registration(
        &endpoint,
        "ctx.modelRegistry.getApiKeyForProvider(\"nan\")",
        mode,
    );
    format!(
        r#"import {{ Type }} from "@earendil-works/pi-ai";

export default function registerNanSearch(pi) {{
{registration}
}}
"#
    )
}

fn search_registration(
    endpoint_expression: &str,
    credential_expression: &str,
    mode: PiSearchMode,
) -> String {
    let force_search = mode == PiSearchMode::Force;
    // Pi checks extension tool-name conflicts immediately after loading factories. Deferring
    // registration until resource discovery avoids that static collision and observes tools
    // registered by package extensions during either factory load or `session_start`.
    format!(
        r#"  const forceNanSearch = {force_search};
  pi.on("resources_discover", () => {{
    const hasWebSearch = pi.getAllTools().some((tool) => tool.name === "web_search");
    if (!forceNanSearch && hasWebSearch) return;

    pi.registerTool({{
      name: "web_search",
      label: "web_search",
      description: "web_search",
      parameters: Type.Object({{
        query: Type.String(),
        maxResults: Type.Optional(Type.Number({{ minimum: 1, maximum: 20 }})),
        allowedDomains: Type.Optional(Type.Array(Type.String())),
        blockedDomains: Type.Optional(Type.Array(Type.String()))
      }}),
      async execute(_toolCallId, params, signal, _onUpdate, ctx) {{
        const searchApiKey = await {credential_expression};
        if (!searchApiKey) throw new Error("NaN provider credential is required for web search");
        const response = await fetch({endpoint_expression}, {{
          method: "POST",
          headers: {{
            authorization: `Bearer ${{searchApiKey}}`,
            "content-type": "application/json"
          }},
          body: JSON.stringify(params),
          signal
        }});
        if (!response.ok) throw new Error(`NH-SEARCH-${{response.status}}`);
        const result = await response.json();
        return {{
          content: [{{ type: "text", text: result.summary }}],
          details: {{ results: result.results }}
        }};
      }}
    }});
  }});"#
    )
}
