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
const EXTENSION_ARTIFACT_ID: &str = "omp-provider-extension";
const EXTENSION_PATH_PLACEHOLDER: &str = "{artifact:omp-provider-extension}";
const CONFIG_ARTIFACT_ID: &str = "omp-launch-config";
const CONFIG_PATH_PLACEHOLDER: &str = "{artifact:omp-launch-config}";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OmpSearchMode {
    Auto,
    Force,
}

#[derive(Debug, Default)]
pub struct OmpAdapter;

impl HarnessAdapter for OmpAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Omp
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_routing_arguments(
            &context.user_arguments,
            &[
                "--model",
                "--provider",
                "--api-key",
                "--models",
                "--config",
                "--smol",
                "--slow",
                "--plan",
            ],
        )?;
        let mut arguments = vec![
            "--extension".to_owned(),
            EXTENSION_PATH_PLACEHOLDER.to_owned(),
            "--config".to_owned(),
            CONFIG_PATH_PLACEHOLDER.to_owned(),
            "--model".to_owned(),
            format!("nan/{}", context.model.resolved_id),
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
                removed_environment: BTreeSet::from([
                    "PI_CONFIG_FILES".to_owned(),
                    "PI_SMOL_MODEL".to_owned(),
                    "PI_SLOW_MODEL".to_owned(),
                    "PI_PLAN_MODEL".to_owned(),
                ]),
                temporary_artifacts: vec![
                    TemporaryArtifact {
                        id: EXTENSION_ARTIFACT_ID.to_owned(),
                        kind: TemporaryArtifactKind::File,
                        path_hint: "nan-omp-provider.mjs".to_owned(),
                        mode: TemporaryArtifactMode::OwnerFile,
                        content_template: Some(provider_extension(context.web_search_policy)),
                        lifecycle: ArtifactLifecycle::Launch,
                    },
                    TemporaryArtifact {
                        id: CONFIG_ARTIFACT_ID.to_owned(),
                        kind: TemporaryArtifactKind::File,
                        path_hint: "nan-omp-config.yml".to_owned(),
                        mode: TemporaryArtifactMode::OwnerFile,
                        content_template: Some(launch_config(&context.model.resolved_id)),
                        lifecycle: ArtifactLifecycle::Launch,
                    },
                ],
                configuration_overlays: Vec::new(),
            },
        )
    }
}

fn launch_config(model_id: &str) -> String {
    let model = serde_json::Value::String(format!("nan/{model_id}"));
    let model = model.to_string();
    let roles = [
        "default", "smol", "slow", "vision", "plan", "designer", "commit", "tiny", "task",
        "advisor",
    ]
    .into_iter()
    .map(|role| format!("  {role}: {model}"))
    .collect::<Vec<_>>()
    .join("\n");
    format!("enabledModels:\n  - \"nan/*\"\nmodelRoles:\n{roles}\nretry:\n  modelFallback: false\n")
}

fn provider_extension(search_policy: WebSearchPolicy) -> String {
    let search = search_registration(
        &serde_json::Value::String(format!("{BRIDGE_BASE_URL_PLACEHOLDER}/v1/search")).to_string(),
        "process.env.NAN_API_KEY",
        if search_policy == WebSearchPolicy::Force {
            OmpSearchMode::Force
        } else {
            OmpSearchMode::Auto
        },
    );
    format!(
        r#"import {{ Type }} from "@oh-my-pi/pi-ai";
import {{ settings }} from "@oh-my-pi/pi-coding-agent";
import {{ getSearchProvider, setExcludedSearchProviders }} from "@oh-my-pi/pi-coding-agent/web/search";

const baseUrl = "{PROVIDER_BASE_URL_PLACEHOLDER}".replace(/\/+$/, "");
const profiles = {PI_MODEL_CATALOG_PLACEHOLDER};

export default function registerNan(pi) {{
  const apiKey = process.env.NAN_API_KEY;
  if (!apiKey) throw new Error("NAN_API_KEY is required for the NaN provider");

  const models = Object.entries(profiles).map(([id, profile]) => {{
    const effortPolicy = profile.reasoningPolicy.kind === "effort";
    const reasoning = effortPolicy || profile.reasoningPolicy.kind === "toggle" || profile.reasoningPolicy.kind === "always-on";
    const effortMap = effortPolicy
      ? Object.fromEntries(profile.reasoningPolicy.supported.map((level) => [level, level]))
      : undefined;
    return {{
      id,
      name: profile.name,
      reasoning,
      ...(effortPolicy ? {{ thinking: {{
        mode: "effort",
        efforts: profile.reasoningPolicy.supported,
        defaultLevel: profile.reasoningPolicy.default,
        effortMap
      }} }} : {{}}),
      input: profile.input,
      cost: {{ input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }},
      contextWindow: profile.contextWindow,
      maxTokens: profile.maxTokens,
      compat: {{
        supportsDeveloperRole: false,
        supportsReasoningEffort: effortPolicy,
        maxTokensField: "max_tokens",
        ...(effortMap ? {{ reasoningEffortMap: effortMap }} : {{}})
      }}
    }};
  }});

  pi.registerProvider("nan", {{
    baseUrl,
    apiKey,
    authHeader: true,
    api: "openai-completions",
    models
  }});
{NAN_SEARCH_BLOCK_BEGIN}
{search}
{NAN_SEARCH_BLOCK_END}
}}
"#
    )
}

#[must_use]
pub fn render_omp_search_extension(base_url: &str, mode: OmpSearchMode) -> String {
    let endpoint =
        serde_json::Value::String(format!("{}/search", base_url.trim_end_matches('/'))).to_string();
    let registration = search_registration(
        &endpoint,
        "await ctx.modelRegistry?.authStorage?.getApiKey(\"nan\")",
        mode,
    );
    format!(
        r#"import {{ Type }} from "@oh-my-pi/pi-ai";
import {{ settings }} from "@oh-my-pi/pi-coding-agent";
import {{ getSearchProvider, setExcludedSearchProviders }} from "@oh-my-pi/pi-coding-agent/web/search";

export default function registerNanSearch(pi) {{
{registration}
}}
"#
    )
}

fn search_registration(
    endpoint_expression: &str,
    credential_expression: &str,
    mode: OmpSearchMode,
) -> String {
    let force = mode == OmpSearchMode::Force;
    format!(
        r#"  const forceNanSearch = {force};
  const anonymousProviders = ["startpage", "duckduckgo", "ecosia", "google", "mojeek", "public"];
  const hybridProviders = ["perplexity", "exa", "firecrawl"];
  let configuredExclusions = [];
  try {{ configuredExclusions = settings.get("providers.webSearchExclude") ?? []; }} catch {{}}

  async function callNan(params, signal, ctx) {{
    const searchApiKey = {credential_expression};
    if (!searchApiKey) throw new Error("NaN provider credential is required for web search");
    const response = await fetch({endpoint_expression}, {{
      method: "POST",
      headers: {{ authorization: `Bearer ${{searchApiKey}}`, "content-type": "application/json" }},
      body: JSON.stringify({{ query: params.query, maxResults: params.limit ?? 10 }}),
      signal
    }});
    if (!response.ok) throw new Error(`NH-SEARCH-${{response.status}}`);
    const result = await response.json();
    return {{ content: [{{ type: "text", text: result.summary }}], details: {{ results: result.results }} }};
  }}

  pi.registerTool({{
    name: "web_search",
    label: "Web Search",
    description: "Search the web for up-to-date information",
    approval: "read",
    strict: true,
    parameters: Type.Object({{
      query: Type.String(),
      recency: Type.Optional(Type.Union([Type.Literal("day"), Type.Literal("week"), Type.Literal("month"), Type.Literal("year")])),
      limit: Type.Optional(Type.Number({{ minimum: 1, maximum: 20 }})),
      max_tokens: Type.Optional(Type.Number()),
      temperature: Type.Optional(Type.Number()),
      num_search_results: Type.Optional(Type.Number())
    }}),
    async execute(_toolCallId, params, signal, onUpdate, ctx) {{
      if (forceNanSearch) return callNan(params, signal, ctx);

      const exclusions = new Set([...configuredExclusions, ...anonymousProviders]);
      const authStorage = ctx.modelRegistry?.authStorage;
      for (const id of hybridProviders) {{
        let authenticated = false;
        try {{ authenticated = !!authStorage && await (await getSearchProvider(id)).isAvailable(authStorage); }} catch {{}}
        if (!authenticated) exclusions.add(id);
      }}
      setExcludedSearchProviders([...exclusions]);

      let nativeFailure;
      try {{
        if (!ctx.invokeTool) throw new Error("native OMP web_search is unavailable");
        const result = await ctx.invokeTool(params, {{ signal, onUpdate }});
        if (!result.isError) return result;
        nativeFailure = new Error("native OMP web_search failed");
      }} catch (error) {{
        nativeFailure = error;
      }}
      try {{
        return await callNan(params, signal, ctx);
      }} catch (error) {{
        const nativeCode = nativeFailure instanceof Error ? nativeFailure.name : "Error";
        const nanCode = error instanceof Error ? error.message : "NH-SEARCH-FAILED";
        throw new Error(`OMP authenticated search failed (${{nativeCode}}); NaN search failed (${{nanCode}})`);
      }}
    }}
  }});
"#
    )
}

#[cfg(test)]
mod tests {
    use super::{OmpSearchMode, launch_config, render_omp_search_extension};

    #[test]
    fn launch_config_routes_every_role_to_nan() {
        let config = launch_config("qwen3.6");
        for role in [
            "default", "smol", "slow", "vision", "plan", "designer", "commit", "tiny", "task",
            "advisor",
        ] {
            assert!(config.contains(&format!("  {role}: \"nan/qwen3.6\"")));
        }
        assert!(config.contains("modelFallback: false"));
    }

    #[test]
    fn native_search_modes_preserve_the_policy_contract() {
        let automatic = render_omp_search_extension("https://api.nan.test/v1", OmpSearchMode::Auto);
        assert!(automatic.contains("const forceNanSearch = false"));
        assert!(automatic.contains("ctx.invokeTool"));
        assert!(automatic.contains("anonymousProviders"));
        assert!(automatic.contains("hybridProviders"));
        assert!(automatic.contains("https://api.nan.test/v1/search"));

        let forced = render_omp_search_extension("https://api.nan.test/v1", OmpSearchMode::Force);
        assert!(forced.contains("const forceNanSearch = true"));
    }
}
