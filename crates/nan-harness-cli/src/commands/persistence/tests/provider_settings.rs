use super::super::{deepseek_provider_settings, qwen_code_provider};
use jsonc_parser::cst::CstRootNode;
use nan_harness_core::coding_models_from_provider_ids;

#[test]
fn qwen_reasoning_settings_use_native_capabilities_and_explicit_defaults() {
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

    assert_eq!(
        by_id("qwen3.6")["capabilities"]["reasoning"]["defaultEffort"],
        "high"
    );
    assert_eq!(
        by_id("glm5.2")["generationConfig"]["thinkingMandatory"],
        true
    );
    assert_eq!(
        by_id("qwen3.6")["generationConfig"]["thinkingMandatory"],
        false
    );
    // No fixed override may mask the native effort selector.
    for id in [
        "qwen3.6",
        "deepseek-v4-flash",
        "glm5.2",
        "future-stale-model",
    ] {
        assert!(
            by_id(id)["generationConfig"].get("reasoning").is_none(),
            "{id} must not force a reasoning toggle"
        );
    }
}

#[test]
fn deepseek_preserves_old_routes_and_exposes_budgeted_controls_separately() {
    let models = coding_models_from_provider_ids(
        ["qwen3.6", "gemma4", "deepseek-v4-flash", "glm5.3-flash"]
            .into_iter()
            .map(str::to_owned),
    );
    let settings = deepseek_provider_settings(&models, "https://api.nan.test/v1").unwrap();
    let patch: serde_yaml_ng::Value = serde_yaml_ng::from_str(&settings).unwrap();
    assert_eq!(patch[0]["config"]["provider"], "nan-harness-budgeted");
    let providers = &patch[1]["config"]["providers"];
    assert_eq!(providers["nan-harness-budgeted"]["reasoning"], "high");
    assert_eq!(
        providers["nan-harness"]["compat"]["maxTokensField"],
        "max_tokens"
    );
    let budgeted = providers["nan-harness-budgeted"]["models"]
        .as_sequence()
        .unwrap();
    assert_eq!(budgeted.len(), 2);
    for model in budgeted {
        assert_eq!(model["reasoningEfforts"]["off"], "none");
        assert_eq!(model["reasoningEfforts"]["max"], "max");
    }
    let original = providers["nan-harness"]["models"].as_sequence().unwrap();
    assert_eq!(original.len(), 4);
    for model in original {
        if model["id"] == "glm5.3-flash" {
            assert_eq!(model["reasoningEfforts"]["max"], "max");
            assert!(model["reasoningEfforts"].get("off").is_none());
        } else {
            assert_eq!(model["reasoningEfforts"], false);
        }
    }
}
