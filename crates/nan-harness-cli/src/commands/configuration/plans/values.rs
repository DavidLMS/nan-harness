use super::super::{CodingModelProfile, ReasoningEffort, ReasoningPolicy, Value, json};

pub(crate) fn pi_provider(base_url: &str, models: &[CodingModelProfile]) -> Value {
    json!({
        "baseUrl": base_url,
        "api": "openai-completions",
        "apiKey": "NAN_API_KEY",
        "models": models.iter().map(pi_model).collect::<Vec<_>>()
    })
}

pub(crate) fn pi_model(model: &CodingModelProfile) -> Value {
    let mut value = json!({
        "id": model.id,
        "name": model.display_name,
        "reasoning": !matches!(model.reasoning, ReasoningPolicy::Unsupported | ReasoningPolicy::Unknown),
        "input": if model.image_input { vec!["text", "image"] } else { vec!["text"] },
        "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
        "contextWindow": model.context_window,
        "maxTokens": model.max_output_tokens,
        "compat": {
            "supportsDeveloperRole": false,
            "supportsReasoningEffort": matches!(model.reasoning, ReasoningPolicy::Effort { .. }),
            "maxTokensField": "max_tokens"
        }
    });
    let mut levels = ["off", "minimal", "low", "medium", "high", "xhigh", "max"]
        .into_iter()
        .map(|level| (level.to_owned(), Value::Null))
        .collect::<serde_json::Map<_, _>>();
    match model.reasoning {
        ReasoningPolicy::Effort {
            supported,
            supports_disabled,
            ..
        } => {
            for effort in supported {
                let name = reasoning_effort_name(effort);
                levels.insert(name.to_owned(), json!(name));
            }
            if supports_disabled {
                levels.insert("off".to_owned(), json!("none"));
            }
        }
        ReasoningPolicy::Toggle { .. } => {
            levels.insert("off".to_owned(), json!("none"));
            levels.insert("high".to_owned(), json!("high"));
        }
        _ => {}
    }
    value["thinkingLevelMap"] = Value::Object(levels);
    value
}

pub(crate) fn omp_provider(api_key: &str, base_url: &str, models: &[CodingModelProfile]) -> Value {
    json!({
        "baseUrl": base_url,
        "api": "openai-completions",
        "apiKey": api_key,
        "authHeader": true,
        "models": models.iter().map(omp_model).collect::<Vec<_>>()
    })
}

pub(crate) fn omp_model(model: &CodingModelProfile) -> Value {
    let mut value = pi_model(model);
    if let Some(fields) = value.as_object_mut() {
        fields.remove("thinkingLevelMap");
    }
    // OMP treats reasoning as an adjustable-control flag, independently of SSE trace parsing.
    let toggle = matches!(model.reasoning, ReasoningPolicy::Toggle { .. });
    let effort = matches!(model.reasoning, ReasoningPolicy::Effort { .. });
    value["reasoning"] = json!(toggle || effort);
    value["compat"]["thinkingFormat"] = json!(if toggle {
        "qwen-chat-template"
    } else {
        "openai"
    });
    value["compat"]["reasoningDisableMode"] = json!(if toggle {
        "qwen-template-false"
    } else {
        "omit"
    });
    value["compat"]["omitReasoningEffort"] = json!(!effort);
    value["compat"]["qwenPreserveThinking"] = json!(false);
    value["compat"]["qwenTemplateReasoningEffort"] = json!(false);
    if toggle {
        value["thinking"] = json!({"mode": "effort", "efforts": ["high"], "defaultLevel": "high", "requiresEffort": false});
    }
    if let ReasoningPolicy::Effort {
        supported,
        default,
        supports_disabled,
    } = model.reasoning
    {
        let supported = supported
            .into_iter()
            .map(|effort| Value::String(reasoning_effort_name(effort).to_owned()))
            .collect::<Vec<_>>();
        let default = Value::String(reasoning_effort_name(default).to_owned());
        let effort_map = Value::Object(
            supported
                .iter()
                .filter_map(|effort| {
                    effort
                        .as_str()
                        .map(|name| (name.to_owned(), Value::String(name.to_owned())))
                })
                .collect(),
        );
        value["thinking"] = json!({
            "mode": "effort",
            "efforts": supported,
            "defaultLevel": default,
            "requiresEffort": false,
            "effortMap": effort_map.clone()
        });
        value["compat"]["reasoningEffortMap"] = effort_map;
        value["compat"]["thinkingFormat"] = json!("openai");
        value["compat"]["omitReasoningEffort"] = json!(false);
        value["compat"]["reasoningDisableMode"] = json!(if supports_disabled {
            "none-effort"
        } else {
            "omit"
        });
    }
    value
}

const fn reasoning_effort_name(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::Max => "max",
    }
}

pub(crate) fn openclaw_provider(
    api_key: &str,
    base_url: &str,
    models: &[CodingModelProfile],
) -> Value {
    json!({
        "api": "openai-completions",
        "apiKey": api_key,
        "baseUrl": base_url,
        "models": models.iter().map(|model| json!({
            "id": model.id,
            "name": model.display_name,
            "reasoning": !matches!(model.reasoning, ReasoningPolicy::Unsupported | ReasoningPolicy::Unknown),
            "input": if model.image_input { vec!["text", "image"] } else { vec!["text"] },
            "contextWindow": model.context_window,
            "maxTokens": model.max_output_tokens
        })).collect::<Vec<_>>()
    })
}

pub(crate) fn openclaw_aliases(models: &[CodingModelProfile]) -> Value {
    Value::Object(
        models
            .iter()
            .map(|model| (format!("nan/{}", model.id), json!({})))
            .collect(),
    )
}

pub(crate) fn cline_models(models: &[CodingModelProfile]) -> Value {
    Value::Array(
        models
            .iter()
            .map(|model| {
                json!({
                    "id": model.id,
                    "name": model.display_name,
                    "contextWindow": model.context_window,
                    "maxTokens": model.max_output_tokens,
                    "supportsImages": model.image_input,
                    "supportsReasoning": !matches!(model.reasoning, ReasoningPolicy::Unsupported | ReasoningPolicy::Unknown)
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod reasoning_tests {
    use super::*;
    use nan_harness_core::coding_model_profile;

    #[test]
    fn pi_places_supported_levels_on_the_native_model() {
        let model = pi_model(&coding_model_profile("qwen3.6").unwrap());
        assert_eq!(model["thinkingLevelMap"]["off"], "none");
        assert_eq!(model["thinkingLevelMap"]["max"], "max");
        assert!(model["compat"].get("thinkingLevelMap").is_none());
        assert_eq!(model["compat"]["supportsReasoningEffort"], true);
        let adaptive = pi_model(&coding_model_profile("deepseek-v4-flash").unwrap());
        assert_eq!(adaptive["compat"]["supportsReasoningEffort"], false);
        assert!(
            adaptive["thinkingLevelMap"]
                .as_object()
                .unwrap()
                .values()
                .all(Value::is_null)
        );
    }

    #[test]
    fn omp_overrides_family_detection_and_disables_only_supported_models() {
        for (id, disable) in [
            ("qwen3.6", "none-effort"),
            ("gemma4", "none-effort"),
            ("glm5.3-flash", "omit"),
        ] {
            let model = omp_model(&coding_model_profile(id).unwrap());
            assert_eq!(
                model["thinking"]["efforts"],
                json!(["low", "medium", "high", "max"])
            );
            assert_eq!(model["compat"]["thinkingFormat"], "openai");
            assert_eq!(model["compat"]["reasoningDisableMode"], disable);
            assert_eq!(model["compat"]["reasoningEffortMap"]["max"], "max");
            assert!(model.get("thinkingLevelMap").is_none());
        }
    }
}
