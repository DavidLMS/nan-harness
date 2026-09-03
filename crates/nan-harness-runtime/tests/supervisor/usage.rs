use super::support::{DIRECT_PLAN, assert_removed, start_chat_provider, test_config_with_url};
use nan_harness_core::LaunchPlan;
use nan_harness_core::launch_plan::{PROVIDER_BASE_URL_PLACEHOLDER, TerminalMode};
use nan_harness_runtime::{
    CancellationToken, ExecutionOutcome, ModelUsageSnapshot, ProviderUsageSnapshot, SignalKind,
    Supervisor,
};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

#[tokio::test]
async fn supervisor_propagates_provider_usage_after_the_bridge_waits() {
    let with_usage = execute_direct_chat_request(true).await;
    assert_eq!(with_usage.outcome, ExecutionOutcome::Succeeded);
    assert_eq!(
        with_usage.provider_usage,
        Some(usage_for(ModelUsageSnapshot {
            responses_with_usage: 1,
            input_tokens: 1,
            output_tokens: 2,
            ..ModelUsageSnapshot::default()
        }))
    );
    assert_removed(with_usage.temporary_root);

    let without_usage = execute_direct_chat_request(false).await;
    assert_eq!(without_usage.outcome, ExecutionOutcome::Succeeded);
    assert_eq!(
        without_usage.provider_usage,
        Some(usage_for(ModelUsageSnapshot {
            responses_without_usage: 1,
            ..ModelUsageSnapshot::default()
        }))
    );
    assert_removed(without_usage.temporary_root);
}

#[tokio::test]
async fn supervisor_preserves_confirmed_usage_after_failure_and_cancellation() {
    let failed = execute_direct_chat_request_with_tail(true, "exit 7", None, None).await;
    assert_eq!(failed.outcome, ExecutionOutcome::Failed);
    assert_eq!(failed.exit_code, 7);
    assert_eq!(failed.provider_usage, Some(confirmed_direct_usage()));

    let cancellation = CancellationToken::new();
    let trigger = cancellation.clone();
    let directory = tempfile::tempdir().expect("ready directory should exist");
    let ready = directory.path().join("request-complete");
    let trigger_ready = ready.clone();
    let task = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(1), async {
            while !trigger_ready.exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the request should complete before cancellation");
        trigger.cancel(SignalKind::Interrupt);
    });
    let cancelled = execute_direct_chat_request_with_tail(
        true,
        ": > \"$1\"; while :; do :; done",
        Some(&cancellation),
        Some(&ready),
    )
    .await;
    task.await.expect("cancellation task should finish");
    assert_eq!(
        cancelled.outcome,
        ExecutionOutcome::Cancelled(SignalKind::Interrupt)
    );
    assert_eq!(cancelled.provider_usage, Some(confirmed_direct_usage()));
}

async fn execute_direct_chat_request(with_usage: bool) -> nan_harness_runtime::ExecutionReport {
    execute_direct_chat_request_with_tail(with_usage, "exit 0", None, None).await
}

async fn execute_direct_chat_request_with_tail(
    with_usage: bool,
    tail: &str,
    cancellation: Option<&CancellationToken>,
    ready_path: Option<&Path>,
) -> nan_harness_runtime::ExecutionReport {
    let (provider_base_url, provider_task) = start_chat_provider(with_usage).await;
    let working_directory = tempfile::tempdir().expect("working directory should exist");
    let mut plan: LaunchPlan = serde_json::from_str(DIRECT_PLAN).expect("valid direct fixture");
    "/bin/sh".clone_into(&mut plan.harness.executable);
    plan.environment.public.insert(
        "NAN_HARNESS_PROVIDER_BASE_URL".to_owned(),
        PROVIDER_BASE_URL_PLACEHOLDER.to_owned(),
    );
    plan.process.arguments = vec![
        "-c".to_owned(),
        format!(
            "{}; {tail}",
            concat!(
                "curl --fail --silent --show-error --header \"Authorization: Bearer $NAN_API_KEY\" ",
                "--header 'Content-Type: application/json' ",
                "--data '{\"model\":\"qwen3.6\",\"messages\":[]}' ",
                "$NAN_HARNESS_PROVIDER_BASE_URL/chat/completions >/dev/null"
            )
        ),
    ];
    if let Some(ready_path) = ready_path {
        plan.process.arguments.extend([
            "nan-harness-test".to_owned(),
            ready_path.to_string_lossy().into_owned(),
        ]);
    }
    plan.process.working_directory = working_directory.path().to_string_lossy().into_owned();
    plan.process.terminal = TerminalMode::Captured;
    let default_cancellation = CancellationToken::new();

    let report = Supervisor::new()
        .execute(
            &plan,
            &test_config_with_url(provider_base_url),
            cancellation.unwrap_or(&default_cancellation),
        )
        .await
        .expect("direct chat launch should complete");
    provider_task.abort();
    report
}

fn usage_for(model: ModelUsageSnapshot) -> ProviderUsageSnapshot {
    ProviderUsageSnapshot {
        models: BTreeMap::from([("qwen3.6".to_owned(), model)]),
    }
}

fn confirmed_direct_usage() -> ProviderUsageSnapshot {
    usage_for(ModelUsageSnapshot {
        responses_with_usage: 1,
        input_tokens: 1,
        output_tokens: 2,
        ..ModelUsageSnapshot::default()
    })
}
