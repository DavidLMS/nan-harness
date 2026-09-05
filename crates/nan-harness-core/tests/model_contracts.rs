use nan_harness_core::{
    HarnessKind, ModelAvailability, ModelCatalog, ModelProfile, ProfileSource, QualificationStatus,
    ReasoningEffort, ReasoningHint, ReasoningPolicy, ReasoningSelection, ResolvedModel,
    coding_model_profile, is_valid_provider_model_id,
};
use serde_json::Value;
use std::collections::BTreeSet;

const MODEL_PROFILE: &str = include_str!("fixtures/model-profile.qwen3.6.json");

#[test]
fn model_profiles_preserve_selected_capabilities_and_round_trip() {
    let expected: Value = serde_json::from_str(MODEL_PROFILE).expect("valid profile fixture");
    let profile: ModelProfile = serde_json::from_str(MODEL_PROFILE).expect("typed profile");

    assert_eq!(profile.id, "qwen3.6");
    assert_eq!(profile.limits.context_tokens, Some(262_144));
    assert_eq!(profile.limits.max_output_tokens, Some(65_536));
    assert_eq!(profile.capabilities.input.len(), 2);
    assert!(profile.capabilities.streaming);
    assert!(profile.capabilities.tools);
    assert!(profile.capabilities.reasoning);
    assert_eq!(
        profile
            .qualification
            .for_harness(HarnessKind::ClaudeCode)
            .status,
        QualificationStatus::Qualified
    );
    assert_eq!(
        serde_json::to_value(profile).expect("serialize profile"),
        expected
    );
}

#[test]
fn resolve_model_reports_discovery_and_profile_diagnostics() {
    let profile: ModelProfile = serde_json::from_str(MODEL_PROFILE).expect("valid profile fixture");
    let catalog = ModelCatalog::new([profile]);
    let discovered = BTreeSet::from(["qwen3.6".to_owned(), "new-model".to_owned()]);

    let known = catalog.resolve_explicit("qwen3.6", HarnessKind::ClaudeCode, &discovered);
    assert_eq!(known.availability, ModelAvailability::Discovered);
    assert_eq!(known.profile_source, ProfileSource::Bundled);
    assert_eq!(known.qualification, QualificationStatus::Qualified);
    assert_eq!(known.warnings, Vec::<String>::new());

    let unknown = catalog.resolve_explicit("new-model", HarnessKind::ClaudeCode, &discovered);
    assert_eq!(unknown.resolved_id, "new-model");
    assert_eq!(unknown.availability, ModelAvailability::Discovered);
    assert_eq!(unknown.profile_source, ProfileSource::Generic);
    assert_eq!(unknown.qualification, QualificationStatus::Unknown);
    assert_eq!(unknown.warnings.len(), 1);
    assert!(unknown.warnings[0].contains("conservative defaults"));

    let missing = catalog.resolve_explicit("private-model", HarnessKind::ClaudeCode, &discovered);
    assert_eq!(
        missing.availability,
        ModelAvailability::ExplicitUndiscovered
    );
    assert_eq!(missing.profile_source, ProfileSource::Generic);
    assert_eq!(missing.qualification, QualificationStatus::Unknown);
    assert_eq!(missing.warnings.len(), 2);
    assert!(
        missing
            .warnings
            .iter()
            .any(|warning| warning.contains("conservative defaults"))
    );
    assert!(
        missing
            .warnings
            .iter()
            .any(|warning| warning.contains("not returned by live discovery"))
    );
}

#[test]
fn resolve_model_keeps_provider_catalog_open_without_accepting_non_models() {
    let known = coding_model_profile("qwen3.6").expect("bundled model");
    assert_eq!(known.source, ProfileSource::Bundled);
    assert_eq!(known.context_window, 262_144);

    let future = coding_model_profile("future-text-model").expect("valid provider model");
    assert_eq!(future.source, ProfileSource::Generic);
    assert_eq!(future.reasoning, ReasoningPolicy::Unknown);
    assert_eq!(future.context_window, 262_144);
    assert_eq!(future.max_output_tokens, 32_768);

    assert!(coding_model_profile("whisper").is_none());
    assert!(coding_model_profile(" model-with-leading-space").is_none());
    assert!(coding_model_profile("model\nwith-control").is_none());
}

#[test]
fn provider_model_id_validation_covers_accepted_and_rejected_boundaries() {
    assert!(is_valid_provider_model_id("a"));
    assert!(is_valid_provider_model_id(&"a".repeat(256)));
    assert!(!is_valid_provider_model_id(""));
    assert!(!is_valid_provider_model_id(&"a".repeat(257)));
    assert!(!is_valid_provider_model_id(" model"));
    assert!(!is_valid_provider_model_id("model "));
    assert!(!is_valid_provider_model_id("model\u{7f}"));
}

#[test]
fn resolve_reasoning_preserves_defaults_and_rejects_unsupported_controls() {
    let toggle = ReasoningPolicy::Toggle {
        default_enabled: false,
    };
    assert_eq!(
        toggle.default_selection(),
        ReasoningSelection::Toggle(false)
    );
    assert_eq!(
        toggle.resolve_hint(ReasoningHint::Disabled),
        Some(ReasoningSelection::Toggle(false))
    );
    assert_eq!(
        toggle.resolve_hint(ReasoningHint::High),
        Some(ReasoningSelection::Toggle(true))
    );

    let effort = ReasoningPolicy::Effort {
        supported: [
            ReasoningEffort::Low,
            ReasoningEffort::Medium,
            ReasoningEffort::High,
        ],
        default: ReasoningEffort::Medium,
    };
    assert_eq!(
        effort.default_selection(),
        ReasoningSelection::Effort(ReasoningEffort::Medium)
    );
    assert_eq!(
        effort.resolve_hint(ReasoningHint::Low),
        Some(ReasoningSelection::Effort(ReasoningEffort::Low))
    );
    assert_eq!(
        effort.resolve_hint(ReasoningHint::ExtraHigh),
        Some(ReasoningSelection::Effort(ReasoningEffort::High))
    );
    assert_eq!(effort.resolve_hint(ReasoningHint::Disabled), None);
    assert!(!effort.accepts(ReasoningSelection::Toggle(true)));

    assert_eq!(
        ReasoningPolicy::AlwaysOn.default_selection(),
        ReasoningSelection::Toggle(true)
    );
    assert_eq!(
        ReasoningPolicy::AlwaysOn.resolve_hint(ReasoningHint::Disabled),
        None
    );
    assert_eq!(
        ReasoningPolicy::Unsupported.default_selection(),
        ReasoningSelection::Auto
    );
    assert_eq!(
        ReasoningPolicy::Unknown.resolve_hint(ReasoningHint::Medium),
        Some(ReasoningSelection::Auto)
    );
}

#[test]
fn model_resolution_round_trip_keeps_typed_diagnostics_stable() {
    let profile: ModelProfile = serde_json::from_str(MODEL_PROFILE).expect("valid profile fixture");
    let catalog = ModelCatalog::new([profile]);
    let resolved =
        catalog.resolve_explicit("private-model", HarnessKind::ClaudeCode, &BTreeSet::new());

    let encoded = serde_json::to_value(&resolved).expect("serialize resolved model");
    let decoded: ResolvedModel =
        serde_json::from_value(encoded.clone()).expect("decode resolved model");
    assert_eq!(decoded, resolved);
    assert_eq!(encoded["availability"], "explicit-undiscovered");
    assert_eq!(encoded["profileSource"], "generic");
    assert_eq!(encoded["qualification"], "unknown");
    assert_eq!(encoded["reasoningSelection"], Value::Null);
}
