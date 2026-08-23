use crate::direct::{
    DirectLaunch, build_direct_plan, provider_environment, validate_routing_arguments,
};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, PI_MODEL_CATALOG_PLACEHOLDER, PROVIDER_BASE_URL_PLACEHOLDER,
    TemporaryArtifact, TemporaryArtifactKind, TemporaryArtifactMode,
};
use nan_harness_core::{HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError};
use std::collections::BTreeSet;

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
                content_template: Some(provider_extension()),
                lifecycle: ArtifactLifecycle::Launch,
            }],
            configuration_overlays: Vec::new(),
        },
    )
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
