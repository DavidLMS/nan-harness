use nan_harness_adapters::FxAdapter;
use nan_harness_core::launch_plan::{LaunchId, ObservabilityFormat, Transport};
use nan_harness_core::{
    DetectedHarness, HarnessAdapter, HarnessKind, LaunchPlanValidator, ModelAvailability,
    PlanContext, ProfileSource, QualificationStatus, ResolvedModel, VersionStatus,
};

#[test]
fn fx_uses_a_loopback_gateway_bridge_and_process_overrides() {
    let plan = FxAdapter
        .plan(&context(Vec::new()))
        .expect("fx plan should build");
    LaunchPlanValidator::validate(&plan).expect("fx plan should validate");
    assert!(matches!(plan.transport, Transport::FxGatewayBridge { .. }));
    assert_eq!(plan.environment.public["FX_MODEL"], "qwen3.6");
    assert_eq!(plan.environment.public["FX_SKIP_ONBOARDING"], "1");
    assert!(plan.environment.public["FX_GATEWAY_BASE_URL"].contains("bridge_base_url"));
    assert!(plan.environment.public["FX_GATEWAY_CHAT_URL"].contains("bridge_chat_url"));
    assert!(plan.environment.secrets.contains_key("AI_GATEWAY_API_KEY"));
}

#[test]
fn fx_rejects_model_arguments_that_bypass_nan_selection() {
    let error = FxAdapter
        .plan(&context(vec!["--model".to_owned(), "other".to_owned()]))
        .expect_err("fx model routing must remain controlled by NaN");
    assert!(
        error
            .to_string()
            .contains("conflicts with NaN Harness routing")
    );
}

fn context(arguments: Vec<String>) -> PlanContext {
    PlanContext {
        launch_id: LaunchId::new("launch_01fxadaptertest").expect("valid launch ID"),
        harness: DetectedHarness {
            kind: HarnessKind::Fx,
            executable: "/tmp/fx".to_owned(),
            detected_version: "0.0.3".to_owned(),
            version_status: VersionStatus::Tested,
        },
        model: ResolvedModel {
            requested_id: "qwen3.6".to_owned(),
            resolved_id: "qwen3.6".to_owned(),
            availability: ModelAvailability::Discovered,
            profile_source: ProfileSource::Bundled,
            qualification: QualificationStatus::Qualified,
            warnings: Vec::new(),
        },
        working_directory: "/tmp".to_owned(),
        user_arguments: arguments,
        observability_format: ObservabilityFormat::Human,
    }
}
