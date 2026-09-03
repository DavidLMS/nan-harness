use super::{
    GENERIC_CODING_MODEL_DESCRIPTION, GENERIC_CODING_MODEL_MAX_OUTPUT_TOKENS, KNOWN_CODING_MODELS,
    ProfileSource, ReasoningEffort, ReasoningHint, ReasoningParameter, ReasoningPolicy,
    ReasoningSelection, coding_model_profile, coding_models_from_provider_ids, known_coding_model,
};
use std::collections::BTreeSet;

#[test]
fn coding_model_metadata_is_complete_and_uniquely_addressable() {
    let ids = KNOWN_CODING_MODELS
        .iter()
        .map(|model| model.id)
        .collect::<BTreeSet<_>>();

    assert_eq!(ids.len(), KNOWN_CODING_MODELS.len());
    for model in KNOWN_CODING_MODELS {
        assert!(!model.display_name.trim().is_empty());
        assert!(!model.description.trim().is_empty());
        assert!(model.context_window > 0);
        assert!(model.max_output_tokens > 0);
        assert_eq!(known_coding_model(model.id), Some(&model));
    }
}

#[test]
fn live_catalog_enriches_known_models_and_accepts_unknown_text_models() {
    let models = coding_models_from_provider_ids([
        "deepseek-v4-flash-0731".to_owned(),
        "qwen3.6".to_owned(),
        "qwen3.6".to_owned(),
        "glm5.2".to_owned(),
    ]);

    assert_eq!(
        models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        ["qwen3.6", "glm5.2", "deepseek-v4-flash-0731"]
    );
    let provisional = models
        .iter()
        .find(|model| model.id == "deepseek-v4-flash-0731")
        .expect("new model should remain selectable");
    assert_eq!(provisional.source, ProfileSource::Generic);
    assert_eq!(provisional.description, GENERIC_CODING_MODEL_DESCRIPTION);
    assert_eq!(provisional.reasoning, ReasoningPolicy::Unknown);
}

#[test]
fn live_catalog_excludes_only_known_non_coding_models() {
    let models = coding_models_from_provider_ids([
        "whisper".to_owned(),
        "qwen3-embedding".to_owned(),
        "rerank".to_owned(),
        "kokoro".to_owned(),
        "flux-2-klein".to_owned(),
        "minimax-h3".to_owned(),
        "future-text-model".to_owned(),
    ]);

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "future-text-model");
    assert!(coding_model_profile("whisper").is_none());
    assert!(coding_model_profile("minimax-h3").is_none());
}

#[test]
fn bundled_reasoning_policies_are_explicit_model_metadata() {
    assert_eq!(
        known_coding_model("qwen3.6")
            .expect("known model")
            .reasoning,
        ReasoningPolicy::Toggle {
            default_enabled: true
        }
    );
    assert_eq!(
        known_coding_model("gemma4").expect("known model").reasoning,
        ReasoningPolicy::Toggle {
            default_enabled: false
        }
    );
    assert_eq!(
        known_coding_model("deepseek-v4-flash")
            .expect("known model")
            .reasoning,
        ReasoningPolicy::Effort {
            supported: [
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            default: ReasoningEffort::Medium,
        }
    );
    assert_eq!(
        known_coding_model("mimo-v2.5")
            .expect("known model")
            .reasoning,
        ReasoningPolicy::AlwaysOn
    );
    assert_eq!(
        known_coding_model("glm5.2").expect("known model").reasoning,
        ReasoningPolicy::Effort {
            supported: [
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            default: ReasoningEffort::Medium,
        }
    );
    assert!(
        known_coding_model("glm5.2")
            .expect("known model")
            .description
            .contains("reasoning")
    );
    assert_eq!(
        known_coding_model("qwen3.8-flash")
            .expect("known model")
            .reasoning,
        ReasoningPolicy::AlwaysOn
    );
    assert_eq!(
        known_coding_model("glm5.3-flash")
            .expect("known model")
            .reasoning,
        ReasoningPolicy::Effort {
            supported: [
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            default: ReasoningEffort::Medium,
        }
    );
}

#[test]
fn new_nan_models_use_announced_context_and_modalities() {
    let deepseek = known_coding_model("deepseek-v4-flash").expect("DeepSeek V4 Flash profile");
    assert_eq!(deepseek.context_window, 1_000_000);
    assert_eq!(deepseek.max_output_tokens, 262_144);
    assert!(deepseek.image_input);
    assert!(deepseek.description.contains("vision"));

    let qwen = known_coding_model("qwen3.8-flash").expect("Qwen 3.8 profile");
    assert_eq!(qwen.context_window, 1_000_000);
    assert_eq!(
        qwen.max_output_tokens,
        GENERIC_CODING_MODEL_MAX_OUTPUT_TOKENS
    );
    assert!(qwen.image_input);
    assert!(qwen.description.contains("vision"));

    let glm = known_coding_model("glm5.3-flash").expect("GLM 5.3 profile");
    assert_eq!(glm.context_window, 1_000_000);
    assert_eq!(
        glm.max_output_tokens,
        GENERIC_CODING_MODEL_MAX_OUTPUT_TOKENS
    );
    assert!(glm.image_input);
    assert!(glm.description.contains("vision"));
}

#[test]
fn live_catalog_enriches_new_models_without_changing_availability_rules() {
    let models = coding_models_from_provider_ids([
        "qwen3.8-flash".to_owned(),
        "glm5.3-flash".to_owned(),
        "future-nan-model".to_owned(),
    ]);

    assert_eq!(
        models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        ["qwen3.8-flash", "glm5.3-flash", "future-nan-model"]
    );
    assert_eq!(models[0].source, ProfileSource::Bundled);
    assert_eq!(models[1].source, ProfileSource::Bundled);
    assert_eq!(models[2].source, ProfileSource::Generic);
}

#[test]
fn auto_is_distinct_from_an_explicit_reasoning_parameter() {
    assert_eq!(ReasoningSelection::Auto.explicit_parameter(), None);
    assert_eq!(
        ReasoningSelection::Toggle(false).explicit_parameter(),
        Some(ReasoningParameter::Toggle(false))
    );
    assert_eq!(
        ReasoningSelection::Effort(ReasoningEffort::High).explicit_parameter(),
        Some(ReasoningParameter::Effort(ReasoningEffort::High))
    );
}

#[test]
fn reasoning_policy_validates_only_controls_the_model_declares() {
    let effort = known_coding_model("deepseek-v4-flash")
        .expect("known model")
        .reasoning;
    assert!(effort.accepts(ReasoningSelection::Auto));
    assert!(effort.accepts(ReasoningSelection::Effort(ReasoningEffort::Low)));
    assert!(!effort.accepts(ReasoningSelection::Toggle(true)));

    let always_on = ReasoningPolicy::AlwaysOn;
    assert!(always_on.accepts(ReasoningSelection::Toggle(true)));
    assert!(!always_on.accepts(ReasoningSelection::Toggle(false)));
    assert!(!ReasoningPolicy::Unknown.accepts(ReasoningSelection::Toggle(true)));
    assert!(!ReasoningPolicy::Unsupported.accepts(ReasoningSelection::Toggle(true)));
}

#[test]
fn reasoning_hints_resolve_against_model_capabilities() {
    let toggle = ReasoningPolicy::Toggle {
        default_enabled: false,
    };
    assert_eq!(
        toggle.resolve_hint(ReasoningHint::Disabled),
        Some(ReasoningSelection::Toggle(false))
    );
    assert_eq!(
        toggle.resolve_hint(ReasoningHint::Medium),
        Some(ReasoningSelection::Toggle(true))
    );

    let effort = known_coding_model("deepseek-v4-flash")
        .expect("known model")
        .reasoning;
    assert_eq!(effort.resolve_hint(ReasoningHint::Disabled), None);
    assert_eq!(
        effort.resolve_hint(ReasoningHint::Medium),
        Some(ReasoningSelection::Effort(ReasoningEffort::Medium))
    );
    assert_eq!(
        effort.resolve_hint(ReasoningHint::ExtraHigh),
        Some(ReasoningSelection::Effort(ReasoningEffort::High))
    );

    assert_eq!(
        ReasoningPolicy::AlwaysOn.resolve_hint(ReasoningHint::Medium),
        Some(ReasoningSelection::Toggle(true))
    );
    assert_eq!(
        ReasoningPolicy::AlwaysOn.resolve_hint(ReasoningHint::Disabled),
        None
    );
    assert_eq!(
        ReasoningPolicy::Unsupported.resolve_hint(ReasoningHint::Medium),
        Some(ReasoningSelection::Auto)
    );
    assert_eq!(
        ReasoningPolicy::Unknown.resolve_hint(ReasoningHint::Medium),
        Some(ReasoningSelection::Auto)
    );
}

#[test]
fn reasoning_contract_serializes_with_stable_discriminants() {
    assert_eq!(
        serde_json::to_value(ReasoningPolicy::Toggle {
            default_enabled: true,
        })
        .expect("serializable"),
        serde_json::json!({"kind": "toggle", "defaultEnabled": true})
    );
    assert_eq!(
        serde_json::to_value(ReasoningSelection::Auto).expect("serializable"),
        serde_json::json!({"kind": "auto"})
    );
}
