use crate::direct::{
    DirectLaunch, PROVIDER_URL_ENVIRONMENT, build_direct_plan, describe_model,
    provider_environment, validate_routing_arguments,
};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, TemporaryArtifact, TemporaryArtifactKind, TemporaryArtifactMode,
};
use nan_harness_core::{HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

const CREDENTIAL_TARGET: &str = "NAN_API_KEY";
const EXTENSION_ARTIFACT_ID: &str = "pi-provider-extension";
const EXTENSION_PATH_PLACEHOLDER: &str = "{artifact:pi-provider-extension}";

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

#[derive(Debug, Default)]
pub struct PersistentPiAdapter;

impl HarnessAdapter for PersistentPiAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Pi
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        persistent_pi_family_plan(context)
    }
}

#[derive(Debug, Default)]
pub struct PersistentPrimeAgentAdapter;

impl HarnessAdapter for PersistentPrimeAgentAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::PrimeAgent
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        persistent_pi_family_plan(context)
    }
}

fn pi_family_plan(context: &PlanContext) -> Result<LaunchPlan, PlanError> {
    validate_routing_arguments(
        &context.user_arguments,
        &["--model", "--provider", "--api-key", "--models"],
    )?;
    let extension = provider_extension(&context.model.resolved_id)?;
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
                content_template: Some(extension),
                lifecycle: ArtifactLifecycle::Launch,
            }],
            configuration_overlays: Vec::new(),
        },
    )
}

fn persistent_pi_family_plan(context: &PlanContext) -> Result<LaunchPlan, PlanError> {
    validate_routing_arguments(
        &context.user_arguments,
        &["--model", "--provider", "--api-key", "--models"],
    )?;
    let mut arguments = vec![
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
            temporary_artifacts: Vec::new(),
            configuration_overlays: Vec::new(),
        },
    )
}

/// Builds the global Pi extension used by the persistent integration.
///
/// # Errors
///
/// Returns [`PlanError`] when the configured provider URL cannot be represented safely in the
/// JavaScript module.
pub fn persistent_provider_extension(provider_base_url: &str) -> Result<String, PlanError> {
    let provider_base_url =
        serde_json::to_string(provider_base_url).map_err(|error| PlanError::InvalidField {
            field: "providerBaseUrl",
            message: format!("could not serialize the persistent Pi provider URL: {error}"),
        })?;
    let profiles = [
        "qwen3.6",
        "deepseek-v4-flash",
        "mimo-v2.5",
        "gemma4",
        "glm5.2",
    ]
    .into_iter()
    .map(|model_id| {
        let model = describe_model(model_id);
        let input = if model.image_input {
            vec!["text", "image"]
        } else {
            vec!["text"]
        };
        (
            model_id,
            json!({
                "name": model.display_name,
                "contextWindow": model.context_window,
                "maxTokens": model.max_tokens,
                "input": input,
            }),
        )
    })
    .collect::<BTreeMap<_, _>>();
    let profiles = serde_json::to_string(&profiles).map_err(|error| PlanError::InvalidField {
        field: "temporaryArtifacts.contentTemplate",
        message: format!("could not serialize persistent Pi model profiles: {error}"),
    })?;
    Ok(format!(
        r#"const defaultBaseUrl = {provider_base_url};

const profiles = {profiles};

export default async function registerNan(pi) {{
  const baseUrl = (process.env.NAN_HARNESS_PROVIDER_BASE_URL || process.env.NAN_BASE_URL || defaultBaseUrl).replace(/\/+$/, "");
  const apiKey = process.env.NAN_API_KEY;
  if (!apiKey) throw new Error("NAN_API_KEY is required for the persistent NaN provider");

  const response = await fetch(`${{baseUrl}}/models`, {{
    headers: {{ Accept: "application/json", Authorization: `Bearer ${{apiKey}}` }},
    signal: AbortSignal.timeout(30000)
  }});
  if (!response.ok) throw new Error(`NaN model discovery failed with HTTP ${{response.status}}`);

  const payload = await response.json();
  const ids = Array.isArray(payload.data)
    ? [...new Set(payload.data
      .map((model) => model?.id)
      .filter((id) => typeof id === "string" && id.length > 0 && id.length <= 256 && !/[\u0000-\u001F\u007F]/.test(id)))]
      .sort()
    : [];
  if (ids.length === 0) throw new Error("NaN returned no models for this credential");

  const models = ids.map((id) => {{
    const profile = profiles[id] || {{ name: `NaN · ${{id}}`, contextWindow: 262144, maxTokens: 32768, input: ["text"] }};
    return {{
      id,
      name: profile.name,
      reasoning: false,
      input: profile.input,
      cost: {{ input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }},
      contextWindow: profile.contextWindow,
      maxTokens: profile.maxTokens,
      compat: {{ supportsDeveloperRole: false, supportsReasoningEffort: false, maxTokensField: "max_tokens" }}
    }};
  }});

  pi.registerProvider("nan", {{
    baseUrl,
    apiKey,
    authHeader: true,
    api: "openai-completions",
    models
  }});
}}
"#
    ))
}

fn provider_extension(model_id: &str) -> Result<String, PlanError> {
    let model = describe_model(model_id);
    let input = if model.image_input {
        vec!["text", "image"]
    } else {
        vec!["text"]
    };
    let model = serde_json::to_string(&json!({
        "id": model_id,
        "name": model.display_name,
        "reasoning": false,
        "input": input,
        "cost": {
            "input": 0,
            "output": 0,
            "cacheRead": 0,
            "cacheWrite": 0
        },
        "contextWindow": model.context_window,
        "maxTokens": model.max_tokens,
        "compat": {
            "supportsDeveloperRole": false,
            "supportsReasoningEffort": false,
            "maxTokensField": "max_tokens"
        }
    }))
    .map_err(|error| PlanError::InvalidField {
        field: "temporaryArtifacts.contentTemplate",
        message: format!("could not serialize Pi model configuration: {error}"),
    })?;
    Ok(format!(
        "export default function registerNan(pi) {{\n  pi.registerProvider(\"nan\", {{\n    baseUrl: process.env.{PROVIDER_URL_ENVIRONMENT},\n    apiKey: process.env.NAN_API_KEY,\n    authHeader: true,\n    api: \"openai-completions\",\n    models: [{model}]\n  }});\n}}\n"
    ))
}
