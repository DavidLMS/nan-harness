use nan_harness_core::launch_plan::LaunchId;
use nan_harness_core::{
    HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError, build_validated_plan,
};
use std::fs;

const DIRECT_PLAN: &str = include_str!("fixtures/launch-plan.direct.json");

struct FixtureAdapter;

impl HarnessAdapter for FixtureAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::OpenCode
    }

    fn plan(&self, _context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        Ok(serde_json::from_str(DIRECT_PLAN).expect("valid direct plan fixture"))
    }
}

#[test]
fn planning_has_no_file_system_side_effects() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let sentinel = directory.path().join("sentinel");
    fs::write(&sentinel, "unchanged").expect("sentinel should be written");
    let plan: LaunchPlan = serde_json::from_str(DIRECT_PLAN).expect("valid fixture");
    let context = PlanContext {
        launch_id: LaunchId::new("launch_01exampledirect").expect("valid launch ID"),
        harness: plan.harness.clone(),
        model: plan.model.clone(),
        working_directory: plan.process.working_directory.clone(),
        user_arguments: Vec::new(),
        web_search_policy: plan.web_search_policy,
        observability_format: plan.observability.format,
    };

    let _ = build_validated_plan(&FixtureAdapter, &context).expect("planning should succeed");

    assert_eq!(
        fs::read_to_string(sentinel).expect("sentinel should remain"),
        "unchanged"
    );
    assert_eq!(
        fs::read_dir(directory.path())
            .expect("directory should remain readable")
            .count(),
        1
    );
}

#[test]
fn adapter_kind_must_match_the_requested_harness() {
    let mut plan: LaunchPlan = serde_json::from_str(DIRECT_PLAN).expect("valid fixture");
    plan.harness.kind = HarnessKind::Pi;
    let context = PlanContext {
        launch_id: plan.launch_id.clone(),
        harness: plan.harness,
        model: plan.model,
        working_directory: plan.process.working_directory,
        user_arguments: Vec::new(),
        web_search_policy: plan.web_search_policy,
        observability_format: plan.observability.format,
    };

    assert!(matches!(
        build_validated_plan(&FixtureAdapter, &context),
        Err(PlanError::AdapterMismatch { .. })
    ));
}
