//! Capacity accounting when the GRANTED frame cannot be written.
//!
//! The scheduler has already charged a permit by the time the reply frame is
//! written, so the failure path must hand back exactly that permit and it must
//! classify it correctly: a foreground-inference lease also occupies the
//! foreground-inference count that drives window growth. Closing a real socket
//! cannot express this, because the peer would race the queued-disconnect arm
//! of the handler's select and could be observed before the grant. These tests
//! therefore feed the handler a reader that stays open after the acquire frame
//! and a writer that fails the grant write, which can only happen after the
//! real scheduler has granted.
//!
//! Foreground accounting is observed through a later grant's `growth_eligible`
//! rather than a counter, because that flag is the policy the count exists for.
//! With the initial window of two, a foreground grant is growth-eligible only
//! when it saturates that window, so one anchored lease of the right kind makes
//! the flag flip on a miscounted release.

use super::harness::{QUIET_WINDOW, STEP_DEADLINE, TOKEN, acquire};
use super::{Transport, handle_transport};
use crate::CaptureSink;
use crate::protocol::{PROTOCOL_VERSION, RequestLane, RequestPriority, write_frame};
use crate::scheduler::{AcquireRequest, Grant, Scheduler};
use std::collections::VecDeque;
use std::io::{self, ErrorKind};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Instant;
use tempfile::TempDir;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::task::JoinHandle;

/// A client that sends one framed message and then stays connected and silent.
///
/// Pending without a waker is deliberate: the handler must reach its grant from
/// the scheduler arm of the select, never from a disconnect.
struct SilentAfterFrame {
    frame: VecDeque<u8>,
}

impl AsyncRead for SilentAfterFrame {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.frame.is_empty() {
            return Poll::Pending;
        }
        let count = self.frame.len().min(buffer.remaining());
        let chunk: Vec<u8> = self.frame.drain(..count).collect();
        buffer.put_slice(&chunk);
        Poll::Ready(Ok(()))
    }
}

/// A peer whose first write fails, standing in for a client that vanished
/// between the grant and its acknowledgement.
struct FailingWriter {
    writes: Arc<AtomicUsize>,
}

impl AsyncWrite for FailingWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        _buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.writes.fetch_add(1, Ordering::Relaxed);
        Poll::Ready(Err(io::Error::new(
            ErrorKind::BrokenPipe,
            "the client is gone",
        )))
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// One real scheduler, driven by the connection handler and by the test.
struct GrantFixture {
    scheduler: Scheduler,
    connections: Arc<AtomicUsize>,
    _directory: TempDir,
}

impl GrantFixture {
    fn start() -> Self {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        Self {
            scheduler: Scheduler::start(directory.path().join("capacity.json")),
            connections: Arc::new(AtomicUsize::new(0)),
            _directory: directory,
        }
    }

    fn request(scope: &str, lane: RequestLane, priority: RequestPriority) -> AcquireRequest {
        AcquireRequest {
            scope: scope.to_owned(),
            launch_id: "codex".to_owned(),
            lane,
            priority,
            enqueued_at: Instant::now(),
        }
    }

    /// Takes and holds a permit on behalf of the test.
    async fn anchor(&self, scope: &str, lane: RequestLane, priority: RequestPriority) -> Grant {
        let request = Self::request(scope, lane, priority);
        tokio::time::timeout(STEP_DEADLINE, self.scheduler.acquire(request))
            .await
            .expect("an anchor within the window should be granted promptly")
            .expect("the scheduler should answer an anchor request")
    }

    /// Requests a permit without waiting for it, so the test can observe
    /// whether it is granted or stays queued.
    fn request_in_background(
        &self,
        scope: &str,
        lane: RequestLane,
        priority: RequestPriority,
    ) -> JoinHandle<Option<Grant>> {
        let scheduler = self.scheduler.clone();
        let request = Self::request(scope, lane, priority);
        tokio::spawn(async move { scheduler.acquire(request).await })
    }

    /// Runs the handler for one acquire whose grant frame cannot be written.
    async fn fail_grant_write(&self, scope: &str, lane: RequestLane, priority: RequestPriority) {
        let mut frame = Vec::new();
        write_frame(
            &mut frame,
            &acquire(scope, lane, priority, PROTOCOL_VERSION, TOKEN),
        )
        .await
        .expect("the acquire frame should encode");
        let writes = Arc::new(AtomicUsize::new(0));
        // The accept loop counts a connection before handing it to the handler.
        self.connections.fetch_add(1, Ordering::Relaxed);
        tokio::time::timeout(
            STEP_DEADLINE,
            handle_transport(
                Transport {
                    reader: SilentAfterFrame {
                        frame: frame.into(),
                    },
                    writer: FailingWriter {
                        writes: Arc::clone(&writes),
                    },
                },
                TOKEN.to_owned(),
                self.scheduler.clone(),
                Arc::clone(&self.connections),
                Arc::new(AtomicU64::new(0)),
                CaptureSink::new("coordinator_test"),
                Instant::now(),
            ),
        )
        .await
        .expect("the handler should finish within the step deadline");
        assert_eq!(
            writes.load(Ordering::Relaxed),
            1,
            "the handler should have attempted exactly the grant frame, \
             which it only reaches once the scheduler has granted"
        );
        assert_eq!(
            self.connections.load(Ordering::Relaxed),
            0,
            "the failed connection should release its active-connection slot"
        );
    }
}

/// Asserts that the capacity permit came back exactly once: the next request is
/// granted, and the one after it waits behind the anchor that is still held.
async fn expect_one_permit_returned(
    fixture: &GrantFixture,
    scope: &str,
) -> (Grant, JoinHandle<Option<Grant>>) {
    let recovered =
        fixture.request_in_background(scope, RequestLane::Inference, RequestPriority::Foreground);
    let grant = tokio::time::timeout(STEP_DEADLINE, recovered)
        .await
        .expect("the released permit should be regranted within the step deadline")
        .expect("the request task should not panic")
        .expect("the scheduler should answer the request");
    let mut queued =
        fixture.request_in_background(scope, RequestLane::Inference, RequestPriority::Foreground);
    assert!(
        tokio::time::timeout(QUIET_WINDOW, &mut queued)
            .await
            .is_err(),
        "the anchor still holds the other permit, so a further request waits"
    );
    (grant, queued)
}

#[tokio::test]
async fn a_failed_foreground_grant_returns_its_foreground_inference_count() {
    let scope = "failed-foreground";
    let fixture = GrantFixture::start();
    // A control anchor occupies a permit without occupying the foreground
    // count, so a leaked foreground count is the only thing that could make the
    // next foreground grant look growth-eligible.
    let _control_anchor = fixture
        .anchor(scope, RequestLane::Control, RequestPriority::Foreground)
        .await;

    fixture
        .fail_grant_write(scope, RequestLane::Inference, RequestPriority::Foreground)
        .await;

    let (grant, queued) = expect_one_permit_returned(&fixture, scope).await;
    assert!(
        !grant.growth_eligible,
        "one foreground lease does not saturate the window, so the failed \
         foreground lease must not still be counted"
    );

    queued.abort();
    fixture.scheduler.release(scope.to_owned(), true);
    fixture.scheduler.release(scope.to_owned(), false);
}

#[tokio::test]
async fn a_failed_background_inference_grant_keeps_the_foreground_inference_count() {
    let scope = "failed-background-inference";
    let fixture = GrantFixture::start();
    let _foreground_anchor = fixture
        .anchor(scope, RequestLane::Inference, RequestPriority::Foreground)
        .await;

    fixture
        .fail_grant_write(scope, RequestLane::Inference, RequestPriority::Background)
        .await;

    let (grant, queued) = expect_one_permit_returned(&fixture, scope).await;
    assert!(
        grant.growth_eligible,
        "the background lease never held a foreground slot, so the legitimate \
         foreground anchor must still saturate the window"
    );

    queued.abort();
    fixture.scheduler.release(scope.to_owned(), true);
    fixture.scheduler.release(scope.to_owned(), true);
}

#[tokio::test]
async fn a_failed_foreground_control_grant_keeps_the_foreground_inference_count() {
    let scope = "failed-foreground-control";
    let fixture = GrantFixture::start();
    let _foreground_anchor = fixture
        .anchor(scope, RequestLane::Inference, RequestPriority::Foreground)
        .await;

    fixture
        .fail_grant_write(scope, RequestLane::Control, RequestPriority::Foreground)
        .await;

    let (grant, queued) = expect_one_permit_returned(&fixture, scope).await;
    assert!(
        grant.growth_eligible,
        "a control lease is not foreground inference, however foreground its \
         priority, so the foreground anchor must survive its release"
    );

    queued.abort();
    fixture.scheduler.release(scope.to_owned(), true);
    fixture.scheduler.release(scope.to_owned(), true);
}

#[tokio::test]
async fn a_failed_background_control_grant_keeps_the_foreground_inference_count() {
    let scope = "failed-background-control";
    let fixture = GrantFixture::start();
    let _foreground_anchor = fixture
        .anchor(scope, RequestLane::Inference, RequestPriority::Foreground)
        .await;

    fixture
        .fail_grant_write(scope, RequestLane::Control, RequestPriority::Background)
        .await;

    let (grant, queued) = expect_one_permit_returned(&fixture, scope).await;
    assert!(
        grant.growth_eligible,
        "neither lane nor priority is foreground inference, so the foreground \
         anchor must survive its release"
    );

    queued.abort();
    fixture.scheduler.release(scope.to_owned(), true);
    fixture.scheduler.release(scope.to_owned(), true);
}
