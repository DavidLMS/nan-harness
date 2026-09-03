use nan_harness_core::CodingModelProfile;
use nan_harness_core::model::ReasoningPolicy;

use super::{effort_name, model_input, reasoning_capable};

pub(in crate::prepared) fn aider_model_metadata(
    models: &[CodingModelProfile],
) -> serde_json::Value {
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

pub(in crate::prepared) fn aider_model_settings(
    models: &[CodingModelProfile],
) -> serde_json::Value {
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

pub(in crate::prepared) fn cline_model_catalog(models: &[CodingModelProfile]) -> serde_json::Value {
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

pub(in crate::prepared) fn goose_model_catalog(models: &[CodingModelProfile]) -> serde_json::Value {
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

pub(in crate::prepared) fn hermes_model_catalog(
    models: &[CodingModelProfile],
) -> serde_json::Value {
    // Hermes' provider schema accepts only model IDs. Reasoning remains upstream passthrough.
    serde_json::Value::Array(models.iter().map(|model| model.id.clone().into()).collect())
}

pub(in crate::prepared) fn replace_json_placeholder(
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

pub(in crate::prepared) fn pi_model_catalog(models: &[CodingModelProfile]) -> serde_json::Value {
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

pub(in crate::prepared) fn opencode_model_catalog(
    models: &[CodingModelProfile],
) -> serde_json::Value {
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

pub(in crate::prepared) fn openclaw_model_aliases(
    models: &[CodingModelProfile],
) -> serde_json::Value {
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

pub(in crate::prepared) fn openclaw_model_catalog(
    models: &[CodingModelProfile],
) -> serde_json::Value {
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

pub(in crate::prepared) fn qwen_code_model_catalog(
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
