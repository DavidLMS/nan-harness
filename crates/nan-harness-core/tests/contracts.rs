use nan_harness_core::launch_plan::{LaunchPlanValidator, Transport};
use nan_harness_core::{HarnessKind, LaunchPlan, ModelCatalog, ModelProfile, PlanError};
use serde_json::Value;
use std::collections::BTreeSet;

const BRIDGE_PLAN: &str = include_str!("fixtures/launch-plan.bridge.json");
const DIRECT_PLAN: &str = include_str!("fixtures/launch-plan.direct.json");
const MODEL_PROFILE: &str = include_str!("fixtures/model-profile.qwen3.6.json");

#[test]
fn launch_plan_examples_round_trip_without_schema_drift() {
    for source in [BRIDGE_PLAN, DIRECT_PLAN] {
        let expected: Value = serde_json::from_str(source).expect("fixture should be JSON");
        let plan: LaunchPlan = serde_json::from_str(source).expect("fixture should match Rust");
        let actual = serde_json::to_value(plan).expect("plan should serialize");

        assert_eq!(actual, expected);
    }
}

#[test]
fn model_profile_example_round_trips_without_schema_drift() {
    let expected: Value = serde_json::from_str(MODEL_PROFILE).expect("fixture should be JSON");
    let profile: ModelProfile =
        serde_json::from_str(MODEL_PROFILE).expect("fixture should match Rust");

    assert_eq!(
        serde_json::to_value(profile).expect("profile should serialize"),
        expected
    );
}

#[test]
fn accepted_examples_pass_semantic_validation() {
    for source in [BRIDGE_PLAN, DIRECT_PLAN] {
        let plan: LaunchPlan = serde_json::from_str(source).expect("fixture should match Rust");
        LaunchPlanValidator::validate(&plan).expect("fixture should be semantically valid");
    }
}

#[test]
fn validator_rejects_transport_environment_and_observability_violations() {
    let mut wrong_transport = direct_plan();
    wrong_transport.transport = bridge_plan().transport;
    assert!(matches!(
        LaunchPlanValidator::validate(&wrong_transport),
        Err(PlanError::TransportMismatch { .. })
    ));

    let mut conflicting_environment = direct_plan();
    conflicting_environment
        .environment
        .remove
        .insert("NAN_API_KEY".to_owned());
    assert!(matches!(
        LaunchPlanValidator::validate(&conflicting_environment),
        Err(PlanError::ConflictingEnvironment { .. })
    ));

    let mut payload_capture = direct_plan();
    payload_capture.observability.payload_capture = true;
    assert!(matches!(
        LaunchPlanValidator::validate(&payload_capture),
        Err(PlanError::InvalidField {
            field: "observability.payloadCapture",
            ..
        })
    ));
}

#[test]
fn model_resolution_keeps_explicit_unknown_models_usable() {
    let profile: ModelProfile = serde_json::from_str(MODEL_PROFILE).expect("valid profile");
    let catalog = ModelCatalog::new([profile]);
    let discovered = BTreeSet::from(["qwen3.6".to_owned(), "custom-model".to_owned()]);

    let known = catalog.resolve_explicit("qwen3.6", HarnessKind::ClaudeCode, &discovered);
    assert!(known.warnings.is_empty());

    let unknown = catalog.resolve_explicit("custom-model", HarnessKind::ClaudeCode, &discovered);
    assert_eq!(unknown.resolved_id, "custom-model");
    assert_eq!(unknown.warnings.len(), 1);

    let undiscovered =
        catalog.resolve_explicit("private-model", HarnessKind::ClaudeCode, &discovered);
    assert_eq!(undiscovered.warnings.len(), 2);
}

fn direct_plan() -> LaunchPlan {
    serde_json::from_str(DIRECT_PLAN).expect("valid direct plan fixture")
}

fn bridge_plan() -> LaunchPlan {
    serde_json::from_str(BRIDGE_PLAN).expect("valid bridge plan fixture")
}

#[test]
fn direct_transport_fixture_stays_direct() {
    assert!(matches!(
        direct_plan().transport,
        Transport::DirectChat { .. }
    ));
}
