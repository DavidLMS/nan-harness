use crate::temporary::{TemporaryError, TemporaryWorkspace};
use nan_harness_core::launch_plan::{
    ARTIFACT_PLACEHOLDER_PREFIX, BRIDGE_BASE_URL_PLACEHOLDER, CLAUDE_AVAILABLE_MODELS_PLACEHOLDER,
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
        bridge: Option<BridgePreparation>,
    ) -> Result<Self, PreparedError> {
        let bridge_base_url = bridge.as_ref().map(|values| values.base_url.as_str());
        let workspace = TemporaryWorkspace::materialize_with(
            &plan.temporary_artifacts,
            |artifact, template| {
                render_template(template, bridge.as_ref()).map_err(|reason| {
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
            .map(|argument| resolve_argument(argument, &workspace))
            .collect::<Result<Vec<_>, _>>()?;
        let public_environment = plan
            .environment
            .public
            .iter()
            .map(|(name, value)| {
                render_public_value(value, bridge_base_url).map(|value| (name.clone(), value))
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

fn render_template(template: &str, bridge: Option<&BridgePreparation>) -> Result<String, String> {
    let Some(bridge) = bridge else {
        if template.contains("{runtime:") || template.contains("{secret:") {
            return Err("runtime placeholders require a bridge preparation".to_owned());
        }
        return Ok(template.to_owned());
    };
    let rendered = template.replace(BRIDGE_BASE_URL_PLACEHOLDER, &bridge.base_url);
    let available_models = serde_json::to_string(&bridge.claude_available_models)
        .map_err(|error| format!("could not serialize Claude model IDs: {error}"))?;
    let quoted_placeholder = format!("\"{CLAUDE_AVAILABLE_MODELS_PLACEHOLDER}\"");
    let rendered = rendered.replace(&quoted_placeholder, &available_models);
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
    bridge_base_url: Option<&str>,
) -> Result<String, PreparedError> {
    if value == BRIDGE_BASE_URL_PLACEHOLDER {
        bridge_base_url.map(str::to_owned).ok_or_else(|| {
            PreparedError::UnresolvedPlaceholder(BRIDGE_BASE_URL_PLACEHOLDER.to_owned())
        })
    } else if value.contains("{runtime:") || value.contains("{secret:") {
        Err(PreparedError::UnresolvedPlaceholder(value.to_owned()))
    } else {
        Ok(value.to_owned())
    }
}

fn resolve_argument(
    argument: &str,
    workspace: &TemporaryWorkspace,
) -> Result<String, PreparedError> {
    if let Some(artifact_id) = artifact_reference(argument) {
        workspace
            .path(artifact_id)
            .map(path_to_string)
            .ok_or_else(|| PreparedError::UnknownArtifact(artifact_id.to_owned()))
    } else if argument.starts_with(ARTIFACT_PLACEHOLDER_PREFIX) {
        Err(PreparedError::UnresolvedPlaceholder(argument.to_owned()))
    } else {
        Ok(argument.to_owned())
    }
}

fn artifact_reference(value: &str) -> Option<&str> {
    value
        .strip_prefix(ARTIFACT_PLACEHOLDER_PREFIX)
        .and_then(|value| value.strip_suffix('}'))
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
