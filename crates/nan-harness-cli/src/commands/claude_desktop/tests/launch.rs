use super::super::{
    BridgeActivity, ClaudeAutoModeReviewStage, ClaudeDesktopArgs, DesktopHarnessKind,
    DesktopTransport, WebSearchPolicy, dry_run_plan, launch_message, render_bridge_activity,
};
use std::path::PathBuf;

#[test]
fn dry_run_plan_preserves_model_executable_diagnostics_and_search_policy() {
    let arguments = ClaudeDesktopArgs {
        model: Some("qwen3.6".to_owned()),
        provider_base_url: None,
        executable: Some(PathBuf::from("/tmp/claude")),
        allow_unsupported: false,
        allow_untested: false,
        search: crate::app::WebSearchArgs {
            no_search: false,
            force_search: true,
        },
        dry_run: true,
        show_auto: true,
        restore: false,
    };

    let plan = dry_run_plan(&arguments);

    assert_eq!(plan.harness, DesktopHarnessKind::Claude);
    assert_eq!(plan.transport, DesktopTransport::AnthropicBridge);
    assert_eq!(plan.executable, arguments.executable);
    assert_eq!(plan.selected_model, arguments.model);
    assert_eq!(plan.web_search_policy, WebSearchPolicy::Force);
    assert!(plan.private_diagnostics);
}

#[test]
fn auto_mode_activity_renders_the_provider_request() {
    let message = render_bridge_activity(&BridgeActivity::ClaudeAutoModeReview {
        review_id: 7,
        stage: ClaudeAutoModeReviewStage::Initial,
        model_id: "qwen3.6".to_owned(),
        request: nan_harness_runtime::ClaudeAutoModeTracePayload::new(
            r#"{"model":"qwen3.6","temperature":0}"#,
        ),
    });

    assert_eq!(
        message,
        concat!(
            "[Auto #7] Claude requested a permission review (stage 1, classifier qwen3.6).\n",
            "[Auto #7] NaN request:\n",
            "{\n  \"model\": \"qwen3.6\",\n  \"temperature\": 0\n}"
        )
    );
}

#[test]
fn auto_mode_response_pretty_prints_json_and_preserves_non_json_bodies() {
    let response = render_bridge_activity(&BridgeActivity::ClaudeAutoModeReviewResponse {
        review_id: 7,
        status: 200,
        response: nan_harness_runtime::ClaudeAutoModeTracePayload::new(
            r#"{"choices":[{"message":{"content":"reviewed"}}]}"#,
        ),
    });
    assert!(response.contains("[Auto #7] NaN response (HTTP 200):"));
    assert!(response.contains("\"content\": \"reviewed\""));

    let plain_text = "provider response body\n";
    let response = render_bridge_activity(&BridgeActivity::ClaudeAutoModeReviewResponse {
        review_id: 8,
        status: 200,
        response: nan_harness_runtime::ClaudeAutoModeTracePayload::new(plain_text),
    });
    assert!(response.ends_with(plain_text));
}

#[test]
fn auto_mode_failure_is_correlated_without_transport_details() {
    let message = render_bridge_activity(&BridgeActivity::ClaudeAutoModeReviewFailed {
        review_id: 9,
        error_code: "NH-BRIDGE-103",
    });

    assert_eq!(
        message,
        "[Auto #9] NaN request failed before a response was received (NH-BRIDGE-103)."
    );
}

#[test]
fn launch_message_mentions_auto_only_when_tracing_is_enabled() {
    assert_eq!(
        launch_message(false),
        "Claude Desktop launched through NaN."
    );
    assert!(!launch_message(false).contains("Auto"));
    assert!(launch_message(true).contains("Auto traces will appear here"));
    assert!(launch_message(true).contains("private data"));
}
