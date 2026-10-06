use nan_harness_core::{CodingModelProfile, ReasoningPolicy};
use serde_json::{Value, json};

/// The separate route supplies DSH's provider-wide default only for models with an off selector.
#[must_use]
pub fn deepseek_provider_for(model: &CodingModelProfile) -> &'static str {
    if matches!(
        model.reasoning,
        ReasoningPolicy::Effort {
            supports_disabled: true,
            ..
        }
    ) {
        "nan-harness-budgeted"
    } else {
        "nan-harness"
    }
}

/// Native DSH providers. Old model/route pairs remain valid for existing sessions.
#[must_use]
pub fn deepseek_provider_catalog(models: &[CodingModelProfile], base_url: &str) -> Value {
    let mut legacy = Vec::new();
    let mut budgeted = Vec::new();
    for model in models {
        let mut entry = json!({
            "id": model.id, "name": model.display_name,
            "contextWindow": model.context_window, "maxTokens": model.max_output_tokens,
            "input": super::model_input(model),
            "reasoningEfforts": false,
        });
        if let ReasoningPolicy::Effort {
            supported,
            supports_disabled,
            ..
        } = model.reasoning
        {
            let mut efforts = serde_json::Map::new();
            for effort in supported {
                let name = super::effort_name(effort);
                efforts.insert(name.to_owned(), json!(name));
            }
            if supports_disabled {
                efforts.insert("off".to_owned(), json!("none"));
                let mut selected = entry.clone();
                selected["reasoningEfforts"] = Value::Object(efforts);
                budgeted.push(selected);
                entry["name"] = json!(format!("{} (existing sessions)", model.display_name));
            } else {
                entry["reasoningEfforts"] = Value::Object(efforts);
            }
        }
        legacy.push(entry);
    }
    let provider = |entries: Vec<Value>| {
        json!({
            "displayName": "NaN", "apiKeyEnv": "NAN_API_KEY", "api": "openai-completions",
            "baseURL": base_url, "compat": {"maxTokensField": "max_tokens"}, "models": entries,
        })
    };
    let mut providers = json!({"nan-harness": provider(legacy)});
    if !budgeted.is_empty() {
        let mut selected = provider(budgeted);
        selected["displayName"] = json!("NaN · reasoning controls");
        // With off:none, DSH otherwise selects none when the user has made no choice.
        selected["reasoning"] = json!("high");
        providers["nan-harness-budgeted"] = selected;
    }
    providers
}
