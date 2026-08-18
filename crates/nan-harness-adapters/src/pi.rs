use crate::direct::{
    DirectLaunch, build_direct_plan, describe_model, provider_environment,
    validate_routing_arguments,
};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, PI_MODEL_CATALOG_PLACEHOLDER, PROVIDER_BASE_URL_PLACEHOLDER,
    TemporaryArtifact, TemporaryArtifactKind, TemporaryArtifactMode,
};
use nan_harness_core::{
    GENERIC_CODING_MODEL_DESCRIPTION, HarnessAdapter, HarnessKind, KNOWN_CODING_MODELS,
    KNOWN_NON_CODING_MODELS, LaunchPlan, PlanContext, PlanError,
};
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
    let extension = provider_extension();
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
    let profiles = KNOWN_CODING_MODELS
        .into_iter()
        .map(|metadata| {
            let model = describe_model(metadata.id);
            let input = if model.image_input {
                vec!["text", "image"]
            } else {
                vec!["text"]
            };
            (
                metadata.id,
                json!({
                    "name": model.display_name,
                    "contextWindow": model.context_window,
                    "maxTokens": model.max_tokens,
                    "input": input,
                    "reasoningPolicy": model.reasoning,
                }),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let profiles = serde_json::to_string(&profiles).map_err(|error| PlanError::InvalidField {
        field: "temporaryArtifacts.contentTemplate",
        message: format!("could not serialize persistent Pi model profiles: {error}"),
    })?;
    let blocked_models = serde_json::to_string(&KNOWN_NON_CODING_MODELS).map_err(|error| {
        PlanError::InvalidField {
            field: "temporaryArtifacts.contentTemplate",
            message: format!("could not serialize incompatible NaN model IDs: {error}"),
        }
    })?;
    let generic_description =
        serde_json::to_string(GENERIC_CODING_MODEL_DESCRIPTION).map_err(|error| {
            PlanError::InvalidField {
                field: "temporaryArtifacts.contentTemplate",
                message: format!("could not serialize the generic model description: {error}"),
            }
        })?;
    Ok(format!(
        r#"const defaultBaseUrl = {provider_base_url};

const knownProfiles = {profiles};
const blockedModels = new Set({blocked_models});
const genericDescription = {generic_description};

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
      .filter((id) => typeof id === "string" && id.length > 0 && id.length <= 256 && !/[\u0000-\u001F\u007F]/.test(id) && !blockedModels.has(id)))]
      .sort()
    : [];
  if (ids.length === 0) throw new Error("NaN returned no compatible coding models for this credential");

  const models = ids.map((id) => {{
    const profile = knownProfiles[id] || {{
      name: `NaN · ${{id}}`,
      description: genericDescription,
      contextWindow: 262144,
      maxTokens: 32768,
      input: ["text"],
      reasoningPolicy: {{ kind: "unknown" }}
    }};
    return {{
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

fn provider_extension() -> String {
    format!(
        r#"const baseUrl = "{PROVIDER_BASE_URL_PLACEHOLDER}".replace(/\/+$/, "");
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
}}
"#
    )
}
