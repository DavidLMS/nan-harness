//! Lease ownership contracts: a connection may only drive the lease it was
//! granted, and a message naming somebody else's lease must never complete,
//! release or otherwise mutate that lease.

use super::harness::{
    HEALTHY_HEADERS_MS, TOKEN, TestDaemon, acquire, connect, expect_closed_without_directive,
    expect_granted, expect_message, expect_quiet, observe, progress, send,
};
use crate::protocol::{
    AttemptOutcome, PROTOCOL_VERSION, RequestLane, RequestPriority, ServerMessage,
};

#[tokio::test]
async fn progress_for_the_granted_lease_keeps_the_attempt_open() {
    let daemon = TestDaemon::start().await;
    let (mut client, lease_id) = daemon.lease("owned-progress").await;

    send(&mut client, &progress(lease_id, HEALTHY_HEADERS_MS)).await;
    expect_quiet(
        &mut client,
        "progress is an observation, not a directive, so the lease stays open",
    )
    .await;

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

#[tokio::test]
async fn progress_naming_another_lease_ends_the_attempt_without_a_directive() {
    let daemon = TestDaemon::start().await;
    let (mut client, lease_id) = daemon.lease("foreign-progress").await;

    send(
        &mut client,
        &progress(lease_id.wrapping_add(1), HEALTHY_HEADERS_MS),
    )
    .await;

    expect_closed_without_directive(
        &mut client,
        "progress for an unowned lease must abandon the attempt",
    )
    .await;
}

#[tokio::test]
async fn an_observation_naming_another_lease_cannot_complete_the_attempt() {
    let daemon = TestDaemon::start().await;
    let (mut client, lease_id) = daemon.lease("foreign-observation").await;

    send(
        &mut client,
        &observe(lease_id.wrapping_add(1), AttemptOutcome::Success, None),
    )
    .await;

    expect_closed_without_directive(
        &mut client,
        "an outcome for an unowned lease must not be answered with a directive",
    )
    .await;
}

#[tokio::test]
async fn a_foreign_observation_releases_only_the_sending_connection() {
    let scope = "shared-scope";
    let daemon = TestDaemon::start().await;
    let (mut owner, owned_lease) = daemon.lease(scope).await;
    let (mut impostor, impostor_lease) = daemon.lease(scope).await;
    assert_ne!(
        owned_lease, impostor_lease,
        "concurrent connections must hold distinct leases"
    );

    let mut queued = daemon.request(scope).await;
    expect_quiet(&mut queued, "both initial permits are held").await;

    send(
        &mut impostor,
        &observe(owned_lease, AttemptOutcome::Success, None),
    )
    .await;
    expect_closed_without_directive(
        &mut impostor,
        "naming another connection's lease abandons the sender's own attempt",
    )
    .await;

    let queued_lease = expect_granted(&mut queued).await;
    assert_ne!(queued_lease, owned_lease);
    assert_ne!(queued_lease, impostor_lease);

    let mut surplus = daemon.request(scope).await;
    expect_quiet(
        &mut surplus,
        "the impostor released exactly one permit, so the owner's permit is still held",
    )
    .await;

    send(
        &mut owner,
        &observe(owned_lease, AttemptOutcome::Success, None),
    )
    .await;
    assert!(
        matches!(expect_message(&mut owner).await, ServerMessage::Complete),
        "the owner's lease survived the foreign observation and completes normally"
    );

    expect_granted(&mut surplus).await;
}

#[tokio::test]
async fn a_rejected_client_never_consumes_scheduler_capacity() {
    let scope = "unauthorized";
    let daemon = TestDaemon::start().await;

    let mut rejected = daemon
        .request_as(
            scope,
            RequestLane::Inference,
            RequestPriority::Foreground,
            "wrong-token",
        )
        .await;
    assert!(matches!(
        expect_message(&mut rejected).await,
        ServerMessage::Rejected { .. }
    ));

    let (_first, _) = daemon.lease(scope).await;
    let (_second, _) = daemon.lease(scope).await;
}

#[tokio::test]
async fn an_unsupported_protocol_version_is_rejected_before_any_lease_exists() {
    let daemon = TestDaemon::start().await;
    let mut client = connect(daemon.address()).await;

    send(
        &mut client,
        &acquire(
            "unsupported",
            RequestLane::Inference,
            RequestPriority::Foreground,
            PROTOCOL_VERSION.wrapping_add(1),
            TOKEN,
        ),
    )
    .await;

    assert!(matches!(
        expect_message(&mut client).await,
        ServerMessage::Rejected { .. }
    ));
}
