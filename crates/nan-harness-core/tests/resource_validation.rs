#[path = "resource_validation/artifacts.rs"]
mod artifacts;
#[path = "resource_validation/launch_scoped.rs"]
mod launch_scoped;
#[path = "resource_validation/overlays.rs"]
mod overlays;

use nan_harness_core::{LaunchPlan, LaunchPlanValidator, PlanError};

const DIRECT_PLAN: &str = include_str!("fixtures/launch-plan.direct.json");

fn base_plan() -> LaunchPlan {
    serde_json::from_str(DIRECT_PLAN).expect("valid direct plan fixture")
}

fn validate(plan: &LaunchPlan) -> Result<(), PlanError> {
    LaunchPlanValidator::validate(plan)
}

fn assert_unsafe(plan: &LaunchPlan) {
    assert!(matches!(
        validate(plan),
        Err(PlanError::UnsafeTemporaryArtifact { .. })
    ));
}

fn assert_invalid(plan: &LaunchPlan, field: &'static str) {
    assert!(matches!(
        validate(plan),
        Err(PlanError::InvalidField { field: actual, .. }) if actual == field
    ));
}
