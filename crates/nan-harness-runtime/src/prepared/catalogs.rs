use nan_harness_core::launch_plan::{
    AIDER_MODEL_METADATA_PLACEHOLDER, AIDER_MODEL_SETTINGS_PLACEHOLDER,
    CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER, CLINE_MODEL_CATALOG_PLACEHOLDER,
    DEEPSEEK_MODEL_CATALOG_PLACEHOLDER, GOOSE_MODEL_CATALOG_PLACEHOLDER,
    HERMES_MODEL_CATALOG_PLACEHOLDER, KIMI_CODE_MODEL_CATALOG_PLACEHOLDER,
    OPENCLAW_MODEL_ALIASES_PLACEHOLDER, OPENCLAW_MODEL_CATALOG_PLACEHOLDER,
    OPENCODE_MODEL_CATALOG_PLACEHOLDER, PI_MODEL_CATALOG_PLACEHOLDER,
    QWEN_CODE_MODEL_CATALOG_PLACEHOLDER, SELECTED_MODEL_CAPABILITIES_PLACEHOLDER,
    SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER, SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER,
    SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER, SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER,
};
use nan_harness_core::model::{ReasoningEffort, ReasoningPolicy, ReasoningSelection};
use nan_harness_core::{CodingModelProfile, claude_gateway_model_id};
use std::collections::BTreeSet;
use std::fmt::Write as _;

pub(super) fn contains_model_catalog_placeholder(value: &str) -> bool {
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
        SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER,
    ]
    .iter()
    .any(|placeholder| value.contains(placeholder))
}

pub(super) fn render_model_catalogs(
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

/// Models whose Claude picker presentation has been curated and verified together.
///
/// Keep this separate from the shared bundled-model catalog: adding metadata for a new
/// provider model must not silently move Claude Code back from gateway discovery to its
/// four presentation slots. The order is the curated picker priority after the selected
/// model, with GLM intentionally preferred over Gemma when both are available.
const CLAUDE_CURATED_MODEL_PRIORITY: [&str; 5] = [
    "qwen3.6",
    "deepseek-v4-flash",
    "mimo-v2.5",
    "glm5.2",
    "gemma4",
];

/// Chooses the Claude Code model-picker presentation for the live NaN catalog.
///
/// A catalog containing only the verified curated IDs uses Claude's three family slots plus its
/// custom slot. A catalog containing any other ID leaves the complete picker to gateway discovery
/// and pins only Qwen's Opus compatibility alias for native Auto mode. In both cases the placeholder
/// is removed from the rendered settings artifact.
pub(super) fn render_claude_model_presentations(
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

pub(super) fn claude_model_presentations(
    selected_model_id: &str,
    models: &[CodingModelProfile],
) -> Vec<(String, String)> {
    if models
        .iter()
        .any(|model| !CLAUDE_CURATED_MODEL_PRIORITY.contains(&model.id.as_str()))
    {
        return claude_gateway_model_presentations(models);
    }

    let selected = models.iter().find(|model| model.id == selected_model_id);
    let ordered = selected
        .into_iter()
        .chain(
            CLAUDE_CURATED_MODEL_PRIORITY
                .iter()
                .filter_map(|model_id| models.iter().find(|model| model.id == *model_id))
                .filter(|model| model.id != selected_model_id),
        )
        .take(CLAUDE_MODEL_FAMILIES.len() + 1);

    let mut entries = Vec::new();
    for (slot, model) in ordered.enumerate() {
        let prefix = CLAUDE_MODEL_FAMILIES.get(slot).map_or_else(
            || "ANTHROPIC_CUSTOM_MODEL_OPTION".to_owned(),
            |family| format!("ANTHROPIC_DEFAULT_{family}_MODEL"),
        );
        append_claude_model_presentation(&mut entries, &prefix, model);
    }
    entries
}

/// Gateway discovery owns the complete picker as soon as NaN returns a model outside the
/// verified curated set. Qwen keeps only the Opus compatibility alias required by Claude
/// Code's native Auto permission mode; every model, including Qwen, remains present in the
/// credential-scoped gateway catalog and `availableModels` allowlist.
pub(super) fn claude_gateway_model_presentations(
    models: &[CodingModelProfile],
) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    if let Some(qwen) = models
        .iter()
        .find(|model| model.id == nan_harness_core::CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID)
    {
        append_claude_model_presentation(&mut entries, "ANTHROPIC_DEFAULT_OPUS_MODEL", qwen);
    }
    entries
}

pub(super) fn append_claude_model_presentation(
    entries: &mut Vec<(String, String)>,
    prefix: &str,
    model: &CodingModelProfile,
) {
    entries.push((prefix.to_owned(), claude_gateway_model_id(&model.id)));
    entries.push((format!("{prefix}_NAME"), model.display_name.clone()));
    entries.push((format!("{prefix}_DESCRIPTION"), model.description.clone()));
}

pub(super) fn unique_models(models: &[CodingModelProfile]) -> Vec<CodingModelProfile> {
    let mut seen = BTreeSet::new();
    models
        .iter()
        .filter(|model| seen.insert(model.id.clone()))
        .cloned()
        .collect()
}

pub(super) fn render_selected_model(
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

pub(super) fn aider_model_metadata(models: &[CodingModelProfile]) -> serde_json::Value {
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

pub(super) fn aider_model_settings(models: &[CodingModelProfile]) -> serde_json::Value {
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
                if let ReasoningPolicy::Effort { default, .. } = model.reasoning {
                    settings["reasoning_effort"] = serde_json::json!(effort_name(default));
                }
                settings
            })
            .collect(),
    )
}

pub(super) fn cline_model_catalog(models: &[CodingModelProfile]) -> serde_json::Value {
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

pub(super) fn goose_model_catalog(models: &[CodingModelProfile]) -> serde_json::Value {
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

pub(super) fn hermes_model_catalog(models: &[CodingModelProfile]) -> serde_json::Value {
    // Hermes' provider schema accepts only model IDs. Reasoning remains upstream passthrough.
    serde_json::Value::Array(models.iter().map(|model| model.id.clone().into()).collect())
}

pub(super) fn replace_json_placeholder(
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

pub(super) fn pi_model_catalog(models: &[CodingModelProfile]) -> serde_json::Value {
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

pub(super) fn opencode_model_catalog(models: &[CodingModelProfile]) -> serde_json::Value {
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

pub(super) fn openclaw_model_aliases(models: &[CodingModelProfile]) -> serde_json::Value {
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

pub(super) fn openclaw_model_catalog(models: &[CodingModelProfile]) -> serde_json::Value {
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

pub(super) fn qwen_code_model_catalog(
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

pub(super) fn deepseek_model_catalog(models: &[CodingModelProfile]) -> Result<String, String> {
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

pub(super) fn kimi_code_model_catalog(
    models: &[CodingModelProfile],
    selected_model_id: &str,
) -> Result<String, String> {
    let model_tables: toml::map::Map<String, toml::Value> = models
        .iter()
        .filter(|model| model.id != selected_model_id)
        .map(kimi_model_table)
        .collect::<Result<_, _>>()?;
    render_kimi_model_catalog(model_tables)
}

pub(super) fn kimi_model_table(
    model: &CodingModelProfile,
) -> Result<(String, toml::Value), String> {
    let (context_window, max_output_tokens) = kimi_model_limits(model)?;
    let model_config = toml::Table::from_iter([
        (
            "capabilities".to_owned(),
            toml::Value::Array(kimi_model_capabilities(model)),
        ),
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
    Ok((
        format!("nan/{}", model.id),
        toml::Value::Table(model_config),
    ))
}

pub(super) fn kimi_model_limits(model: &CodingModelProfile) -> Result<(i64, i64), String> {
    let context_window = i64::try_from(model.context_window)
        .map_err(|_| format!("model '{}' context window is too large for TOML", model.id))?;
    let max_output_tokens = i64::try_from(model.max_output_tokens)
        .map_err(|_| format!("model '{}' output limit is too large for TOML", model.id))?;
    Ok((context_window, max_output_tokens))
}

pub(super) fn kimi_model_capabilities(model: &CodingModelProfile) -> Vec<toml::Value> {
    match (model.image_input, reasoning_capable(model.reasoning)) {
        (true, true) => vec![
            toml::Value::String("image_in".to_owned()),
            toml::Value::String("thinking".to_owned()),
        ],
        (true, false) => vec![toml::Value::String("image_in".to_owned())],
        (false, true) => vec![toml::Value::String("thinking".to_owned())],
        (false, false) => Vec::new(),
    }
}

pub(super) fn render_kimi_model_catalog(
    model_tables: toml::map::Map<String, toml::Value>,
) -> Result<String, String> {
    toml::to_string(&toml::Value::Table(toml::Table::from_iter([(
        "models".to_owned(),
        toml::Value::Table(model_tables),
    )])))
    .map_err(|error| format!("could not render the Kimi Code model catalog: {error}"))
}

pub(super) fn model_input(model: &CodingModelProfile) -> serde_json::Value {
    if model.image_input {
        serde_json::json!(["text", "image"])
    } else {
        serde_json::json!(["text"])
    }
}

pub(super) fn reasoning_capable(policy: ReasoningPolicy) -> bool {
    matches!(
        policy,
        ReasoningPolicy::Toggle { .. } | ReasoningPolicy::Effort { .. } | ReasoningPolicy::AlwaysOn
    )
}

pub(super) fn effort_name(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
    }
}

pub(super) fn selected_model_reasoning_effort(
    selected_model_id: &str,
    requested: Option<ReasoningSelection>,
    models: &[CodingModelProfile],
) -> Result<String, String> {
    let model = models
        .iter()
        .find(|model| model.id == selected_model_id)
        .ok_or_else(|| {
            format!(
                "selected model '{selected_model_id}' is not present in the discovered NaN catalog"
            )
        })?;
    let default = model.reasoning.default_selection();
    let selection = requested
        .filter(|selection| model.reasoning.accepts(*selection))
        .unwrap_or(default);
    Ok(match selection {
        ReasoningSelection::Auto | ReasoningSelection::Toggle(false) => "none".to_owned(),
        ReasoningSelection::Toggle(true) => "high".to_owned(),
        ReasoningSelection::Effort(effort) => effort_name(effort).to_owned(),
    })
}

pub(super) fn render_reasoning_effort(value: &str, effort: Option<&str>) -> Result<String, String> {
    if !value.contains(SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER) {
        return Ok(value.to_owned());
    }
    let effort = effort
        .ok_or_else(|| "selected model reasoning requires live NaN model discovery".to_owned())?;
    Ok(value.replace(SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER, effort))
}
