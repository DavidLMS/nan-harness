//! Progress contracts: header timings reported for the granted lease reach the
//! scheduler as the health evidence a saturated scope needs to earn capacity,
//! and an attempt that reports none never counts as healthy.

use super::harness::{
    HEALTHY_HEADERS_MS, TestDaemon, expect_message, expect_quiet, observe, progress, send,
};
use crate::protocol::{AttemptOutcome, ServerMessage};

/// Consecutive healthy successes a saturated scope needs before the coordinator
/// widens its initial two-permit window.
const HEALTHY_SUCCESSES: usize = 4;

#[tokio::test]
async fn reported_header_timings_let_a_saturated_scope_earn_capacity() {
    let scope = "healthy-growth";
    let daemon = TestDaemon::start().await;
    let (_saturating_holder, _) = daemon.lease(scope).await;

    for _ in 0..HEALTHY_SUCCESSES {
        complete_success(&daemon, scope, Some(HEALTHY_HEADERS_MS)).await;
    }

    let (_second, _) = daemon.lease(scope).await;
    let (_third, _) = daemon.lease(scope).await;
    let mut surplus = daemon.request(scope).await;
    expect_quiet(
        &mut surplus,
        "the window grew by exactly one permit, so a fourth concurrent lease still waits",
    )
    .await;
}

#[tokio::test]
async fn attempts_without_reported_header_timings_do_not_earn_capacity() {
    let scope = "unproven-growth";
    let daemon = TestDaemon::start().await;
    let (_saturating_holder, _) = daemon.lease(scope).await;

    for _ in 0..HEALTHY_SUCCESSES {
        complete_success(&daemon, scope, None).await;
    }

    let (_second, _) = daemon.lease(scope).await;
    let mut surplus = daemon.request(scope).await;
    expect_quiet(
        &mut surplus,
        "successes without header evidence are not healthy, so the window stays at two",
    )
    .await;
}

/// Takes a permit, optionally reports header timings, and completes the attempt.
async fn complete_success(daemon: &TestDaemon, scope: &str, headers_ms: Option<u64>) {
    let (mut client, lease_id) = daemon.lease(scope).await;
    if let Some(elapsed_ms) = headers_ms {
        send(&mut client, &progress(lease_id, elapsed_ms)).await;
    }
    send(
        &mut client,
        &observe(lease_id, AttemptOutcome::Success, None),
    )
    .await;
    assert!(matches!(
        expect_message(&mut client).await,
        ServerMessage::Complete
    ));
}
