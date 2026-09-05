use super::lifecycle::{millis, record_activity};
use super::state::tokens_match;
use crate::protocol::{
    AttemptOutcome, AttemptPhase, ClientMessage, ServerMessage, read_frame, write_frame,
};
use crate::scheduler::{AcquireRequest, Scheduler};
use crate::{CaptureLeg, CaptureSink};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt as _;
use tokio::net::TcpStream;

pub(super) async fn handle_connection(
    stream: TcpStream,
    token: String,
    scheduler: Scheduler,
    connections: Arc<AtomicUsize>,
    last_activity: Arc<AtomicU64>,
    capture: CaptureSink,
    started: Instant,
) {
    let _guard = ConnectionGuard(connections);
    let (mut reader, mut writer) = stream.into_split();
    let Ok(message) = read_frame::<ClientMessage>(&mut reader).await else {
        return;
    };
    let ClientMessage::Acquire {
        protocol_version,
        token: supplied_token,
        scope,
        launch_id,
        endpoint,
        model,
        lane,
        priority,
    } = message
    else {
        return;
    };
    if protocol_version != crate::protocol::PROTOCOL_VERSION
        || !tokens_match(&token, &supplied_token)
    {
        let _ = write_frame(
            &mut writer,
            &ServerMessage::Rejected {
                reason: "incompatible or unauthorized coordinator client".to_owned(),
            },
        )
        .await;
        return;
    }
    let event_launch_id = launch_id.clone();
    let acquire = scheduler.acquire(AcquireRequest {
        scope: scope.clone(),
        launch_id,
        lane,
        priority,
        enqueued_at: Instant::now(),
    });
    tokio::pin!(acquire);
    let grant = tokio::select! {
        grant = &mut acquire => grant,
        _disconnected = reader.read_u8() => {
            return;
        }
    };
    let Some(grant) = grant else {
        return;
    };
    if write_frame(
        &mut writer,
        &ServerMessage::Granted {
            lease_id: grant.lease_id,
            queued_ms: millis(grant.queued),
        },
    )
    .await
    .is_err()
    {
        scheduler.release(
            scope,
            lane == crate::RequestLane::Inference && priority == crate::RequestPriority::Foreground,
        );
        return;
    }
    record_activity(&last_activity, started);
    let capture = capture.begin_request(format!("lease_{}", grant.lease_id));
    if let Some(capture) = &capture {
        let event = serde_json::json!({
            "event": "permit_granted",
            "launch_id": event_launch_id,
            "queued_ms": millis(grant.queued),
            "endpoint": endpoint,
            "model": model,
            "lane": lane,
            "priority": priority,
        });
        if let Ok(payload) = serde_json::to_vec(&event) {
            capture.record(CaptureLeg::Coordinator, &payload);
        }
    }
    let context = LeaseContext {
        scope,
        lease_id: grant.lease_id,
        growth_eligible: grant.growth_eligible,
        foreground_inference: lane == crate::RequestLane::Inference
            && priority == crate::RequestPriority::Foreground,
        capture,
    };
    observe_until_release(&mut reader, &mut writer, &scheduler, context).await;
    record_activity(&last_activity, started);
}

struct LeaseContext {
    scope: String,
    lease_id: u64,
    growth_eligible: bool,
    foreground_inference: bool,
    capture: Option<crate::CaptureRequest>,
}

async fn observe_until_release(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    scheduler: &Scheduler,
    context: LeaseContext,
) {
    let LeaseContext {
        scope,
        lease_id,
        growth_eligible,
        foreground_inference,
        capture,
    } = context;
    let mut headers_ms = None;
    let observed = loop {
        match read_frame::<ClientMessage>(reader).await {
            Ok(ClientMessage::Progress {
                lease_id: observed_lease,
                phase: AttemptPhase::HeadersReceived,
                elapsed_ms,
            }) if observed_lease == lease_id => {
                headers_ms = Some(elapsed_ms);
            }
            Ok(ClientMessage::Observe {
                lease_id: observed_lease,
                outcome,
                retry_after_ms,
            }) if observed_lease == lease_id => break Some((outcome, retry_after_ms)),
            Ok(_) | Err(_) => break None,
        }
    };
    let Some((outcome, retry_after_ms)) = observed else {
        if let Some(capture) = &capture {
            let event = serde_json::json!({
                "event": "attempt_abandoned",
                "outcome": AttemptOutcome::Cancelled,
            });
            if let Ok(payload) = serde_json::to_vec(&event) {
                capture.record(CaptureLeg::Coordinator, &payload);
            }
        }
        let _ = scheduler
            .observe(
                scope.clone(),
                AttemptOutcome::Cancelled,
                None,
                false,
                foreground_inference,
                None,
            )
            .await;
        scheduler.release(scope, foreground_inference);
        return;
    };
    let retry_after = retry_after_ms.map(Duration::from_millis);
    let observation = scheduler
        .observe(
            scope.clone(),
            outcome,
            retry_after,
            growth_eligible,
            foreground_inference,
            headers_ms.map(Duration::from_millis),
        )
        .await;
    if let Some(capture) = &capture {
        let event = serde_json::json!({
            "event": "attempt_observed",
            "outcome": outcome,
            "retry_after_ms": retry_after_ms,
            "headers_ms": headers_ms,
            "previous_window": observation.map(|value| value.previous_window),
            "window": observation.map(|value| value.window),
            "growth_blocked_seconds": observation.map(|value| value.growth_blocked_seconds),
        });
        if let Ok(payload) = serde_json::to_vec(&event) {
            capture.record(CaptureLeg::Coordinator, &payload);
        }
    }
    let delay = observation.map_or(Duration::ZERO, |value| value.delay);
    if is_retryable(outcome) {
        scheduler.release(scope, foreground_inference);
        let _ = write_frame(
            writer,
            &ServerMessage::Retry {
                delay_ms: millis(delay),
            },
        )
        .await;
        return;
    }
    let _ = write_frame(writer, &ServerMessage::Complete).await;
    scheduler.release(scope, foreground_inference);
}

const fn is_retryable(outcome: AttemptOutcome) -> bool {
    matches!(
        outcome,
        AttemptOutcome::Transport
            | AttemptOutcome::Timeout
            | AttemptOutcome::RateLimited
            | AttemptOutcome::ServerError
            | AttemptOutcome::InvalidResponse
    )
}

struct ConnectionGuard(Arc<AtomicUsize>);

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
#[path = "connection_tests.rs"]
mod tests;
