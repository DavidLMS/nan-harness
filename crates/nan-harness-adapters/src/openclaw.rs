use crate::direct::{
    DirectLaunch, build_direct_plan, provider_environment, validate_routing_arguments,
};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, BRIDGE_BASE_URL_PLACEHOLDER, ConfigurationOverlay, NAN_SEARCH_BLOCK_BEGIN,
    NAN_SEARCH_BLOCK_END, OPENCLAW_MODEL_ALIASES_PLACEHOLDER, OPENCLAW_MODEL_CATALOG_PLACEHOLDER,
    OverlayFile, OverlayFilePolicy, PROVIDER_BASE_URL_PLACEHOLDER, TemporaryArtifactMode,
    USER_HOME_PLACEHOLDER,
};
use nan_harness_core::{HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError};
use serde_json::json;
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "NAN_API_KEY";
const CONFIG_OVERLAY_ID: &str = "openclaw-config";
const CONFIG_PATH: &str = "{artifact:openclaw-config}/nan-harness.json";
const SEARCH_PLUGIN_PATH: &str = "{artifact:openclaw-config}/plugins/nan-harness-search";

fn openclaw_config(model_id: &str) -> Result<String, PlanError> {
    let model_reference = format!("nan/{model_id}");
    let base = serde_json::to_string(&json!({
        "$include": "./openclaw.json",
        "agents": {
            "defaults": {
                "model": {"primary": model_reference},
                "models": OPENCLAW_MODEL_ALIASES_PLACEHOLDER
            }
        },
        "models": {
            "mode": "merge",
            "providers": {
                "nan": {
                    "api": "openai-completions",
                    "apiKey": {
                        "id": CREDENTIAL_TARGET,
                        "provider": "default",
                        "source": "env"
                    },
                    "baseUrl": PROVIDER_BASE_URL_PLACEHOLDER,
                    "models": OPENCLAW_MODEL_CATALOG_PLACEHOLDER
                }
            }
        }
    }))
    .map_err(|error| openclaw_serialization_error(&error))?;
    let search = serde_json::to_string(&json!({
        "plugins": {
            "load": {"paths": [SEARCH_PLUGIN_PATH]},
            "entries": {"nan-harness-search": {"enabled": true}}
        },
        "tools": {"web": {"search": {"enabled": true, "provider": "nan-harness"}}}
    }))
    .map_err(|error| openclaw_serialization_error(&error))?;

    Ok(format!(
        "{}{NAN_SEARCH_BLOCK_BEGIN},{}{NAN_SEARCH_BLOCK_END}}}",
        &base[..base.len() - 1],
        &search[1..search.len() - 1]
    ))
}

fn openclaw_serialization_error(error: &serde_json::Error) -> PlanError {
    PlanError::InvalidField {
        field: "configurationOverlays.files.contentTemplate",
        message: format!("could not serialize OpenClaw configuration: {error}"),
    }
}

fn search_plugin_files() -> Vec<OverlayFile> {
    vec![
        OverlayFile {
            path: "plugins/nan-harness-search/package.json".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: r#"{"name":"nan-harness-search","version":"1.0.0","type":"module","peerDependencies":{"openclaw":">=2026.3.24"},"openclaw":{"extensions":["./index.js"]}}"#
                .to_owned(),
            policy: OverlayFilePolicy::Replace,
        },
        OverlayFile {
            path: "plugins/nan-harness-search/openclaw.plugin.json".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: r#"{"id":"nan-harness-search","activation":{"onStartup":false},"contracts":{"webSearchProviders":["nan-harness"]},"configSchema":{"type":"object","additionalProperties":false}}"#
                .to_owned(),
            policy: OverlayFilePolicy::Replace,
        },
        OverlayFile {
            path: "plugins/nan-harness-search/index.js".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: format!(
                r#"import {{ definePluginEntry }} from "openclaw/plugin-sdk/plugin-entry";

const parameters = {{
  type: "object",
  properties: {{
    query: {{ type: "string" }},
    count: {{ type: "integer", minimum: 1, maximum: 20 }}
  }},
  required: ["query"],
  additionalProperties: false
}};

const provider = {{
  id: "nan-harness",
  label: "nan-search",
  hint: "nan-search",
  requiresCredential: true,
  envVars: ["NAN_API_KEY"],
  placeholder: "nan-session",
  signupUrl: "https://nan.im",
  credentialPath: "",
  getCredentialValue: () => process.env.NAN_API_KEY,
  setCredentialValue: () => {{}},
  createTool: () => ({{
    description: "nan-search",
    parameters,
    execute: async (args, context) => {{
      const query = typeof args.query === "string" ? args.query.trim() : "";
      if (!query) throw new Error("NH-SEARCH-QUERY");
      const count = Number.isInteger(args.count) ? Math.min(Math.max(args.count, 1), 20) : 5;
      const response = await fetch("{BRIDGE_BASE_URL_PLACEHOLDER}/v1/search", {{
        method: "POST",
        headers: {{
          authorization: `Bearer ${{process.env.NAN_API_KEY ?? ""}}`,
          "content-type": "application/json"
        }},
        body: JSON.stringify({{ query, maxResults: count }}),
        signal: context?.signal
      }});
      if (!response.ok) throw new Error(`NH-SEARCH-HTTP-${{response.status}}`);
      const payload = await response.json();
      const results = Array.isArray(payload.results) ? payload.results : [];
      return {{
        query,
        provider: "nan-harness",
        count: results.length,
        externalContent: {{ untrusted: true, source: "web_search", provider: "nan-harness" }},
        results
      }};
    }}
  }})
}};

export default definePluginEntry({{
  id: "nan-harness-search",
  name: "nan-search",
  description: "nan-search",
  register(api) {{
    api.registerWebSearchProvider(provider);
  }}
}});
"#
            ),
            policy: OverlayFilePolicy::Replace,
        },
    ]
}

#[derive(Debug, Default)]
pub struct OpenClawAdapter;

impl HarnessAdapter for OpenClawAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::OpenClaw
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_routing_arguments(
            &context.user_arguments,
            &["--model", "--profile", "--dev", "--container"],
        )?;
        let config = openclaw_config(&context.model.resolved_id)?;
        let mut public_environment = provider_environment();
        public_environment.insert("OPENCLAW_CONFIG_PATH".to_owned(), CONFIG_PATH.to_owned());
        public_environment.insert(
            "OPENCLAW_INCLUDE_ROOTS".to_owned(),
            USER_HOME_PLACEHOLDER.to_owned(),
        );
        let arguments = if context.user_arguments.is_empty() {
            vec!["chat".to_owned()]
        } else {
            context.user_arguments.clone()
        };

        build_direct_plan(
            context,
            DirectLaunch {
                arguments,
                credential_target: CREDENTIAL_TARGET,
                public_environment,
                removed_environment: BTreeSet::new(),
                temporary_artifacts: Vec::new(),
                configuration_overlays: vec![ConfigurationOverlay {
                    id: CONFIG_OVERLAY_ID.to_owned(),
                    path_hint: "openclaw".to_owned(),
                    source_path: format!("{USER_HOME_PLACEHOLDER}/.openclaw"),
                    files: vec![
                        OverlayFile {
                            path: "openclaw.json".to_owned(),
                            mode: TemporaryArtifactMode::OwnerFile,
                            content_template: "{}".to_owned(),
                            policy: OverlayFilePolicy::Preserve,
                        },
                        OverlayFile {
                            path: "nan-harness.json".to_owned(),
                            mode: TemporaryArtifactMode::OwnerFile,
                            content_template: config,
                            policy: OverlayFilePolicy::Replace,
                        },
                    ]
                    .into_iter()
                    .chain(search_plugin_files())
                    .collect(),
                    lifecycle: ArtifactLifecycle::Launch,
                }],
            },
        )
    }
}
