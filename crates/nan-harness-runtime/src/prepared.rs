use crate::temporary::{TemporaryError, TemporaryWorkspace};
use nan_harness_core::launch_plan::{
    ARTIFACT_PLACEHOLDER_PREFIX, BRIDGE_BASE_URL_PLACEHOLDER, CLAUDE_AVAILABLE_MODELS_PLACEHOLDER,
    CODEX_MODEL_CATALOG_PLACEHOLDER, PROVIDER_BASE_URL_PLACEHOLDER,
};
use nan_harness_core::{LaunchPlan, SecretError, SecretRef, SecretStore, SecretValue};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

pub(crate) struct BridgePreparation {
    pub(crate) base_url: String,
    pub(crate) session_token_ref: SecretRef,
    pub(crate) session_token: Arc<SecretValue>,
    pub(crate) claude_available_models: Vec<String>,
    pub(crate) codex_model_catalog: Option<String>,
}

pub(crate) struct PreparedLaunch {
    arguments: Vec<String>,
    public_environment: BTreeMap<String, String>,
    runtime_secrets: BTreeMap<SecretRef, Arc<SecretValue>>,
    workspace: TemporaryWorkspace,
}

impl PreparedLaunch {
    pub(crate) fn prepare(
        plan: &LaunchPlan,
        provider_base_url: &str,
        bridge: Option<BridgePreparation>,
    ) -> Result<Self, PreparedError> {
        let bridge_base_url = bridge.as_ref().map(|values| values.base_url.as_str());
        let workspace = TemporaryWorkspace::materialize_with(
            &plan.temporary_artifacts,
            |artifact, template| {
                render_template(template, provider_base_url, bridge.as_ref()).map_err(|reason| {
                    TemporaryError::InvalidArtifact {
                        artifact_id: artifact.id.clone(),
                        reason,
                    }
                })
            },
        )?;
        let arguments = plan
            .process
            .arguments
            .iter()
            .map(|argument| {
                resolve_argument(argument, &workspace).and_then(|argument| {
                    render_runtime_value(&argument, provider_base_url, bridge_base_url)
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let public_environment = plan
            .environment
            .public
            .iter()
            .map(|(name, value)| {
                render_public_value(value, provider_base_url, bridge_base_url)
                    .map(|value| (name.clone(), value))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let runtime_secrets = bridge
            .map(|values| BTreeMap::from([(values.session_token_ref, values.session_token)]))
            .unwrap_or_default();

        Ok(Self {
            arguments,
            public_environment,
            runtime_secrets,
            workspace,
        })
    }

    pub(crate) fn arguments(&self) -> &[String] {
        &self.arguments
    }

    pub(crate) fn public_environment(&self) -> &BTreeMap<String, String> {
        &self.public_environment
    }

    pub(crate) fn with_secret<T>(
        &self,
        provider_secrets: &SecretStore,
        reference: &SecretRef,
        operation: impl FnOnce(&str) -> T,
    ) -> Result<T, SecretError> {
        if let Some(value) = self.runtime_secrets.get(reference) {
            Ok(value.with_secret(operation))
        } else {
            provider_secrets.with_secret(reference, operation)
        }
    }

    pub(crate) fn temporary_root(&self, has_artifacts: bool) -> Option<std::path::PathBuf> {
        has_artifacts.then(|| self.workspace.root().to_path_buf())
    }
}

fn render_template(
    template: &str,
    provider_base_url: &str,
    bridge: Option<&BridgePreparation>,
) -> Result<String, String> {
    let rendered = template.replace(PROVIDER_BASE_URL_PLACEHOLDER, provider_base_url);
    let Some(bridge) = bridge else {
        if rendered.contains("{runtime:") || rendered.contains("{secret:") {
            return Err("runtime placeholders require a bridge preparation".to_owned());
        }
        return Ok(rendered);
    };
    let rendered = rendered.replace(BRIDGE_BASE_URL_PLACEHOLDER, &bridge.base_url);
    let available_models = serde_json::to_string(&bridge.claude_available_models)
        .map_err(|error| format!("could not serialize Claude model IDs: {error}"))?;
    let quoted_placeholder = format!("\"{CLAUDE_AVAILABLE_MODELS_PLACEHOLDER}\"");
    let rendered = rendered.replace(&quoted_placeholder, &available_models);
    let rendered = match bridge.codex_model_catalog.as_deref() {
        Some(catalog) => rendered.replace(CODEX_MODEL_CATALOG_PLACEHOLDER, catalog),
        None => rendered,
    };
    let placeholder = format!("{{secret:{}}}", bridge.session_token_ref.as_str());
    let rendered = bridge
        .session_token
        .with_secret(|token| rendered.replace(&placeholder, token));
    if rendered.contains("{runtime:") || rendered.contains("{secret:") {
        Err("content contains an unresolved runtime placeholder".to_owned())
    } else {
        Ok(rendered)
    }
}

fn render_public_value(
    value: &str,
    provider_base_url: &str,
    bridge_base_url: Option<&str>,
) -> Result<String, PreparedError> {
    render_runtime_value(value, provider_base_url, bridge_base_url)
}

fn render_runtime_value(
    value: &str,
    provider_base_url: &str,
    bridge_base_url: Option<&str>,
) -> Result<String, PreparedError> {
    let mut rendered = value.replace(PROVIDER_BASE_URL_PLACEHOLDER, provider_base_url);
    if rendered.contains(BRIDGE_BASE_URL_PLACEHOLDER) {
        let bridge_base_url = bridge_base_url.ok_or_else(|| {
            PreparedError::UnresolvedPlaceholder(BRIDGE_BASE_URL_PLACEHOLDER.to_owned())
        })?;
        rendered = rendered.replace(BRIDGE_BASE_URL_PLACEHOLDER, bridge_base_url);
    }
    if rendered.contains("{runtime:") || rendered.contains("{secret:") {
        Err(PreparedError::UnresolvedPlaceholder(rendered))
    } else {
        Ok(rendered)
    }
}

fn resolve_argument(
    argument: &str,
    workspace: &TemporaryWorkspace,
) -> Result<String, PreparedError> {
    let mut rendered = argument.to_owned();
    while let Some(start) = rendered.find(ARTIFACT_PLACEHOLDER_PREFIX) {
        let content_start = start + ARTIFACT_PLACEHOLDER_PREFIX.len();
        let Some(relative_end) = rendered[content_start..].find('}') else {
            return Err(PreparedError::UnresolvedPlaceholder(rendered));
        };
        let end = content_start + relative_end;
        let artifact_id = &rendered[content_start..end];
        if artifact_id.is_empty() || artifact_id.contains(['{', '}']) {
            return Err(PreparedError::UnresolvedPlaceholder(rendered));
        }
        let path = workspace
            .path(artifact_id)
            .map(path_to_string)
            .ok_or_else(|| PreparedError::UnknownArtifact(artifact_id.to_owned()))?;
        rendered.replace_range(start..=end, &path);
    }
    Ok(rendered)
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[derive(Debug, Error)]
pub enum PreparedError {
    #[error(transparent)]
    Temporary(#[from] TemporaryError),
    #[error("launch references unknown temporary artifact '{0}'")]
    UnknownArtifact(String),
    #[error("launch contains unresolved placeholder '{0}'")]
    UnresolvedPlaceholder(String),
}
