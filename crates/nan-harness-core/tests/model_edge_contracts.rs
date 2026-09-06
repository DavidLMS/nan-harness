use nan_harness_core::{
    HarnessKind, ModelAvailability, ModelCatalog, ModelProfile, ProfileSource, QualificationStatus,
    ReasoningEffort, ReasoningPolicy,
};
use std::collections::BTreeSet;

const BUNDLED_QWEN_PROFILE: &str = include_str!("fixtures/model-profile.qwen3.6.json");
const BUNDLED_QWEN_MODEL_ID: &str = "qwen3.6";

#[test]
fn reasoning_policy_is_unknown_only_for_unknown_policy() {
    let policies = [
        (
            ReasoningPolicy::Toggle {
                default_enabled: true,
            },
            false,
        ),
        (
            ReasoningPolicy::Effort {
                supported: [
                    ReasoningEffort::Low,
                    ReasoningEffort::Medium,
                    ReasoningEffort::High,
                ],
                default: ReasoningEffort::Medium,
            },
            false,
        ),
        (ReasoningPolicy::AlwaysOn, false),
        (ReasoningPolicy::Unsupported, false),
        (ReasoningPolicy::Unknown, true),
    ];

    for (policy, expected_is_unknown) in policies {
        assert_eq!(
            policy.is_unknown(),
            expected_is_unknown,
            "unexpected unknown status for {policy:?}"
        );
    }
}

#[test]
fn claude_gateway_model_id_uses_the_public_prefix_and_provider_id() {
    assert_eq!(
        nan_harness_core::claude_gateway_model_id(BUNDLED_QWEN_MODEL_ID),
        "anthropic/nan/qwen3.6"
    );
}

#[test]
fn bundled_known_model_availability_controls_discovery_warning_once() {
    let profile: ModelProfile =
        serde_json::from_str(BUNDLED_QWEN_PROFILE).expect("valid bundled profile fixture");
    assert_eq!(profile.id, BUNDLED_QWEN_MODEL_ID);
    assert_eq!(profile.source, ProfileSource::Bundled);

    let catalog = ModelCatalog::new([profile]);
    let discovered_ids = BTreeSet::from([BUNDLED_QWEN_MODEL_ID.to_owned()]);

    let discovered = catalog.resolve_explicit(
        BUNDLED_QWEN_MODEL_ID,
        HarnessKind::ClaudeCode,
        &discovered_ids,
    );
    assert_eq!(discovered.availability, ModelAvailability::Discovered);
    assert_eq!(discovered.profile_source, ProfileSource::Bundled);
    assert_eq!(discovered.qualification, QualificationStatus::Qualified);
    assert!(discovered.warnings.is_empty());

    let undiscovered = catalog.resolve_explicit(
        BUNDLED_QWEN_MODEL_ID,
        HarnessKind::ClaudeCode,
        &BTreeSet::new(),
    );
    assert_eq!(
        undiscovered.availability,
        ModelAvailability::ExplicitUndiscovered
    );
    assert_eq!(undiscovered.profile_source, ProfileSource::Bundled);
    assert_eq!(undiscovered.qualification, QualificationStatus::Qualified);
    assert_eq!(undiscovered.warnings.len(), 1);
    assert_eq!(
        undiscovered
            .warnings
            .iter()
            .filter(|warning| warning.contains("live discovery"))
            .count(),
        1
    );
}
