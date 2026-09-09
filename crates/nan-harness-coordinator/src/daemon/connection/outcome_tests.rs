//! Outcome contracts: which reported attempt outcomes earn a retry directive,
//! which end the lease, and which requests count as foreground inference for
//! the capacity that the connection releases.

use super::harness::{
    LONG_RETRY_AFTER_MS, TestDaemon, expect_granted, expect_message, expect_quiet, observe, send,
};
use crate::protocol::{AttemptOutcome, ServerMessage};

const RETRYABLE_OUTCOMES: [AttemptOutcome; 5] = [
    AttemptOutcome::Transport,
    AttemptOutcome::Timeout,
    AttemptOutcome::RateLimited,
    AttemptOutcome::ServerError,
    AttemptOutcome::InvalidResponse,
];

const TERMINAL_OUTCOMES: [AttemptOutcome; 3] = [
    AttemptOutcome::Success,
    AttemptOutcome::Cancelled,
    AttemptOutcome::Terminal,
];

#[tokio::test]
async fn retryable_outcomes_are_answered_with_a_retry_directive() {
    let daemon = TestDaemon::start().await;
    for (index, outcome) in RETRYABLE_OUTCOMES.into_iter().enumerate() {
        // A fresh scope per outcome keeps one outcome's cooldown out of the next.
        let (mut client, lease_id) = daemon.lease(&format!("retryable-{index}")).await;
        send(&mut client, &observe(lease_id, outcome, None)).await;
        assert!(
            matches!(
                expect_message(&mut client).await,
                ServerMessage::Retry { .. }
            ),
            "{outcome:?} is retryable and must be answered with a retry directive"
        );
    }
}

#[tokio::test]
async fn terminal_outcomes_are_answered_with_completion() {
    let daemon = TestDaemon::start().await;
    for (index, outcome) in TERMINAL_OUTCOMES.into_iter().enumerate() {
        let (mut client, lease_id) = daemon.lease(&format!("terminal-{index}")).await;
        send(&mut client, &observe(lease_id, outcome, None)).await;
        assert!(
            matches!(expect_message(&mut client).await, ServerMessage::Complete),
            "{outcome:?} ends the attempt and must be answered with completion"
        );
    }
}

#[tokio::test]
async fn a_retry_directive_reports_the_requested_retry_after() {
    let daemon = TestDaemon::start().await;
    let (mut client, lease_id) = daemon.lease("reported-retry-after").await;

    send(
        &mut client,
        &observe(
            lease_id,
            AttemptOutcome::RateLimited,
            Some(LONG_RETRY_AFTER_MS),
        ),
    )
    .await;

    match expect_message(&mut client).await {
        ServerMessage::Retry { delay_ms } => assert_eq!(delay_ms, LONG_RETRY_AFTER_MS),
        other => panic!("expected a retry directive, observed {other:?}"),
    }
}

#[tokio::test]
async fn a_foreground_inference_rate_limit_cools_the_scope_down() {
    let scope = "foreground-rate-limit";
    let daemon = TestDaemon::start().await;
    let (mut client, lease_id) = daemon.lease(scope).await;

    send(
        &mut client,
        &observe(
            lease_id,
            AttemptOutcome::RateLimited,
            Some(LONG_RETRY_AFTER_MS),
        ),
    )
    .await;
    assert!(matches!(
        expect_message(&mut client).await,
        ServerMessage::Retry { .. }
    ));

    let mut next = daemon.request(scope).await;
    expect_quiet(
        &mut next,
        "a rate limited foreground inference attempt holds the whole scope back",
    )
    .await;
}

#[tokio::test]
async fn a_control_lane_rate_limit_returns_the_permit_without_a_cooldown() {
    let scope = "control-rate-limit";
    let daemon = TestDaemon::start().await;
    let mut first = daemon.control_lease(scope).await;
    let first_lease = expect_granted(&mut first).await;
    let mut second = daemon.control_lease(scope).await;
    expect_granted(&mut second).await;

    let mut queued = daemon.control_lease(scope).await;
    expect_quiet(&mut queued, "both permits are held").await;

    send(
        &mut first,
        &observe(
            first_lease,
            AttemptOutcome::RateLimited,
            Some(LONG_RETRY_AFTER_MS),
        ),
    )
    .await;
    assert!(matches!(
        expect_message(&mut first).await,
        ServerMessage::Retry { .. }
    ));

    // The retry directive hands the permit back, and a control lane rate limit
    // never holds the whole scope back the way a foreground inference one does.
    expect_granted(&mut queued).await;
}

#[test]
fn retry_diagnostics_classify_only_the_chosen_delay_source() {
    use super::{RetryDelaySource, retry_delay_source};
    use std::time::Duration;
    for (outcome, hint, delay, expected) in [
        (
            AttemptOutcome::RateLimited,
            None,
            15,
            Some(RetryDelaySource::LocalPolicy),
        ),
        (
            AttemptOutcome::RateLimited,
            Some(0),
            0,
            Some(RetryDelaySource::ProviderHint),
        ),
        (
            AttemptOutcome::ServerError,
            Some(1),
            2,
            Some(RetryDelaySource::LocalPolicy),
        ),
        (
            AttemptOutcome::ServerError,
            Some(7),
            7,
            Some(RetryDelaySource::ProviderHint),
        ),
        (
            AttemptOutcome::Transport,
            Some(7),
            1,
            Some(RetryDelaySource::LocalPolicy),
        ),
        (AttemptOutcome::Success, None, 0, None),
    ] {
        assert_eq!(
            retry_delay_source(
                outcome,
                hint.map(Duration::from_secs),
                Duration::from_secs(delay)
            ),
            expected
        );
    }
    assert_eq!(
        serde_json::to_value(RetryDelaySource::ProviderHint).expect("source"),
        "provider_hint"
    );
    assert_eq!(
        serde_json::to_value(RetryDelaySource::LocalPolicy).expect("source"),
        "local_policy"
    );
}

#[tokio::test]
async fn a_hintless_rate_limit_pauses_other_requests_and_allows_cancellation() {
    let daemon = TestDaemon::start().await;
    let (mut client, lease_id) = daemon.lease("hintless-quota").await;
    send(
        &mut client,
        &observe(lease_id, AttemptOutcome::RateLimited, None),
    )
    .await;
    assert!(matches!(
        expect_message(&mut client).await,
        ServerMessage::Retry {
            delay_ms: 15_000..=20_000
        }
    ));
    let mut queued = daemon.request("hintless-quota").await;
    expect_quiet(&mut queued, "hintless quota cooldown holds shared requests").await;
    drop(queued);
    let (_other, _) = daemon.lease("unrelated-scope").await;
}
