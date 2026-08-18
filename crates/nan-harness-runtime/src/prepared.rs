use crate::temporary::{TemporaryError, TemporaryWorkspace};
use nan_harness_core::launch_plan::{
    AIDER_MODEL_METADATA_PLACEHOLDER, AIDER_MODEL_SETTINGS_PLACEHOLDER,
    ARTIFACT_PLACEHOLDER_PREFIX, BRIDGE_BASE_URL_PLACEHOLDER, CLAUDE_AVAILABLE_MODELS_PLACEHOLDER,
    CLINE_MODEL_CATALOG_PLACEHOLDER, CODEX_MODEL_CATALOG_PLACEHOLDER,
    DEEPSEEK_MODEL_CATALOG_PLACEHOLDER, GOOSE_MODEL_CATALOG_PLACEHOLDER,
    HERMES_MODEL_CATALOG_PLACEHOLDER, OPENCLAW_MODEL_ALIASES_PLACEHOLDER,
    OPENCLAW_MODEL_CATALOG_PLACEHOLDER, OPENCODE_MODEL_CATALOG_PLACEHOLDER,
    PI_MODEL_CATALOG_PLACEHOLDER, PROVIDER_BASE_URL_PLACEHOLDER,
    QWEN_CODE_MODEL_CATALOG_PLACEHOLDER, SELECTED_MODEL_CAPABILITIES_PLACEHOLDER,
    SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER, SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER,
    SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER, USER_HOME_PLACEHOLDER,
};
use nan_harness_core::{
    CodingModelProfile, LaunchPlan, SecretError, SecretRef, SecretStore, SecretValue,
};
use std::collections::BTreeMap;
use std::fmt::Write as _;
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
        model_catalog: Option<&[CodingModelProfile]>,
    ) -> Result<Self, PreparedError> {
        let bridge_base_url = bridge.as_ref().map(|values| values.base_url.as_str());
        let workspace = TemporaryWorkspace::materialize_with(
            &plan.temporary_artifacts,
            &plan.configuration_overlays,
            |resource_id, template| {
                render_template(
                    template,
                    provider_base_url,
                    &plan.model.resolved_id,
                    bridge.as_ref(),
                    model_catalog,
                )
                .map_err(|reason| TemporaryError::InvalidArtifact {
                    artifact_id: resource_id.to_owned(),
                    reason,
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
                resolve_argument(value, &workspace)
                    .and_then(|value| {
                        render_public_value(
                            &value,
                            provider_base_url,
                            bridge_base_url,
                            workspace.user_home(),
                            &plan.model.resolved_id,
                            model_catalog,
                        )
                    })
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
    selected_model_id: &str,
    bridge: Option<&BridgePreparation>,
    model_catalog: Option<&[CodingModelProfile]>,
) -> Result<String, String> {
    let rendered = template.replace(PROVIDER_BASE_URL_PLACEHOLDER, provider_base_url);
    let rendered = render_model_catalogs(
        &rendered,
        provider_base_url,
        selected_model_id,
        model_catalog,
    )?;
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
    user_home: &Path,
    selected_model_id: &str,
    model_catalog: Option<&[CodingModelProfile]>,
) -> Result<String, PreparedError> {
    let value = value.replace(USER_HOME_PLACEHOLDER, &user_home.to_string_lossy());
    let value = render_model_catalogs(&value, provider_base_url, selected_model_id, model_catalog)
        .map_err(PreparedError::ModelCatalog)?;
    render_runtime_value(&value, provider_base_url, bridge_base_url)
}

pub(crate) fn requires_model_catalog(plan: &LaunchPlan) -> bool {
    plan.temporary_artifacts
        .iter()
        .filter_map(|artifact| artifact.content_template.as_deref())
        .chain(plan.configuration_overlays.iter().flat_map(|overlay| {
            overlay
                .files
                .iter()
                .map(|file| file.content_template.as_str())
        }))
        .chain(plan.environment.public.values().map(String::as_str))
        .any(contains_model_catalog_placeholder)
}

fn contains_model_catalog_placeholder(value: &str) -> bool {
    [
        AIDER_MODEL_METADATA_PLACEHOLDER,
        AIDER_MODEL_SETTINGS_PLACEHOLDER,
        CLINE_MODEL_CATALOG_PLACEHOLDER,
        DEEPSEEK_MODEL_CATALOG_PLACEHOLDER,
        GOOSE_MODEL_CATALOG_PLACEHOLDER,
        HERMES_MODEL_CATALOG_PLACEHOLDER,
        OPENCODE_MODEL_CATALOG_PLACEHOLDER,
        OPENCLAW_MODEL_ALIASES_PLACEHOLDER,
        OPENCLAW_MODEL_CATALOG_PLACEHOLDER,
        PI_MODEL_CATALOG_PLACEHOLDER,
        QWEN_CODE_MODEL_CATALOG_PLACEHOLDER,
        SELECTED_MODEL_CAPABILITIES_PLACEHOLDER,
        SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER,
        SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER,
        SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER,
    ]
    .iter()
    .any(|placeholder| value.contains(placeholder))
}

fn render_model_catalogs(
    template: &str,
    provider_base_url: &str,
    selected_model_id: &str,
    model_catalog: Option<&[CodingModelProfile]>,
) -> Result<String, String> {
    if !contains_model_catalog_placeholder(template) {
        return Ok(template.to_owned());
    }
    let models = model_catalog
        .ok_or_else(|| "model catalog placeholders require live NaN model discovery".to_owned())?;
    let mut rendered = template.to_owned();
    render_selected_model(&mut rendered, selected_model_id, models)?;
    replace_json_placeholder(
        &mut rendered,
        AIDER_MODEL_METADATA_PLACEHOLDER,
        &aider_model_metadata(models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        AIDER_MODEL_SETTINGS_PLACEHOLDER,
        &aider_model_settings(models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        CLINE_MODEL_CATALOG_PLACEHOLDER,
        &cline_model_catalog(models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        GOOSE_MODEL_CATALOG_PLACEHOLDER,
        &goose_model_catalog(models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        HERMES_MODEL_CATALOG_PLACEHOLDER,
        &hermes_model_catalog(models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        PI_MODEL_CATALOG_PLACEHOLDER,
        &pi_model_catalog(models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        OPENCODE_MODEL_CATALOG_PLACEHOLDER,
        &opencode_model_catalog(models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        OPENCLAW_MODEL_ALIASES_PLACEHOLDER,
        &openclaw_model_aliases(models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        OPENCLAW_MODEL_CATALOG_PLACEHOLDER,
        &openclaw_model_catalog(models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        QWEN_CODE_MODEL_CATALOG_PLACEHOLDER,
        &qwen_code_model_catalog(models, provider_base_url),
    )?;
    rendered = rendered.replace(
        DEEPSEEK_MODEL_CATALOG_PLACEHOLDER,
        &deepseek_model_catalog(models)?,
    );
    Ok(rendered)
}

fn render_selected_model(
    target: &mut String,
    selected_model_id: &str,
    models: &[CodingModelProfile],
) -> Result<(), String> {
    let Some(model) = models.iter().find(|model| model.id == selected_model_id) else {
        return Err(format!(
            "selected model '{selected_model_id}' is not present in the discovered NaN catalog"
        ));
    };
    let capabilities = if model.image_input {
        "image_in,thinking"
    } else {
        "thinking"
    };
    *target = target
        .replace(SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER, &model.display_name)
        .replace(
            SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER,
            &model.context_window.to_string(),
        )
        .replace(
            SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER,
            &model.max_output_tokens.to_string(),
        )
        .replace(SELECTED_MODEL_CAPABILITIES_PLACEHOLDER, capabilities);
    Ok(())
}

fn aider_model_metadata(models: &[CodingModelProfile]) -> serde_json::Value {
    serde_json::Value::Object(
        models
            .iter()
            .map(|model| {
                (
                    format!("openai/{}", model.id),
                    serde_json::json!({
                        "litellm_provider": "openai",
                        "max_input_tokens": model.context_window,
                        "max_output_tokens": model.max_output_tokens,
                        "max_tokens": model.max_output_tokens,
                        "mode": "chat",
                        "supports_function_calling": true,
                        "supports_vision": model.image_input,
                    }),
                )
            })
            .collect(),
    )
}

fn aider_model_settings(models: &[CodingModelProfile]) -> serde_json::Value {
    serde_json::Value::Array(
        models
            .iter()
            .map(|model| {
                let name = format!("openai/{}", model.id);
                serde_json::json!({
                    "edit_format": "diff",
                    "editor_model_name": name,
                    "name": name,
                    "use_repo_map": true,
                    "weak_model_name": name,
                })
            })
            .collect(),
    )
}

fn cline_model_catalog(models: &[CodingModelProfile]) -> serde_json::Value {
    serde_json::Value::Object(
        models
            .iter()
            .map(|model| {
                (
                    model.id.clone(),
                    serde_json::json!({
                        "contextWindow": model.context_window,
                        "id": model.id,
                        "maxInputTokens": model.context_window,
                        "maxTokens": model.max_output_tokens,
                        "name": model.display_name,
                        "supportsAttachments": model.image_input,
                        "supportsVision": model.image_input,
                    }),
                )
            })
            .collect(),
    )
}

fn goose_model_catalog(models: &[CodingModelProfile]) -> serde_json::Value {
    serde_json::Value::Array(
        models
            .iter()
            .enumerate()
            .map(|(index, model)| {
                serde_json::json!({
                    "alias": model.display_name,
                    "context_limit": model.context_window,
                    "id": index + 1,
                    "name": model.id,
                    "provider": "openai",
                    "subtext": model.description,
                })
            })
            .collect(),
    )
}

fn hermes_model_catalog(models: &[CodingModelProfile]) -> serde_json::Value {
    serde_json::Value::Array(
        models
            .iter()
            .map(|model| serde_json::Value::String(model.id.clone()))
            .collect(),
    )
}

fn replace_json_placeholder(
    target: &mut String,
    placeholder: &str,
    value: &serde_json::Value,
) -> Result<(), String> {
    if !target.contains(placeholder) {
        return Ok(());
    }
    let encoded = serde_json::to_string(value)
        .map_err(|error| format!("could not serialize the NaN model catalog: {error}"))?;
    let quoted = serde_json::to_string(placeholder)
        .map_err(|error| format!("could not serialize a model catalog placeholder: {error}"))?;
    *target = target
        .replace(&quoted, &encoded)
        .replace(placeholder, &encoded);
    Ok(())
}

fn pi_model_catalog(models: &[CodingModelProfile]) -> serde_json::Value {
    serde_json::Value::Object(
        models
            .iter()
            .map(|model| {
                (
                    model.id.clone(),
                    serde_json::json!({
                        "contextWindow": model.context_window,
                        "description": model.description,
                        "input": model_input(model),
                        "maxTokens": model.max_output_tokens,
                        "name": model.display_name,
                    }),
                )
            })
            .collect(),
    )
}

fn opencode_model_catalog(models: &[CodingModelProfile]) -> serde_json::Value {
    serde_json::Value::Object(
        models
            .iter()
            .map(|model| {
                (
                    model.id.clone(),
                    serde_json::json!({
                        "description": model.description,
                        "limit": {
                            "context": model.context_window,
                            "output": model.max_output_tokens,
                        },
                        "modalities": {"input": model_input(model), "output": ["text"]},
                        "name": model.display_name,
                    }),
                )
            })
            .collect(),
    )
}

fn openclaw_model_aliases(models: &[CodingModelProfile]) -> serde_json::Value {
    serde_json::Value::Object(
        models
            .iter()
            .map(|model| {
                (
                    format!("nan/{}", model.id),
                    serde_json::json!({"alias": model.display_name}),
                )
            })
            .collect(),
    )
}

fn openclaw_model_catalog(models: &[CodingModelProfile]) -> serde_json::Value {
    serde_json::Value::Array(
        models
            .iter()
            .map(|model| {
                serde_json::json!({
                    "contextWindow": model.context_window,
                    "id": model.id,
                    "input": model_input(model),
                    "maxTokens": model.max_output_tokens,
                    "name": model.display_name,
                    "reasoning": false,
                })
            })
            .collect(),
    )
}

fn qwen_code_model_catalog(
    models: &[CodingModelProfile],
    provider_base_url: &str,
) -> serde_json::Value {
    serde_json::Value::Array(
        models
            .iter()
            .map(|model| {
                serde_json::json!({
                    "baseUrl": provider_base_url,
                    "description": model.description,
                    "envKey": "OPENAI_API_KEY",
                    "generationConfig": {
                        "contextWindowSize": model.context_window,
                        "modalities": {"image": model.image_input},
                        "samplingParams": {"max_tokens": model.max_output_tokens},
                    },
                    "id": model.id,
                    "name": model.display_name,
                })
            })
            .collect(),
    )
}

fn deepseek_model_catalog(models: &[CodingModelProfile]) -> Result<String, String> {
    let mut output = String::new();
    for model in models {
        let id = serde_json::to_string(&model.id)
            .map_err(|error| format!("could not serialize a NaN model ID: {error}"))?;
        let name = serde_json::to_string(&model.display_name)
            .map_err(|error| format!("could not serialize a NaN model name: {error}"))?;
        let input = if model.image_input {
            "[text, image]"
        } else {
            "[text]"
        };
        write!(
            output,
            "          - id: {id}\n            name: {name}\n            contextWindow: {}\n            maxTokens: {}\n            input: {input}\n",
            model.context_window, model.max_output_tokens
        )
        .map_err(|error| format!("could not render the DeepSeek model catalog: {error}"))?;
    }
    Ok(output)
}

fn model_input(model: &CodingModelProfile) -> serde_json::Value {
    if model.image_input {
        serde_json::json!(["text", "image"])
    } else {
        serde_json::json!(["text"])
    }
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
    #[error("could not materialize the live NaN model catalog: {0}")]
    ModelCatalog(String),
}
