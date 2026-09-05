use crate::prepared::catalogs::render_model_catalogs;
use nan_harness_core::coding_model_profile;
use nan_harness_core::launch_plan::{
    CLAUDE_MODEL_PICKER_PLACEHOLDER, CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER,
};
use std::collections::BTreeSet;

use super::support::{known_models, model};

fn claude_settings_template() -> String {
    claude_settings_template_for("anthropic/nan/qwen3.6")
}

fn claude_settings_template_for(model: &str) -> String {
    serde_json::json!({
        "availableModels": "{runtime:claude_available_models}",
        "model": model,
        "env": {
            "ANTHROPIC_MODEL": model,
            CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER: ""
        }
    })
    .to_string()
}

fn claude_model_picker_settings_template_for(model: &str) -> String {
    serde_json::json!({
        "availableModels": "{runtime:claude_available_models}",
        "model": model,
        "modelPicker": CLAUDE_MODEL_PICKER_PLACEHOLDER,
        "env": {
            "ANTHROPIC_MODEL": model,
            CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER: ""
        }
    })
    .to_string()
}

#[test]
fn claude_picker_slots_come_from_the_discovered_catalog() {
    let models = [
        coding_model_profile("qwen3.6").expect("known coding model"),
        coding_model_profile("mimo-v2.5").expect("known coding model"),
    ];
    let rendered = render_model_catalogs(
        &claude_settings_template(),
        "https://nan.invalid/v1",
        "qwen3.6",
        Some(&models),
    )
    .expect("Claude settings should render");
    let settings: serde_json::Value =
        serde_json::from_str(&rendered).expect("rendered settings should be valid JSON");
    let environment = settings["env"]
        .as_object()
        .expect("settings should keep an env object");

    assert!(!environment.contains_key(CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER));
    assert_eq!(environment["ANTHROPIC_MODEL"], "anthropic/nan/qwen3.6");
    assert_eq!(
        environment["ANTHROPIC_DEFAULT_OPUS_MODEL"],
        "anthropic/nan/qwen3.6"
    );
    assert_eq!(
        environment["ANTHROPIC_DEFAULT_OPUS_MODEL_NAME"],
        "NaN · Qwen 3.6"
    );
    assert_eq!(
        environment["ANTHROPIC_DEFAULT_SONNET_MODEL"],
        "anthropic/nan/mimo-v2.5"
    );
    assert!(
        !environment.contains_key("ANTHROPIC_DEFAULT_HAIKU_MODEL"),
        "slots without a discovered model must stay unset"
    );
    assert!(!environment.contains_key("ANTHROPIC_CUSTOM_MODEL_OPTION"));
    assert!(
        !rendered.contains("deepseek"),
        "a model missing from discovery must never reach the picker"
    );
}

#[test]
fn claude_picker_puts_the_selected_model_first() {
    let models = known_models();
    let rendered = render_model_catalogs(
        &claude_settings_template(),
        "https://nan.invalid/v1",
        "gemma4",
        Some(&models),
    )
    .expect("Claude settings should render");
    let settings: serde_json::Value =
        serde_json::from_str(&rendered).expect("rendered settings should be valid JSON");
    let environment = &settings["env"];

    assert_eq!(
        environment["ANTHROPIC_DEFAULT_OPUS_MODEL"],
        "anthropic/nan/gemma4"
    );
    let slots = [
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_CUSTOM_MODEL_OPTION",
    ]
    .map(|slot| environment[slot].as_str().expect("slot should be filled"));
    assert_eq!(
        slots.iter().collect::<BTreeSet<_>>().len(),
        slots.len(),
        "picker slots must not repeat a model"
    );
}

#[test]
fn claude_curated_picker_prioritizes_glm_over_gemma() {
    let rendered = render_model_catalogs(
        &claude_settings_template(),
        "https://nan.invalid/v1",
        "qwen3.6",
        Some(&known_models()),
    )
    .expect("Claude settings should render");
    let settings: serde_json::Value =
        serde_json::from_str(&rendered).expect("rendered settings should be valid JSON");
    let environment = &settings["env"];

    assert_eq!(
        environment["ANTHROPIC_CUSTOM_MODEL_OPTION"],
        "anthropic/nan/glm5.2"
    );
    assert_eq!(
        environment["ANTHROPIC_CUSTOM_MODEL_OPTION_NAME"],
        "NaN · GLM 5.2"
    );
    assert!(
        !environment
            .as_object()
            .expect("environment object")
            .values()
            .any(|value| value.as_str() == Some("anthropic/nan/gemma4")),
        "Gemma must yield the fourth curated slot to GLM"
    );
}

#[test]
fn claude_gateway_mode_preserves_qwen_auto_alias() {
    let mut models = known_models();
    models.push(model("future-model"));
    let rendered = render_model_catalogs(
        &claude_settings_template_for("opus"),
        "https://nan.invalid/v1",
        "qwen3.6",
        Some(&models),
    )
    .expect("Claude settings should render");
    let settings: serde_json::Value =
        serde_json::from_str(&rendered).expect("rendered settings should be valid JSON");
    let environment = settings["env"]
        .as_object()
        .expect("settings should keep an env object");

    assert_eq!(settings["model"], "opus");
    assert_eq!(environment["ANTHROPIC_MODEL"], "opus");
    assert_eq!(
        environment["ANTHROPIC_DEFAULT_OPUS_MODEL"],
        "anthropic/nan/qwen3.6"
    );
    assert_eq!(
        environment["ANTHROPIC_DEFAULT_OPUS_MODEL_NAME"],
        "NaN · Qwen 3.6"
    );
    for absent in [
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_CUSTOM_MODEL_OPTION",
    ] {
        assert!(
            !environment.contains_key(absent),
            "gateway mode must not consume the {absent} presentation slot"
        );
    }
    assert!(!environment.contains_key(CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER));
}

#[test]
fn claude_model_picker_exposes_standard_and_eligible_1m_variants() {
    let models = [
        coding_model_profile("qwen3.6").expect("known coding model"),
        coding_model_profile("deepseek-v4-flash").expect("known coding model"),
        coding_model_profile("glm5.2").expect("known coding model"),
        model("future-model"),
    ];
    let rendered = render_model_catalogs(
        &claude_model_picker_settings_template_for("anthropic/nan/deepseek-v4-flash"),
        "https://nan.invalid/v1",
        "deepseek-v4-flash",
        Some(&models),
    )
    .expect("Claude modelPicker settings should render");
    let settings: serde_json::Value =
        serde_json::from_str(&rendered).expect("rendered settings should be valid JSON");

    assert_eq!(settings["model"], "anthropic/nan/deepseek-v4-flash");
    assert_eq!(settings["modelPicker"]["replaceBuiltInOptions"], true);
    assert_eq!(
        settings["modelPicker"]["options"],
        serde_json::json!([
            {
                "model": "opus",
                "label": "NaN · Qwen 3.6",
                "description": "Standard context · 256K"
            },
            {
                "model": "anthropic/nan/deepseek-v4-flash",
                "label": "NaN · DeepSeek V4 Flash",
                "description": "Standard context · 256K"
            },
            {
                "model": "anthropic/nan/deepseek-v4-flash[1m]",
                "label": "NaN · DeepSeek V4 Flash (1M)",
                "description": "Extended context · 1M"
            },
            {
                "model": "anthropic/nan/glm5.2",
                "label": "NaN · GLM 5.2",
                "description": "Standard context · 256K"
            },
            {
                "model": "anthropic/nan/future-model",
                "label": "NaN · future-model",
                "description": "Standard context · 256K"
            }
        ])
    );
    let environment = settings["env"].as_object().expect("environment object");
    assert_eq!(
        environment["ANTHROPIC_DEFAULT_OPUS_MODEL"],
        "anthropic/nan/qwen3.6"
    );
    assert!(!environment.contains_key("ANTHROPIC_DEFAULT_SONNET_MODEL"));
    assert!(!rendered.contains(CLAUDE_MODEL_PICKER_PLACEHOLDER));
    assert!(!rendered.contains(CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER));
}

#[test]
fn new_nan_models_keep_claude_in_gateway_discovery_mode() {
    let models = [
        coding_model_profile("qwen3.6").expect("known coding model"),
        coding_model_profile("qwen3.8-flash").expect("known coding model"),
        coding_model_profile("glm5.3-flash").expect("known coding model"),
        coding_model_profile("glm5.3").expect("known coding model"),
    ];
    let rendered = render_model_catalogs(
        &claude_settings_template(),
        "https://nan.invalid/v1",
        "qwen3.6",
        Some(&models),
    )
    .expect("Claude settings should render");
    let settings: serde_json::Value =
        serde_json::from_str(&rendered).expect("rendered settings should be valid JSON");
    let environment = settings["env"].as_object().expect("environment object");

    assert_eq!(
        environment["ANTHROPIC_DEFAULT_OPUS_MODEL"],
        "anthropic/nan/qwen3.6"
    );
    for absent in [
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_CUSTOM_MODEL_OPTION",
    ] {
        assert!(!environment.contains_key(absent));
    }
    assert!(!rendered.contains("qwen3.8-flash"));
    assert!(!rendered.contains("glm5.3-flash"));
}
