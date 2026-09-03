use super::super::{deepseek_provider_settings, qwen_code_provider};
use jsonc_parser::cst::CstRootNode;
use nan_harness_core::coding_models_from_provider_ids;

#[test]
fn qwen_reasoning_settings_are_model_aware_without_freezing_provider_defaults() {
    let models = coding_models_from_provider_ids(
        [
            "qwen3.6",
            "deepseek-v4-flash",
            "glm5.2",
            "future-stale-model",
        ]
        .map(str::to_owned),
    );
    let root =
        CstRootNode::parse("[]", &jsonc_parser::ParseOptions::default()).expect("valid JSON root");
    root.set_value(qwen_code_provider(&models, "https://api.nan.test/v1"));
    let value = root.to_serde_value().expect("provider should serialize");
    let entries = value
        .as_array()
        .expect("provider catalog should be an array");
    let by_id = |id: &str| {
        entries
            .iter()
            .find(|entry| entry["id"] == id)
            .expect("requested model should be present")
    };

    // GLM-5.2 supports reasoning effort, so it must use provider passthrough
    // instead of freezing reasoning off when the user has not chosen explicitly.
    for id in [
        "qwen3.6",
        "deepseek-v4-flash",
        "glm5.2",
        "future-stale-model",
    ] {
        assert!(
            by_id(id)["generationConfig"].get("reasoning").is_none(),
            "{id} must use provider passthrough until the user makes an explicit choice"
        );
    }
}

#[test]
fn deepseek_serializes_reasoning_capabilities_without_serializing_defaults() {
    let models = coding_models_from_provider_ids(
        [
            "qwen3.6",
            "deepseek-v4-flash",
            "glm5.2",
            "future-stale-model",
        ]
        .map(str::to_owned),
    );
    let settings = deepseek_provider_settings(&models, "https://api.nan.test/v1")
        .expect("DeepSeek settings should serialize");

    let qwen = settings
        .split("        - id: \"qwen3.6\"")
        .nth(1)
        .expect("Qwen block")
        .split("        - id:")
        .next()
        .expect("bounded Qwen block");
    assert!(qwen.contains("reasoning: true"));
    assert!(qwen.contains("supportsReasoningEffort: false"));

    let effort = settings
        .split("        - id: \"deepseek-v4-flash\"")
        .nth(1)
        .expect("effort block")
        .split("        - id:")
        .next()
        .expect("bounded effort block");
    assert!(effort.contains("reasoning: true"));
    assert!(effort.contains("supportsReasoningEffort: true"));

    // GLM-5.2 supports reasoning effort, so it must not freeze reasoning off.
    let glm = settings
        .split("        - id: \"glm5.2\"")
        .nth(1)
        .expect("GLM block")
        .split("        - id:")
        .next()
        .expect("bounded GLM block");
    assert!(glm.contains("reasoning: true"));
    assert!(glm.contains("supportsReasoningEffort: true"));

    let stale = settings
        .split("        - id: \"future-stale-model\"")
        .nth(1)
        .expect("fallback block")
        .split("        - id:")
        .next()
        .expect("bounded fallback block");
    assert!(stale.contains("reasoning: false"));
    assert!(stale.contains("supportsReasoningEffort: false"));
    assert!(!settings.contains("reasoningEffort:"));
    assert!(!settings.contains("defaultEffort:"));
}
