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
    json!({
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
    })
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
    if let ReasoningPolicy::Effort { supported, default } = model.reasoning {
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
            "effortMap": effort_map.clone()
        });
        value["compat"]["reasoningEffortMap"] = effort_map;
    }
    value
}

const fn reasoning_effort_name(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
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
