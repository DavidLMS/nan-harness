use nan_harness_core::{CodingModelProfile, ReasoningPolicy};
use serde_json::{Value, json};

/// Prime 0.10 ignores compat on custom model definitions, so thinking toggles
/// need their own provider-level wire format.
#[must_use]
pub fn prime_provider_catalog(
    models: &[CodingModelProfile],
    base_url: &str,
) -> serde_json::Map<String, Value> {
    let mut providers = serde_json::Map::new();
    for (id, toggle) in [("nan", false), ("nan-thinking", true)] {
        let models = models
            .iter()
            .filter(|model| matches!(model.reasoning, ReasoningPolicy::Toggle { .. }) == toggle)
            .map(model_definition)
            .collect::<Vec<_>>();
        providers.insert(id.into(), json!({
            "baseUrl": base_url, "api": "openai-completions", "apiKey": "NAN_API_KEY", "authHeader": true,
            "models": models,
            "compat": {"supportsDeveloperRole": false, "supportsReasoningEffort": !toggle,
                "maxTokensField": "max_tokens", "thinkingFormat": if toggle { "qwen-chat-template" } else { "openai" }}
        }));
    }
    providers
}

fn model_definition(model: &CodingModelProfile) -> Value {
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
                let name = super::effort_name(effort);
                levels.insert(name.to_owned(), json!(name));
            }
            if supports_disabled {
                levels.insert("off".into(), json!("none"));
            }
        }
        ReasoningPolicy::Toggle { .. } => {
            levels.insert("off".into(), json!("none"));
            levels.insert("high".into(), json!("high"));
        }
        _ => {}
    }
    json!({
        "id": model.id, "name": model.display_name,
        "reasoning": matches!(model.reasoning, ReasoningPolicy::Effort { .. } | ReasoningPolicy::Toggle { .. }),
        "thinkingLevelMap": levels,
        "input": super::model_input(model),
        "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
        "contextWindow": model.context_window, "maxTokens": model.max_output_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nan_harness_core::coding_model_profile;

    #[test]
    fn prime_native_catalog_exposes_only_supported_reasoning_controls() {
        for (id, disabled) in [("qwen3.6", true), ("glm5.3", false)] {
            let model = coding_model_profile(id).expect("bundled model");
            let value = json!([model_definition(&model)]);
            assert_eq!(value[0]["reasoning"], true);
            assert_eq!(value[0]["thinkingLevelMap"]["high"], "high");
            assert_eq!(
                value[0]["thinkingLevelMap"]["off"],
                if disabled { json!("none") } else { Value::Null }
            );
        }
        let model = coding_model_profile("mimo-v2.6-flash").unwrap();
        let value = json!([model_definition(&model)]);
        assert_eq!(value[0]["thinkingLevelMap"]["off"], "none");
        assert_eq!(value[0]["thinkingLevelMap"]["high"], "high");

        let mut model = coding_model_profile("qwen3.6").unwrap();
        model.reasoning = ReasoningPolicy::AlwaysOn;
        let value = json!([model_definition(&model)]);
        assert_eq!(value[0]["reasoning"], false);
        assert!(
            value[0]["thinkingLevelMap"]
                .as_object()
                .unwrap()
                .values()
                .all(Value::is_null)
        );
    }
    #[test]
    fn prime_routes_toggle_models_through_provider_level_compatibility() {
        let models = ["qwen3.6", "mimo-v2.6-flash", "deepseek-v4-flash"]
            .map(|id| coding_model_profile(id).unwrap());
        let value = prime_provider_catalog(&models, "https://nan.invalid/v1");
        let rendered = super::super::render_model_catalogs(
            nan_harness_core::launch_plan::PRIME_PROVIDER_CATALOG_PLACEHOLDER,
            "https://nan.invalid/v1",
            "qwen3.6",
            Some(&models),
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&rendered).unwrap(),
            json!(value)
        );
        assert_eq!(value["nan"]["models"].as_array().unwrap().len(), 2);
        assert_eq!(value["nan"]["compat"]["thinkingFormat"], "openai");
        assert_eq!(value["nan-thinking"]["models"][0]["id"], "mimo-v2.6-flash");
        assert_eq!(
            value["nan-thinking"]["compat"]["thinkingFormat"],
            "qwen-chat-template"
        );
        assert_eq!(
            value["nan-thinking"]["compat"]["supportsReasoningEffort"],
            false
        );
    }
}
