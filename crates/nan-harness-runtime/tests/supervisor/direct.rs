use super::support::{DIRECT_PLAN, assert_removed, execute_shell, test_config};
use nan_harness_core::LaunchPlan;
use nan_harness_core::launch_plan::{PROVIDER_BASE_URL_PLACEHOLDER, TerminalMode, Transport};
use nan_harness_runtime::{CancellationToken, ExecutionOutcome, Supervisor};

#[tokio::test]
async fn supervisor_resolves_provider_urls_in_direct_overlays() {
    let working_directory = tempfile::tempdir().expect("working directory should exist");
    let mut plan: LaunchPlan = serde_json::from_str(DIRECT_PLAN).expect("valid direct fixture");
    "/bin/sh".clone_into(&mut plan.harness.executable);
    let Transport::DirectChat { base_url, .. } = &mut plan.transport else {
        panic!("fixture should use direct chat");
    };
    PROVIDER_BASE_URL_PLACEHOLDER.clone_into(base_url);
    plan.environment.public.insert(
        "NAN_HARNESS_PROVIDER_BASE_URL".to_owned(),
        PROVIDER_BASE_URL_PLACEHOLDER.to_owned(),
    );
    plan.temporary_artifacts[0].content_template = Some(format!(
        "{{\"baseURL\":\"{PROVIDER_BASE_URL_PLACEHOLDER}\"}}"
    ));
    plan.process.arguments = vec![
        "-c".to_owned(),
        concat!(
            "printf '%s\\n' \"$NAN_HARNESS_PROVIDER_BASE_URL\" | ",
            "grep -Eq '^http://127\\.0\\.0\\.1:[0-9]+/v1$' && ",
            "test \"$NAN_HARNESS_PROVIDER_BASE_URL\" != \"${NAN_HARNESS_PROVIDER_BASE_URL%/v1}\" && ",
            "grep -Fq \"$NAN_HARNESS_PROVIDER_BASE_URL\" \"$1\""
        )
        .to_owned(),
        "nan-harness-test".to_owned(),
        "{artifact:opencode-config}".to_owned(),
    ];
    plan.process.working_directory = working_directory.path().to_string_lossy().into_owned();
    plan.process.terminal = TerminalMode::Captured;
    let report = Supervisor::new()
        .execute(&plan, &test_config(), &CancellationToken::new())
        .await
        .expect("direct launch should complete");

    assert_eq!(report.outcome, ExecutionOutcome::Succeeded);
    assert_removed(report.temporary_root);
}

#[tokio::test]
async fn supervisor_gives_direct_children_only_a_launch_scoped_session_token() {
    let report = execute_shell(
        "test \"${#NAN_API_KEY}\" -eq 64 && test \"$NAN_API_KEY\" != test-key",
        true,
        None,
        None,
        None,
    )
    .await;

    assert_eq!(report.outcome, ExecutionOutcome::Succeeded);
    assert_removed(report.temporary_root);
}

#[tokio::test]
async fn supervisor_can_run_direct_chat_without_the_gateway() {
    let working_directory = tempfile::tempdir().expect("working directory should exist");
    let mut plan: LaunchPlan = serde_json::from_str(DIRECT_PLAN).expect("valid direct fixture");
    "/bin/sh".clone_into(&mut plan.harness.executable);
    plan.environment.public.insert(
        "NAN_HARNESS_PROVIDER_BASE_URL".to_owned(),
        PROVIDER_BASE_URL_PLACEHOLDER.to_owned(),
    );
    plan.process.arguments = vec![
        "-c".to_owned(),
        concat!(
            "test \"$NAN_API_KEY\" = test-key && ",
            "test \"$NAN_HARNESS_PROVIDER_BASE_URL\" = http://127.0.0.1:9/v1"
        )
        .to_owned(),
    ];
    plan.process.working_directory = working_directory.path().to_string_lossy().into_owned();
    plan.process.terminal = TerminalMode::Captured;

    let report = Supervisor::new()
        .without_direct_chat_gateway()
        .execute(&plan, &test_config(), &CancellationToken::new())
        .await
        .expect("direct launch should complete without a gateway");

    assert_eq!(report.outcome, ExecutionOutcome::Succeeded);
    assert_eq!(report.provider_usage, None);
    assert!(report.bridge_diagnostics.is_empty());
    assert_removed(report.temporary_root);
}

#[tokio::test]
async fn direct_chat_gateway_is_enabled_by_default() {
    let report = execute_shell(
        "test \"${#NAN_API_KEY}\" -eq 64 && test \"$NAN_API_KEY\" != test-key",
        true,
        None,
        None,
        None,
    )
    .await;

    assert_eq!(report.outcome, ExecutionOutcome::Succeeded);
}
