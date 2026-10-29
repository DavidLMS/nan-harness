use nan_harness_core::{CodingModelProfile, model::ReasoningPolicy};
use serde_json::{Value, json};

use super::{effort_name, model_input};

/// Render the V2 catalog without changing the V1 schema shared with `MiMo`.
#[must_use]
pub fn opencode_v2_model_catalog(models: &[CodingModelProfile]) -> Value {
    Value::Object(models.iter().map(|model| {
        let mut entry = json!({
            "name": model.display_name,
            "limit": {"context": model.context_window, "output": model.max_output_tokens},
            "capabilities": {"tools": true, "input": model_input(model), "output": ["text"]},
        });
        let variants = match model.reasoning {
            ReasoningPolicy::Effort { supported, supports_disabled, .. } => {
                let mut variants: Vec<Value> = supported.into_iter().map(|effort| {
                    json!({"id": effort_name(effort), "settings": {"reasoningEffort": effort_name(effort)}})
                }).collect();
                if supports_disabled {
                    variants.push(json!({"id": "off", "settings": {"reasoningEffort": "none"}}));
                }
                variants
            }
            ReasoningPolicy::Toggle { .. } => vec![
                json!({"id": "thinking", "body": {"chat_template_kwargs": {"enable_thinking": true}}}),
                json!({"id": "no-thinking", "body": {"chat_template_kwargs": {"enable_thinking": false}}}),
            ],
            _ => Vec::new(),
        };
        if !variants.is_empty() { entry["variants"] = json!(variants); }
        (model.id.clone(), entry)
    }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_on_models_have_no_adjustable_variants_in_either_catalog() {
        let models = ["qwen3.8-flash", "deepseek-v4-flash"]
            .map(|id| nan_harness_core::coding_model_profile(id).unwrap());
        let v1 = super::super::opencode_model_catalog(&models);
        let v2 = opencode_v2_model_catalog(&models);
        for model in &models {
            assert!(v1[&model.id].get("variants").is_none());
            assert!(v2[&model.id].get("variants").is_none());
        }
    }

    #[test]
    fn v2_catalog_preserves_capabilities_and_native_variant_shapes() {
        let mut models = ["qwen3.6", "deepseek-v4-flash"]
            .map(|id| nan_harness_core::coding_model_profile(id).unwrap());
        models[0].reasoning = ReasoningPolicy::Toggle {
            default_enabled: true,
        };
        models[1].reasoning = ReasoningPolicy::Effort {
            supported: nan_harness_core::model::SupportedReasoningEfforts::ALL,
            default: nan_harness_core::model::ReasoningEffort::High,
            supports_disabled: true,
        };
        let catalog = opencode_v2_model_catalog(&models);
        for model in &models {
            assert_eq!(catalog[&model.id]["limit"]["context"], model.context_window);
            assert_eq!(catalog[&model.id]["capabilities"]["tools"], true);
            assert!(catalog[&model.id]["variants"].is_array());
        }
        let variants = catalog["deepseek-v4-flash"]["variants"].as_array().unwrap();
        assert!(variants.iter().any(
            |variant| variant["id"] == "max" && variant["settings"]["reasoningEffort"] == "max"
        ));
        let variants = catalog["qwen3.6"]["variants"].as_array().unwrap();
        assert!(variants.iter().any(|variant| variant["id"] == "no-thinking"
            && variant["body"]["chat_template_kwargs"]["enable_thinking"] == false));
    }

    #[test]
    fn v2_placeholder_renders_in_inline_configuration() {
        let profile = nan_harness_core::coding_model_profile("qwen3.6").unwrap();
        let rendered = super::super::render_model_catalogs(
            &format!(
                r#"{{"models":"{}"}}"#,
                nan_harness_core::launch_plan::OPENCODE_V2_MODEL_CATALOG_PLACEHOLDER
            ),
            "https://nan.invalid/v1",
            "qwen3.6",
            Some(std::slice::from_ref(&profile)),
        )
        .unwrap();
        let config: Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(config["models"]["qwen3.6"]["name"], profile.display_name);
    }
}
