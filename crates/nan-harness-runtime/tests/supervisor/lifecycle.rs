use super::support::{assert_removed, execute_shell};
use nan_harness_runtime::{CancellationToken, ExecutionOutcome, ProviderUsageSnapshot, SignalKind};
use std::time::{Duration, Instant};

#[tokio::test]
async fn supervisor_preserves_success_and_failure_exit_codes_and_cleans_up() {
    let success = execute_shell("exit 0", true, None, None, None).await;
    assert_eq!(success.outcome, ExecutionOutcome::Succeeded);
    assert_eq!(success.exit_code, 0);
    assert_eq!(
        success.provider_usage,
        Some(ProviderUsageSnapshot::default())
    );
    assert_removed(success.temporary_root);

    let failure = execute_shell("exit 7", true, None, None, None).await;
    assert_eq!(failure.outcome, ExecutionOutcome::Failed);
    assert_eq!(failure.exit_code, 7);
    assert_eq!(
        failure.provider_usage,
        Some(ProviderUsageSnapshot::default())
    );
    assert_removed(failure.temporary_root);

    let normalized = execute_shell("exit 7", false, None, None, None).await;
    assert_eq!(normalized.exit_code, 1);
    assert_removed(normalized.temporary_root);
}

#[tokio::test]
async fn supervisor_cancels_a_child_and_cleans_up() {
    let cancellation = CancellationToken::new();
    let trigger = cancellation.clone();
    let task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(40)).await;
        trigger.cancel(SignalKind::Interrupt);
    });
    let report = execute_shell("while :; do :; done", true, Some(&cancellation), None, None).await;
    task.await.expect("cancellation task should finish");

    assert_eq!(
        report.outcome,
        ExecutionOutcome::Cancelled(SignalKind::Interrupt)
    );
    assert_eq!(report.exit_code, 130);
    assert_eq!(
        report.provider_usage,
        Some(ProviderUsageSnapshot::default())
    );
    assert_removed(report.temporary_root);
}

#[tokio::test]
async fn supervisor_force_cancels_a_child_that_ignores_the_first_signal() {
    let cancellation = CancellationToken::new();
    let ready_directory = tempfile::tempdir().expect("ready directory should exist");
    let ready_path = ready_directory.path().join("trap-ready");
    let trigger = cancellation.clone();
    let trigger_ready_path = ready_path.clone();
    let task = tokio::spawn(async move {
        let ready = tokio::time::timeout(Duration::from_secs(1), async {
            while !trigger_ready_path.exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .is_ok();
        if !ready {
            trigger.cancel(SignalKind::Interrupt);
            return false;
        }
        trigger.cancel(SignalKind::Interrupt);
        tokio::time::sleep(Duration::from_millis(80)).await;
        trigger.cancel(SignalKind::Interrupt);
        true
    });
    let started = Instant::now();
    let report = execute_shell(
        "trap '' INT; : > \"$1\"; while :; do :; done",
        true,
        Some(&cancellation),
        Some(1_000),
        Some(&ready_path),
    )
    .await;
    let elapsed = started.elapsed();
    assert!(
        task.await.expect("cancellation task should finish"),
        "child should install its signal handler before cancellation"
    );

    assert_eq!(
        report.outcome,
        ExecutionOutcome::Cancelled(SignalKind::Interrupt)
    );
    assert_eq!(report.exit_code, 130);
    assert!(
        elapsed < Duration::from_millis(500),
        "second cancellation should skip the grace period (elapsed: {elapsed:?})"
    );
    assert_removed(report.temporary_root);
}
