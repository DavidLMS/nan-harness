use crate::temporary::{TemporaryError, TemporaryWorkspace};
use nan_harness_core::launch_plan::{
    AIDER_MODEL_METADATA_PLACEHOLDER, AIDER_MODEL_SETTINGS_PLACEHOLDER,
    ARTIFACT_PLACEHOLDER_PREFIX, BRIDGE_BASE_URL_PLACEHOLDER, CLAUDE_AVAILABLE_MODELS_PLACEHOLDER,
    CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER, CLINE_MODEL_CATALOG_PLACEHOLDER,
    CODEX_MODEL_CATALOG_PLACEHOLDER, DEEPSEEK_MODEL_CATALOG_PLACEHOLDER,
    FX_GATEWAY_CHAT_URL_PLACEHOLDER, GOOSE_MODEL_CATALOG_PLACEHOLDER,
    HERMES_MODEL_CATALOG_PLACEHOLDER, KIMI_CODE_MODEL_CATALOG_PLACEHOLDER,
    OPENCLAW_MODEL_ALIASES_PLACEHOLDER, OPENCLAW_MODEL_CATALOG_PLACEHOLDER,
    OPENCODE_MODEL_CATALOG_PLACEHOLDER, PI_MODEL_CATALOG_PLACEHOLDER,
    PROVIDER_BASE_URL_PLACEHOLDER, QWEN_CODE_MODEL_CATALOG_PLACEHOLDER,
    SELECTED_MODEL_CAPABILITIES_PLACEHOLDER, SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER,
    SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER, SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER,
    USER_HOME_PLACEHOLDER,
};
use nan_harness_core::model::{ReasoningEffort, ReasoningPolicy};
use nan_harness_core::{
    CodingModelProfile, LaunchPlan, SecretError, SecretRef, SecretStore, SecretValue,
    claude_gateway_model_id,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

pub(crate) struct BridgePreparation {
    pub(crate) base_url: String,
    pub(crate) chat_url: Option<String>,
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
                    let argument = render_model_catalogs(
                        &argument,
                        provider_base_url,
                        &plan.model.resolved_id,
                        model_catalog,
                    )
                    .map_err(PreparedError::ModelCatalog)?;
                    render_runtime_value(
                        &argument,
                        provider_base_url,
                        bridge_base_url,
                        bridge
                            .as_ref()
                            .and_then(|values| values.chat_url.as_deref()),
                    )
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
                            bridge
                                .as_ref()
                                .and_then(|values| values.chat_url.as_deref()),
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

    pub(crate) fn temporary_root(&self, has_artifacts: bool) -> Option<PathBuf> {
        has_artifacts.then(|| self.workspace.root().to_path_buf())
    }

    pub(crate) fn artifact_file(&self, artifact_id: &str, relative: &str) -> Option<PathBuf> {
        self.workspace
            .path(artifact_id)
            .map(|path| path.join(relative))
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
    bridge_chat_url: Option<&str>,
) -> Result<String, PreparedError> {
    let value = value.replace(USER_HOME_PLACEHOLDER, &user_home.to_string_lossy());
    let value = render_model_catalogs(&value, provider_base_url, selected_model_id, model_catalog)
        .map_err(PreparedError::ModelCatalog)?;
    render_runtime_value(&value, provider_base_url, bridge_base_url, bridge_chat_url)
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
        .chain(plan.process.arguments.iter().map(String::as_str))
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
        KIMI_CODE_MODEL_CATALOG_PLACEHOLDER,
        CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER,
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
    let models = unique_models(models);
    let mut rendered = template.to_owned();
    render_selected_model(&mut rendered, selected_model_id, &models)?;
    replace_json_placeholder(
        &mut rendered,
        AIDER_MODEL_METADATA_PLACEHOLDER,
        &aider_model_metadata(&models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        AIDER_MODEL_SETTINGS_PLACEHOLDER,
        &aider_model_settings(&models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        CLINE_MODEL_CATALOG_PLACEHOLDER,
        &cline_model_catalog(&models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        GOOSE_MODEL_CATALOG_PLACEHOLDER,
        &goose_model_catalog(&models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        HERMES_MODEL_CATALOG_PLACEHOLDER,
        &hermes_model_catalog(&models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        PI_MODEL_CATALOG_PLACEHOLDER,
        &pi_model_catalog(&models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        OPENCODE_MODEL_CATALOG_PLACEHOLDER,
        &opencode_model_catalog(&models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        OPENCLAW_MODEL_ALIASES_PLACEHOLDER,
        &openclaw_model_aliases(&models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        OPENCLAW_MODEL_CATALOG_PLACEHOLDER,
        &openclaw_model_catalog(&models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        QWEN_CODE_MODEL_CATALOG_PLACEHOLDER,
        &qwen_code_model_catalog(&models, provider_base_url),
    )?;
    rendered = rendered.replace(
        DEEPSEEK_MODEL_CATALOG_PLACEHOLDER,
        &deepseek_model_catalog(&models)?,
    );
    rendered = rendered.replace(
        KIMI_CODE_MODEL_CATALOG_PLACEHOLDER,
        &kimi_code_model_catalog(&models, selected_model_id)?,
    );
    rendered = render_claude_model_presentations(&rendered, selected_model_id, &models)?;
    Ok(rendered)
}

/// Claude Code exposes one model per built-in family plus a single custom slot.
const CLAUDE_MODEL_FAMILIES: [&str; 3] = ["OPUS", "SONNET", "HAIKU"];

/// Expands the Claude Code model picker from the live NaN catalog.
///
/// The placeholder is an `env` key in the settings artifact: it is removed and replaced by the
/// `ANTHROPIC_DEFAULT_*_MODEL` and `ANTHROPIC_CUSTOM_MODEL_OPTION` entries for the models this
/// credential can actually reach, so retired models never reach the picker.
fn render_claude_model_presentations(
    template: &str,
    selected_model_id: &str,
    models: &[CodingModelProfile],
) -> Result<String, String> {
    if !template.contains(CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER) {
        return Ok(template.to_owned());
    }
    let mut settings = serde_json::from_str::<serde_json::Value>(template)
        .map_err(|error| format!("Claude Code settings are not valid JSON: {error}"))?;
    let environment = settings
        .get_mut("env")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| "Claude Code settings have no 'env' object".to_owned())?;
    environment.remove(CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER);
    for (key, value) in claude_model_presentations(selected_model_id, models) {
        environment.insert(key, serde_json::Value::String(value));
    }
    serde_json::to_string(&settings)
        .map_err(|error| format!("could not serialize the Claude Code settings: {error}"))
}

fn claude_model_presentations(
    selected_model_id: &str,
    models: &[CodingModelProfile],
) -> Vec<(String, String)> {
    let selected = models.iter().find(|model| model.id == selected_model_id);
    let ordered = selected
        .into_iter()
        .chain(models.iter().filter(|model| model.id != selected_model_id))
        .take(CLAUDE_MODEL_FAMILIES.len() + 1);

    let mut entries = Vec::new();
    for (slot, model) in ordered.enumerate() {
        let prefix = CLAUDE_MODEL_FAMILIES.get(slot).map_or_else(
            || "ANTHROPIC_CUSTOM_MODEL_OPTION".to_owned(),
            |family| format!("ANTHROPIC_DEFAULT_{family}_MODEL"),
        );
        entries.push((prefix.clone(), claude_gateway_model_id(&model.id)));
        entries.push((format!("{prefix}_NAME"), model.display_name.clone()));
        entries.push((format!("{prefix}_DESCRIPTION"), model.description.clone()));
    }
    entries
}

fn unique_models(models: &[CodingModelProfile]) -> Vec<CodingModelProfile> {
    let mut seen = BTreeSet::new();
    models
        .iter()
        .filter(|model| seen.insert(model.id.clone()))
        .cloned()
        .collect()
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
    let capabilities = match (model.image_input, reasoning_capable(model.reasoning)) {
        (true, true) => "image_in,thinking",
        (true, false) => "image_in",
        (false, true) => "thinking",
        (false, false) => "",
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
                        "supports_reasoning": reasoning_capable(model.reasoning),
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
                let mut settings = serde_json::json!({
                    "edit_format": "diff",
                    "editor_model_name": name,
                    "name": name,
                    "use_repo_map": true,
                    "weak_model_name": name,
                });
                if model.id == "deepseek-v4-flash"
                    && let ReasoningPolicy::Effort { default, .. } = model.reasoning
                {
                    settings["reasoning_effort"] = serde_json::json!(effort_name(default));
                }
                settings
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
                        "reasoningPolicy": model.reasoning,
                        "reasoningControl": "metadata-only",
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
                    "reasoning_policy": model.reasoning,
                    "reasoning_control": "passthrough",
                })
            })
            .collect(),
    )
}

fn hermes_model_catalog(models: &[CodingModelProfile]) -> serde_json::Value {
    // Hermes' provider schema accepts only model IDs. Reasoning remains upstream passthrough.
    serde_json::Value::Array(models.iter().map(|model| model.id.clone().into()).collect())
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
                        "reasoningPolicy": model.reasoning,
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
                let mut entry = serde_json::json!({
                    "description": model.description,
                    "limit": {
                        "context": model.context_window,
                        "output": model.max_output_tokens,
                    },
                    "modalities": {"input": model_input(model), "output": ["text"]},
                    "name": model.display_name,
                    "reasoning": reasoning_capable(model.reasoning),
                });
                if let ReasoningPolicy::Effort { supported, .. } = model.reasoning {
                    entry["variants"] = serde_json::Value::Object(
                        supported
                            .into_iter()
                            .map(|effort| {
                                (
                                    effort_name(effort).to_owned(),
                                    serde_json::json!({"reasoningEffort": effort_name(effort)}),
                                )
                            })
                            .collect(),
                    );
                } else if let ReasoningPolicy::Toggle { default_enabled } = model.reasoning {
                    entry["variants"] = serde_json::json!({
                        "thinking": {"enable_thinking": true},
                        "no-thinking": {"enable_thinking": false},
                    });
                    entry["defaultVariant"] = serde_json::json!(if default_enabled {
                        "thinking"
                    } else {
                        "no-thinking"
                    });
                }
                (model.id.clone(), entry)
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
                    "reasoning": reasoning_capable(model.reasoning),
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
                let mut entry = serde_json::json!({
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
                });
                if let ReasoningPolicy::Toggle { default_enabled } = model.reasoning {
                    entry["generationConfig"]["samplingParams"]["enable_thinking"] =
                        serde_json::json!(default_enabled);
                }
                entry
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
            "          - id: {id}\n            name: {name}\n            contextWindow: {}\n            maxTokens: {}\n            input: {input}\n            reasoning: {}\n",
            model.context_window,
            model.max_output_tokens,
            reasoning_capable(model.reasoning)
        )
        .map_err(|error| format!("could not render the DeepSeek model catalog: {error}"))?;
    }
    Ok(output)
}

fn kimi_code_model_catalog(
    models: &[CodingModelProfile],
    selected_model_id: &str,
) -> Result<String, String> {
    let mut model_tables = toml::map::Map::new();
    for model in models.iter().filter(|model| model.id != selected_model_id) {
        let context_window = i64::try_from(model.context_window)
            .map_err(|_| format!("model '{}' context window is too large for TOML", model.id))?;
        let max_output_tokens = i64::try_from(model.max_output_tokens)
            .map_err(|_| format!("model '{}' output limit is too large for TOML", model.id))?;
        let capabilities = if model.image_input && reasoning_capable(model.reasoning) {
            vec![
                toml::Value::String("image_in".to_owned()),
                toml::Value::String("thinking".to_owned()),
            ]
        } else if model.image_input {
            vec![toml::Value::String("image_in".to_owned())]
        } else if reasoning_capable(model.reasoning) {
            vec![toml::Value::String("thinking".to_owned())]
        } else {
            Vec::new()
        };
        let model_config = toml::Table::from_iter([
            ("capabilities".to_owned(), toml::Value::Array(capabilities)),
            (
                "display_name".to_owned(),
                toml::Value::String(model.display_name.clone()),
            ),
            (
                "max_context_size".to_owned(),
                toml::Value::Integer(context_window),
            ),
            (
                "max_output_size".to_owned(),
                toml::Value::Integer(max_output_tokens),
            ),
            ("model".to_owned(), toml::Value::String(model.id.clone())),
            (
                "provider".to_owned(),
                toml::Value::String("__kimi_env__".to_owned()),
            ),
        ]);
        model_tables.insert(
            format!("nan/{}", model.id),
            toml::Value::Table(model_config),
        );
    }
    toml::to_string(&toml::Value::Table(toml::Table::from_iter([(
        "models".to_owned(),
        toml::Value::Table(model_tables),
    )])))
    .map_err(|error| format!("could not render the Kimi Code model catalog: {error}"))
}

fn model_input(model: &CodingModelProfile) -> serde_json::Value {
    if model.image_input {
        serde_json::json!(["text", "image"])
    } else {
        serde_json::json!(["text"])
    }
}

fn reasoning_capable(policy: ReasoningPolicy) -> bool {
    matches!(
        policy,
        ReasoningPolicy::Toggle { .. } | ReasoningPolicy::Effort { .. } | ReasoningPolicy::AlwaysOn
    )
}

fn effort_name(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
    }
}

fn render_runtime_value(
    value: &str,
    provider_base_url: &str,
    bridge_base_url: Option<&str>,
    bridge_chat_url: Option<&str>,
) -> Result<String, PreparedError> {
    let mut rendered = value.replace(PROVIDER_BASE_URL_PLACEHOLDER, provider_base_url);
    if rendered.contains(BRIDGE_BASE_URL_PLACEHOLDER) {
        let bridge_base_url = bridge_base_url.ok_or_else(|| {
            PreparedError::UnresolvedPlaceholder(BRIDGE_BASE_URL_PLACEHOLDER.to_owned())
        })?;
        rendered = rendered.replace(BRIDGE_BASE_URL_PLACEHOLDER, bridge_base_url);
    }
    if rendered.contains(FX_GATEWAY_CHAT_URL_PLACEHOLDER) {
        let bridge_chat_url = bridge_chat_url.ok_or_else(|| {
            PreparedError::UnresolvedPlaceholder(FX_GATEWAY_CHAT_URL_PLACEHOLDER.to_owned())
        })?;
        rendered = rendered.replace(FX_GATEWAY_CHAT_URL_PLACEHOLDER, bridge_chat_url);
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

#[cfg(test)]
mod tests {
    use super::{PreparedLaunch, requires_model_catalog};
    use nan_harness_core::launch_plan::{
        CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER, LaunchPlan, OPENCODE_MODEL_CATALOG_PLACEHOLDER,
        PI_MODEL_CATALOG_PLACEHOLDER, SELECTED_MODEL_CAPABILITIES_PLACEHOLDER,
    };
    use nan_harness_core::model::ReasoningPolicy;
    use nan_harness_core::{CodingModelProfile, ProfileSource, coding_model_profile};
    use std::collections::BTreeSet;

    fn model(id: &str) -> CodingModelProfile {
        CodingModelProfile {
            id: id.to_owned(),
            display_name: format!("NaN · {id}"),
            description: "test model".to_owned(),
            context_window: 262_144,
            max_output_tokens: 32_768,
            image_input: false,
            reasoning: ReasoningPolicy::Unknown,
            source: ProfileSource::Generic,
        }
    }

    fn known_models() -> Vec<CodingModelProfile> {
        [
            "qwen3.6",
            "deepseek-v4-flash",
            "mimo-v2.5",
            "gemma4",
            "glm5.2",
        ]
        .into_iter()
        .map(|id| coding_model_profile(id).expect("known coding model"))
        .collect()
    }

    fn claude_settings_template() -> String {
        serde_json::json!({
            "availableModels": "{runtime:claude_available_models}",
            "model": "anthropic/nan/qwen3.6",
            "env": {
                "ANTHROPIC_MODEL": "anthropic/nan/qwen3.6",
                CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER: ""
            }
        })
        .to_string()
    }

    #[test]
    fn claude_picker_slots_come_from_the_discovered_catalog() {
        let models = [
            coding_model_profile("qwen3.6").expect("known coding model"),
            coding_model_profile("mimo-v2.5").expect("known coding model"),
        ];
        let rendered = super::render_model_catalogs(
            &claude_settings_template(),
            "https://nan.invalid/v1",
            "qwen3.6",
            Some(&models),
        )
        .expect("Claude settings should render");
        let settings: serde_json::Value =
            serde_json::from_str(&rendered).expect("rendered settings should be valid JSON");
        let environment = settings["env"]
            .as_object()
            .expect("settings should keep an env object");

        assert!(!environment.contains_key(CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER));
        assert_eq!(environment["ANTHROPIC_MODEL"], "anthropic/nan/qwen3.6");
        assert_eq!(
            environment["ANTHROPIC_DEFAULT_OPUS_MODEL"],
            "anthropic/nan/qwen3.6"
        );
        assert_eq!(
            environment["ANTHROPIC_DEFAULT_OPUS_MODEL_NAME"],
            "NaN · Qwen 3.6"
        );
        assert_eq!(
            environment["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            "anthropic/nan/mimo-v2.5"
        );
        assert!(
            !environment.contains_key("ANTHROPIC_DEFAULT_HAIKU_MODEL"),
            "slots without a discovered model must stay unset"
        );
        assert!(!environment.contains_key("ANTHROPIC_CUSTOM_MODEL_OPTION"));
        assert!(
            !rendered.contains("deepseek"),
            "a model missing from discovery must never reach the picker"
        );
    }

    #[test]
    fn claude_picker_puts_the_selected_model_first() {
        let models = known_models();
        let rendered = super::render_model_catalogs(
            &claude_settings_template(),
            "https://nan.invalid/v1",
            "gemma4",
            Some(&models),
        )
        .expect("Claude settings should render");
        let settings: serde_json::Value =
            serde_json::from_str(&rendered).expect("rendered settings should be valid JSON");
        let environment = &settings["env"];

        assert_eq!(
            environment["ANTHROPIC_DEFAULT_OPUS_MODEL"],
            "anthropic/nan/gemma4"
        );
        let slots = [
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_CUSTOM_MODEL_OPTION",
        ]
        .map(|slot| environment[slot].as_str().expect("slot should be filled"));
        assert_eq!(
            slots.iter().collect::<BTreeSet<_>>().len(),
            slots.len(),
            "picker slots must not repeat a model"
        );
    }

    #[test]
    fn model_catalog_rendering_deduplicates_ids_stably() {
        let models = [model("qwen3.6"), model("qwen3.6"), model("mimo-v2.5")];
        let template = format!(
            r#"{{"opencode":{OPENCODE_MODEL_CATALOG_PLACEHOLDER},"pi":{PI_MODEL_CATALOG_PLACEHOLDER}}}"#
        );
        let rendered = super::render_model_catalogs(
            &template,
            "https://api.nan.builders/v1",
            "qwen3.6",
            Some(&models),
        )
        .expect("catalogs should render");
        let value: serde_json::Value =
            serde_json::from_str(&rendered).expect("rendered catalogs should be JSON");

        assert_eq!(value["opencode"].as_object().expect("map").len(), 2);
        assert_eq!(value["pi"].as_object().expect("map").len(), 2);
        assert_eq!(
            value["opencode"]
                .as_object()
                .expect("map")
                .keys()
                .collect::<Vec<_>>(),
            &[&"mimo-v2.5".to_owned(), &"qwen3.6".to_owned()]
        );
    }

    #[test]
    fn catalog_placeholders_in_arguments_trigger_live_discovery() {
        let source = include_str!("../../nan-harness-core/tests/fixtures/launch-plan.direct.json");
        let mut plan: LaunchPlan = serde_json::from_str(source).expect("fixture should parse");
        plan.process.arguments = vec![OPENCODE_MODEL_CATALOG_PLACEHOLDER.to_owned()];

        assert!(requires_model_catalog(&plan));
    }

    #[test]
    fn model_catalog_placeholders_in_arguments_are_rendered() {
        let source = include_str!("../../nan-harness-core/tests/fixtures/launch-plan.direct.json");
        let mut plan: LaunchPlan = serde_json::from_str(source).expect("fixture should parse");
        plan.process.arguments = vec![OPENCODE_MODEL_CATALOG_PLACEHOLDER.to_owned()];
        let models = [model("qwen3.6")];

        let prepared =
            PreparedLaunch::prepare(&plan, "https://api.nan.builders/v1", None, Some(&models))
                .expect("argument catalog should render");

        assert!(prepared.arguments()[0].contains("qwen3.6"));
        assert!(!prepared.arguments()[0].contains(OPENCODE_MODEL_CATALOG_PLACEHOLDER));
    }

    #[test]
    fn native_reasoning_catalogs_are_model_aware() {
        let models = known_models();
        let opencode = super::opencode_model_catalog(&models);
        assert_eq!(opencode["qwen3.6"]["reasoning"], true);
        assert_eq!(opencode["qwen3.6"]["defaultVariant"], "thinking");
        assert_eq!(opencode["gemma4"]["defaultVariant"], "no-thinking");
        assert_eq!(
            opencode["deepseek-v4-flash"]["variants"]["high"]["reasoningEffort"],
            "high"
        );
        assert_eq!(opencode["glm5.2"]["reasoning"], false);
        assert!(opencode["glm5.2"].get("variants").is_none());

        let qwen = super::qwen_code_model_catalog(&models, "https://nan.invalid/v1");
        let by_id = |id: &str| {
            qwen.as_array()
                .expect("catalog")
                .iter()
                .find(|entry| entry["id"] == id)
                .expect("model")
        };
        assert_eq!(
            by_id("qwen3.6")["generationConfig"]["samplingParams"]["enable_thinking"],
            true
        );
        assert_eq!(
            by_id("gemma4")["generationConfig"]["samplingParams"]["enable_thinking"],
            false
        );
        assert!(
            by_id("deepseek-v4-flash")["generationConfig"]["samplingParams"]
                .get("reasoning_effort")
                .is_none()
        );
        assert_eq!(
            by_id("qwen3.6")["generationConfig"]["samplingParams"]["max_tokens"],
            65_536
        );
    }

    #[test]
    fn metadata_and_capabilities_do_not_claim_reasoning_for_every_model() {
        let models = known_models();
        let openclaw = super::openclaw_model_catalog(&models);
        let by_id = |id: &str| {
            openclaw
                .as_array()
                .expect("catalog")
                .iter()
                .find(|entry| entry["id"] == id)
                .expect("model")
        };
        assert_eq!(by_id("mimo-v2.5")["reasoning"], true);
        assert_eq!(by_id("glm5.2")["reasoning"], false);

        let selected = super::render_model_catalogs(
            SELECTED_MODEL_CAPABILITIES_PLACEHOLDER,
            "https://nan.invalid/v1",
            "glm5.2",
            Some(&models),
        )
        .expect("selected capabilities");
        assert_eq!(selected, "");

        let kimi = super::kimi_code_model_catalog(&models, "qwen3.6").expect("Kimi catalog");
        assert!(kimi.contains("thinking"));
        let glm_section = kimi
            .split("[models.\"nan/glm5.2\"]")
            .nth(1)
            .expect("glm section");
        assert!(
            !glm_section
                .lines()
                .take(8)
                .any(|line| line.contains("thinking"))
        );

        let pi = super::pi_model_catalog(&models);
        assert_eq!(pi["qwen3.6"]["reasoningPolicy"]["kind"], "toggle");
        assert_eq!(pi["glm5.2"]["reasoningPolicy"]["kind"], "unsupported");

        let cline = super::cline_model_catalog(&models);
        assert_eq!(cline["qwen3.6"]["reasoningControl"], "metadata-only");
        assert_eq!(cline["glm5.2"]["reasoningPolicy"]["kind"], "unsupported");

        let goose = super::goose_model_catalog(&models);
        assert!(
            goose
                .as_array()
                .expect("Goose catalog")
                .iter()
                .all(|entry| {
                    entry["reasoning_control"] == "passthrough"
                        && entry.get("reasoning_policy").is_some()
                })
        );

        let deepseek = super::deepseek_model_catalog(&models).expect("DeepSeek catalog");
        assert!(deepseek.contains("id: \"mimo-v2.5\""));
        assert!(deepseek.contains("reasoning: true"));
        let glm_section = deepseek
            .split("id: \"glm5.2\"")
            .nth(1)
            .expect("DeepSeek GLM section");
        assert!(glm_section.contains("reasoning: false"));

        let hermes = super::hermes_model_catalog(&models);
        assert!(
            hermes
                .as_array()
                .expect("Hermes IDs only")
                .iter()
                .all(serde_json::Value::is_string)
        );
    }

    #[test]
    fn aider_only_sets_reasoning_effort_for_deepseek() {
        let settings = super::aider_model_settings(&known_models());
        let by_name = |name: &str| {
            settings
                .as_array()
                .expect("settings")
                .iter()
                .find(|entry| entry["name"] == name)
                .expect("model")
        };
        assert_eq!(
            by_name("openai/deepseek-v4-flash")["reasoning_effort"],
            "medium"
        );
        assert!(by_name("openai/qwen3.6").get("reasoning_effort").is_none());
        assert!(
            by_name("openai/mimo-v2.5")
                .get("reasoning_effort")
                .is_none()
        );
    }
}
