use crate::prepared::catalogs::{
    aider_model_settings, cline_model_catalog, deepseek_model_catalog, goose_model_catalog,
    hermes_model_catalog, kimi_code_model_catalog, openclaw_model_catalog, opencode_model_catalog,
    pi_model_catalog, qwen_code_model_catalog, render_model_catalogs,
};
use nan_harness_core::coding_model_profile;
use nan_harness_core::launch_plan::{
    OPENCODE_MODEL_CATALOG_PLACEHOLDER, PI_MODEL_CATALOG_PLACEHOLDER,
    SELECTED_MODEL_CAPABILITIES_PLACEHOLDER,
};

use super::support::{known_models, model};

#[test]
fn model_catalog_rendering_deduplicates_ids_stably() {
    let models = [model("qwen3.6"), model("qwen3.6"), model("mimo-v2.6-flash")];
    let template = format!(
        r#"{{"opencode":{OPENCODE_MODEL_CATALOG_PLACEHOLDER},"pi":{PI_MODEL_CATALOG_PLACEHOLDER}}}"#
    );
    let rendered = render_model_catalogs(
        &template,
        "https://api.nan.builders/v1",
        "qwen3.6",
        Some(&models),
    )
    .expect("catalogs should render");
    let value: serde_json::Value =
        serde_json::from_str(&rendered).expect("rendered catalogs should be JSON");

    assert_eq!(value["opencode"].as_object().expect("map").len(), 2);
    assert_eq!(value["pi"].as_object().expect("map").len(), 2);
    assert_eq!(
        value["opencode"]
            .as_object()
            .expect("map")
            .keys()
            .collect::<Vec<_>>(),
        &[&"mimo-v2.6-flash".to_owned(), &"qwen3.6".to_owned()]
    );
}

#[test]
fn opencode_reasoning_variants_are_model_aware() {
    let mut models = known_models();
    models.extend([
        coding_model_profile("qwen3.8-flash").expect("known coding model"),
        coding_model_profile("glm5.3-flash").expect("known coding model"),
        coding_model_profile("glm5.3").expect("known coding model"),
    ]);
    let opencode = opencode_model_catalog(&models);
    assert_eq!(opencode["qwen3.6"]["reasoning"], true);
    for id in ["qwen3.6", "gemma4"] {
        assert!(opencode[id].get("defaultVariant").is_none());
        assert_eq!(opencode[id]["variants"]["off"]["reasoningEffort"], "none");
        assert_eq!(opencode[id]["variants"]["max"]["reasoningEffort"], "max");
    }
    assert!(opencode["deepseek-v4-flash"].get("variants").is_none());
    assert!(opencode["glm5.3"]["variants"].get("off").is_none());
    assert_eq!(opencode["glm5.2"]["reasoning"], true);
    assert_eq!(
        opencode["glm5.2"]["variants"]["high"]["reasoningEffort"],
        "high"
    );
    assert_eq!(opencode["qwen3.8-flash"]["reasoning"], true);
    assert!(opencode["qwen3.8-flash"].get("defaultVariant").is_none());
    assert_eq!(opencode["glm5.3-flash"]["reasoning"], true);
    assert_eq!(
        opencode["glm5.3-flash"]["variants"]["high"]["reasoningEffort"],
        "high"
    );
    assert_eq!(opencode["glm5.3"]["reasoning"], true);
    assert_eq!(
        opencode["glm5.3"]["variants"]["high"]["reasoningEffort"],
        "high"
    );
}

#[test]
fn qwen_reasoning_capabilities_are_model_aware() {
    let mut models = known_models();
    models.extend([
        coding_model_profile("qwen3.8-flash").expect("known coding model"),
        coding_model_profile("glm5.3-flash").expect("known coding model"),
        coding_model_profile("glm5.3").expect("known coding model"),
    ]);
    let qwen = qwen_code_model_catalog(&models, "https://nan.invalid/v1");
    let by_id = |id: &str| {
        qwen.as_array()
            .expect("catalog")
            .iter()
            .find(|entry| entry["id"] == id)
            .expect("model")
    };
    for id in ["qwen3.6", "gemma4", "glm5.3", "glm5.3-flash"] {
        assert_eq!(
            by_id(id)["capabilities"]["reasoning"]["profile"],
            "openai-effort"
        );
        assert_eq!(
            by_id(id)["capabilities"]["reasoning"]["efforts"],
            serde_json::json!(["low", "medium", "high", "max"])
        );
        assert_eq!(
            by_id(id)["generationConfig"]["thinkingMandatory"],
            id.starts_with("glm")
        );
        assert!(
            by_id(id)["generationConfig"]["samplingParams"]
                .get("enable_thinking")
                .is_none()
        );
    }
    assert_eq!(
        by_id("qwen3.6")["capabilities"]["reasoning"]["defaultEffort"],
        "high"
    );
    assert!(by_id("deepseek-v4-flash").get("capabilities").is_none());
    assert!(
        by_id("deepseek-v4-flash")["generationConfig"]["samplingParams"]
            .get("reasoning_effort")
            .is_none()
    );
    assert_eq!(
        by_id("deepseek-v4-flash")["generationConfig"]["modalities"]["image"],
        true
    );
    assert_eq!(
        by_id("qwen3.6")["generationConfig"]["samplingParams"]["max_tokens"],
        65_536
    );
    assert_eq!(
        by_id("qwen3.8-flash")["generationConfig"]["contextWindowSize"],
        1_000_000
    );
    assert_eq!(
        by_id("qwen3.8-flash")["generationConfig"]["modalities"]["image"],
        true
    );
    assert!(
        by_id("qwen3.8-flash")["generationConfig"]["samplingParams"]
            .get("enable_thinking")
            .is_none()
    );
    assert_eq!(
        by_id("glm5.3-flash")["generationConfig"]["modalities"]["image"],
        true
    );
    assert_eq!(
        by_id("glm5.3")["generationConfig"]["contextWindowSize"],
        1_000_000
    );
    assert_eq!(
        by_id("glm5.3")["generationConfig"]["modalities"]["image"],
        true
    );
}

#[test]
fn metadata_and_capabilities_do_not_claim_reasoning_for_every_model() {
    let models = known_models();
    let openclaw = openclaw_model_catalog(&models);
    let by_id = |id: &str| {
        openclaw
            .as_array()
            .expect("catalog")
            .iter()
            .find(|entry| entry["id"] == id)
            .expect("model")
    };
    assert_eq!(by_id("mimo-v2.6-flash")["reasoning"], true);
    assert_eq!(by_id("glm5.2")["reasoning"], true);

    let selected = render_model_catalogs(
        SELECTED_MODEL_CAPABILITIES_PLACEHOLDER,
        "https://nan.invalid/v1",
        "glm5.2",
        Some(&models),
    )
    .expect("selected capabilities");
    assert_eq!(selected, "thinking");

    let kimi = kimi_code_model_catalog(&models, "qwen3.6").expect("Kimi catalog");
    assert!(kimi.contains("thinking"));
    let glm_section = kimi
        .split("[models.\"nan/glm5.2\"]")
        .nth(1)
        .expect("glm section");
    assert!(
        glm_section
            .lines()
            .take(8)
            .any(|line| line.contains("thinking"))
    );

    let pi = pi_model_catalog(&models);
    assert_eq!(pi["qwen3.6"]["reasoningPolicy"]["kind"], "effort");
    assert_eq!(pi["glm5.2"]["reasoningPolicy"]["kind"], "effort");

    let cline = cline_model_catalog(&models);
    assert_eq!(cline["qwen3.6"]["reasoningControl"], "metadata-only");
    assert_eq!(cline["glm5.2"]["reasoningPolicy"]["kind"], "effort");

    let goose = goose_model_catalog(&models);
    assert!(
        goose
            .as_array()
            .expect("Goose catalog")
            .iter()
            .all(|entry| {
                entry["reasoning_control"] == "passthrough"
                    && entry.get("reasoning_policy").is_some()
            })
    );

    let deepseek =
        deepseek_model_catalog(&models, "https://nan.invalid/v1").expect("DeepSeek catalog");
    let deepseek: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&deepseek).expect("provider mapping");
    assert_eq!(deepseek["nan-harness-budgeted"]["reasoning"], "high");
    for route in ["nan-harness", "nan-harness-budgeted"] {
        assert_eq!(deepseek[route]["api"], "openai-completions");
        assert_eq!(deepseek[route]["baseURL"], "https://nan.invalid/v1");
        assert_eq!(deepseek[route]["apiKeyEnv"], "NAN_API_KEY");
    }
    let legacy = deepseek["nan-harness"]["models"]
        .as_sequence()
        .expect("legacy models");
    let qwen = legacy
        .iter()
        .find(|model| model["id"] == "qwen3.6")
        .expect("old session route");
    assert_eq!(qwen["reasoningEfforts"], false);
    let glm = legacy
        .iter()
        .find(|model| model["id"] == "glm5.2")
        .expect("GLM route");
    assert_eq!(glm["reasoningEfforts"]["high"], "high");
    assert!(glm["reasoningEfforts"].get("off").is_none());

    let hermes = hermes_model_catalog(&models);
    assert!(
        hermes
            .as_array()
            .expect("Hermes IDs only")
            .iter()
            .all(serde_json::Value::is_string)
    );
}

#[test]
fn aider_declares_reasoning_effort_for_effort_capable_models() {
    let mut models = known_models();
    models.extend([
        coding_model_profile("qwen3.8-flash").expect("known coding model"),
        coding_model_profile("glm5.3-flash").expect("known coding model"),
        coding_model_profile("glm5.3").expect("known coding model"),
    ]);
    let settings = aider_model_settings(&models);
    let by_name = |name: &str| {
        settings
            .as_array()
            .expect("settings")
            .iter()
            .find(|entry| entry["name"] == name)
            .expect("model")
    };
    for model in [
        "openai/qwen3.6",
        "openai/gemma4",
        "openai/glm5.2",
        "openai/glm5.3-flash",
        "openai/glm5.3",
    ] {
        assert_eq!(
            by_name(model)["accepts_settings"],
            serde_json::json!(["reasoning_effort"])
        );
        assert!(by_name(model).get("reasoning_effort").is_none());
    }
    assert!(
        by_name("openai/deepseek-v4-flash")
            .get("accepts_settings")
            .is_none()
    );
    assert!(by_name("openai/qwen3.6").get("reasoning_effort").is_none());
    assert!(
        by_name("openai/mimo-v2.6-flash")
            .get("reasoning_effort")
            .is_none()
    );
}
