use super::effort_name;
use nan_harness_core::{CodingModelProfile, model::ReasoningPolicy};
use serde_json::{Value, json};

/// Renders `ZCode`'s versioned personal-provider file from the discovered NaN catalog.
/// The credential is supplied only while materializing a private file.
#[must_use]
pub fn zcode_provider_config(
    models: &[CodingModelProfile],
    base_url: &str,
    credential: &str,
    selected: &str,
) -> Value {
    let rules = models.iter().map(|model| json!({
        "providerId": "nan", "modelId": model.id,
        "config": {"enabled": true, "properties": {
            "contextWindow": model.context_window,
            "inputFormat": {"supportsText": true, "supportsImage": model.image_input, "supportsVideo": false, "supportsAudio": false, "supportsPdf": false},
            "outputFormat": {"supportsText": true},
            "supportsToolCall": true, "supportsJsonSchemaOutput": false,
            "supportsNativeWebSearch": false, "supportsMidConversationSystem": false,
            "requiresMfjsToolSchema": false
        }, "optionSpecs": {
            "maxOutputTokens": {"max": model.max_output_tokens, "map": "{\"max_tokens\": maxOutputTokens}"},
            "reasoningLevel": reasoning_options(model.reasoning)
        }}
    })).collect::<Vec<_>>();
    json!({"schemaVersion": 1, "config": {
        "providerConfigRules": {"providerRules": [{
            "providerId": "nan", "providerName": "NaN", "enabled": true,
            "config": {"group": "standard-personal", "access": {"type": "api-key", "apiKey": credential},
                "api": {"type": "openai-chat-completions", "baseUrl": base_url},
                "personalModelIds": models.iter().map(|model| &model.id).collect::<Vec<_>>()}
        }]},
        "modelConfigRules": {"providerModelRules": rules, "manualProviderModelRules": []},
        "defaultModelSelection": {"providerId": "nan", "modelId": selected, "options": {"reasoningLevel": "auto"}}
    }})
}

fn reasoning_options(policy: ReasoningPolicy) -> Value {
    // ZCode picks the last value for a new selection; auto omits an explicit
    // reasoning parameter and lets the provider retain the shared model default.
    match policy {
        ReasoningPolicy::Toggle { .. } => json!({
            "values": ["disabled", "enabled", "auto"],
            "map": "reasoningLevel == \"auto\" ? {} : {\"chat_template_kwargs\": {\"enable_thinking\": reasoningLevel == \"enabled\"}}"
        }),
        ReasoningPolicy::Effort {
            supported,
            supports_disabled,
            ..
        } => {
            let mut values = supported.iter().map(effort_name).collect::<Vec<_>>();
            if supports_disabled {
                values.insert(0, "disabled");
            }
            values.push("auto");
            json!({"values": values, "map": "reasoningLevel == \"auto\" ? {} : {\"reasoning_effort\": reasoningLevel == \"disabled\" ? \"none\" : reasoningLevel}"})
        }
        ReasoningPolicy::AlwaysOn | ReasoningPolicy::Unsupported | ReasoningPolicy::Unknown => {
            json!({"values": ["auto"], "map": "{}"})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nan_harness_core::coding_model_profile;

    #[test]
    fn zcode_catalog_uses_live_models_capabilities_and_omitted_reasoning_defaults() {
        let models = [
            coding_model_profile("qwen3.6").unwrap(),
            coding_model_profile("deepseek-v4-flash").unwrap(),
            CodingModelProfile::generic("future-model"),
        ];
        let config = zcode_provider_config(
            &models,
            "http://127.0.0.1:1234/v1",
            "{secret-json:nan_api_key}",
            "future-model",
        );
        assert_eq!(config["schemaVersion"], 1);
        assert_eq!(
            config["config"]["providerConfigRules"]["providerRules"][0]["config"]["personalModelIds"],
            json!(["qwen3.6", "deepseek-v4-flash", "future-model"])
        );
        let rules = config["config"]["modelConfigRules"]["providerModelRules"]
            .as_array()
            .unwrap();
        for (rule, model) in rules.iter().zip(&models) {
            assert_eq!(
                rule["config"]["properties"]["contextWindow"],
                model.context_window
            );
            assert_eq!(
                rule["config"]["properties"]["inputFormat"]["supportsImage"],
                model.image_input
            );
            assert_eq!(
                rule["config"]["optionSpecs"]["maxOutputTokens"]["max"],
                model.max_output_tokens
            );
            assert_eq!(
                rule["config"]["optionSpecs"]["reasoningLevel"]["values"]
                    .as_array()
                    .unwrap()
                    .last()
                    .unwrap(),
                "auto"
            );
        }
        assert_eq!(
            rules[2]["config"]["optionSpecs"]["reasoningLevel"]["map"],
            "{}"
        );
        assert_eq!(
            config["config"]["defaultModelSelection"]["options"]["reasoningLevel"],
            "auto"
        );
    }
}
