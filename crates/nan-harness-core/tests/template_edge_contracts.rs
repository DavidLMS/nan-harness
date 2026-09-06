use nan_harness_core::launch_plan::{
    LaunchPlan, LaunchPlanValidator, NAN_SEARCH_BLOCK_BEGIN, NAN_SEARCH_BLOCK_END,
};
use nan_harness_core::{ErrorCategory, PlanError, TransportKind};

const DIRECT_PLAN: &str = include_str!("fixtures/launch-plan.direct.json");

fn direct_plan() -> LaunchPlan {
    serde_json::from_str(DIRECT_PLAN).expect("valid direct plan fixture")
}

#[test]
fn search_end_before_begin_is_rejected_even_when_later_markers_are_balanced() {
    let mut plan = direct_plan();
    plan.temporary_artifacts[0].content_template = Some(format!(
        "{NAN_SEARCH_BLOCK_END} stray {NAN_SEARCH_BLOCK_BEGIN} body {NAN_SEARCH_BLOCK_END}"
    ));

    assert!(matches!(
        LaunchPlanValidator::validate(&plan),
        Err(PlanError::UnsafeTemporaryArtifact { artifact_id, reason })
            if artifact_id == "opencode-config"
                && reason == "contentTemplate contains malformed or nested NaN search blocks"
    ));
}

#[test]
fn nested_search_begin_is_rejected_before_consuming_following_balanced_markers() {
    let mut plan = direct_plan();
    plan.temporary_artifacts[0].content_template = Some(format!(
        "{NAN_SEARCH_BLOCK_BEGIN} outer {NAN_SEARCH_BLOCK_BEGIN} nested {NAN_SEARCH_BLOCK_END} {NAN_SEARCH_BLOCK_BEGIN} later {NAN_SEARCH_BLOCK_END} {NAN_SEARCH_BLOCK_END}"
    ));

    assert!(matches!(
        LaunchPlanValidator::validate(&plan),
        Err(PlanError::UnsafeTemporaryArtifact { artifact_id, reason })
            if artifact_id == "opencode-config"
                && reason == "contentTemplate contains malformed or nested NaN search blocks"
    ));
}

#[test]
fn non_ascii_unknown_artifact_is_a_typed_field_error_without_panicking() {
    let mut plan = direct_plan();
    plan.process.arguments = vec!["--config={artifact:café}".to_owned()];

    let result = std::panic::catch_unwind(|| LaunchPlanValidator::validate(&plan));
    let error = result
        .expect("non-ASCII artifact validation must not panic")
        .expect_err("unknown artifact should be rejected");

    assert_eq!(error.code(), "NH-PLAN-001");
    assert_eq!(error.category(), ErrorCategory::Contract);
    assert!(matches!(
        error,
        PlanError::InvalidField { field, message }
            if field == "process.arguments"
                && message.contains("unknown temporary artifact 'café'")
    ));
}

#[test]
fn transport_kinds_display_their_canonical_wire_values() {
    let cases = [
        (TransportKind::DirectChat, "direct-chat"),
        (TransportKind::AnthropicBridge, "anthropic-bridge"),
        (TransportKind::ResponsesBridge, "responses-bridge"),
        (TransportKind::FxGatewayBridge, "fx-gateway-bridge"),
    ];

    for (kind, expected_wire_value) in cases {
        assert_eq!(kind.to_string(), expected_wire_value);
    }
}
