use super::lifecycle::{millis, record_activity};
use super::state::tokens_match;
use crate::protocol::{
    AttemptOutcome, AttemptPhase, ClientMessage, ServerMessage, read_frame, write_frame,
};
use crate::scheduler::{AcquireRequest, ObservationRequest, Scheduler};
use crate::{CaptureLeg, CaptureSink};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite};
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
    let (reader, writer) = stream.into_split();
    handle_transport(
        Transport { reader, writer },
        token,
        scheduler,
        connections,
        last_activity,
        capture,
        started,
    )
    .await;
}

/// The read and write halves a connection is served over.
struct Transport<R, W> {
    reader: R,
    writer: W,
}

/// The connection body, over the transport alone.
///
/// Splitting the transport out of [`handle_connection`] lets the tests drive a
/// grant whose reply frame fails to write while the client half stays open,
/// which a real socket cannot do without racing the queued-disconnect arm of
/// the select below. The seam is private: the daemon still serves a `TcpStream`.
async fn handle_transport<R, W>(
    transport: Transport<R, W>,
    token: String,
    scheduler: Scheduler,
    connections: Arc<AtomicUsize>,
    last_activity: Arc<AtomicU64>,
    capture: CaptureSink,
    started: Instant,
) where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let Transport {
        mut reader,
        mut writer,
    } = transport;
    let _guard = ConnectionGuard(connections);
    let Some(acquire_context) = read_authenticated_acquire(&mut reader, &mut writer, &token).await
    else {
        return;
    };
    let acquire = scheduler.acquire(AcquireRequest {
        scope: acquire_context.scope.clone(),
        launch_id: acquire_context.launch_id.clone(),
        lane: acquire_context.lane,
        priority: acquire_context.priority,
        enqueued_at: Instant::now(),
        budget_tokens: acquire_context.budget_tokens,
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
    if let Some(rejection) = grant.rejection {
        reject(&mut writer, rejection_reason(&rejection)).await;
        return;
    }
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
        settle_grant_write_failure(&scheduler, acquire_context).await;
        return;
    }
    record_activity(&last_activity, started);
    let capture = capture.begin_request(format!("lease_{}", grant.lease_id));
    if let Some(capture) = &capture {
        let event = serde_json::json!({
            "event": "permit_granted",
            "launch_id": acquire_context.launch_id,
            "queued_ms": millis(grant.queued),
            "endpoint": acquire_context.endpoint,
            "model": acquire_context.model,
            "lane": acquire_context.lane,
            "priority": acquire_context.priority,
        });
        if let Ok(payload) = serde_json::to_vec(&event) {
            capture.record(CaptureLeg::Coordinator, &payload);
        }
    }
    let context = LeaseContext {
        scope: acquire_context.scope,
        lease_id: grant.lease_id,
        growth_eligible: grant.growth_eligible,
        foreground_inference: is_foreground_inference(
            acquire_context.lane,
            acquire_context.priority,
        ),
        launch_id: acquire_context.launch_id,
        budget_tokens: acquire_context.budget_tokens,
        capture,
    };
    observe_until_release(&mut reader, &mut writer, &scheduler, context).await;
    record_activity(&last_activity, started);
}

struct AcquireContext {
    scope: String,
    launch_id: String,
    endpoint: crate::EndpointKind,
    model: Option<String>,
    lane: crate::RequestLane,
    priority: crate::RequestPriority,
    budget_tokens: Option<u64>,
}

async fn read_authenticated_acquire<R, W>(
    reader: &mut R,
    writer: &mut W,
    token: &str,
) -> Option<AcquireContext>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let Ok(ClientMessage::Acquire {
        protocol_version,
        token: supplied_token,
        scope,
        launch_id,
        endpoint,
        model,
        lane,
        priority,
        budget_tokens,
    }) = read_frame::<ClientMessage>(reader).await
    else {
        return None;
    };
    if protocol_version != crate::protocol::PROTOCOL_VERSION
        || !tokens_match(token, &supplied_token)
    {
        reject(
            writer,
            "incompatible or unauthorized coordinator client".to_owned(),
        )
        .await;
        return None;
    }
    Some(AcquireContext {
        scope,
        launch_id,
        endpoint,
        model,
        lane,
        priority,
        budget_tokens,
    })
}

fn rejection_reason(rejection: &crate::scheduler::GrantRejection) -> String {
    match rejection {
        crate::scheduler::GrantRejection::BudgetExhausted { consumed, limit } => {
            format!("budget_exhausted:{consumed}:{limit}")
        }
        crate::scheduler::GrantRejection::AccountingUnavailable => {
            "accounting_unavailable".to_owned()
        }
        crate::scheduler::GrantRejection::BudgetMismatch => "budget_mismatch".to_owned(),
    }
}

async fn reject(writer: &mut (impl AsyncWrite + Unpin), reason: String) {
    let _ = write_frame(writer, &ServerMessage::Rejected { reason }).await;
}

async fn settle_grant_write_failure(scheduler: &Scheduler, acquire: AcquireContext) {
    let foreground_inference = is_foreground_inference(acquire.lane, acquire.priority);
    let _ = scheduler
        .observe(ObservationRequest {
            scope: acquire.scope.clone(),
            outcome: AttemptOutcome::Cancelled,
            retry_after: None,
            growth_eligible: false,
            foreground_inference,
            headers_elapsed: None,
            launch_id: acquire.launch_id,
            budget_tokens: acquire.budget_tokens,
            usage: None,
        })
        .await;
    scheduler.release(acquire.scope, foreground_inference);
}

fn is_foreground_inference(lane: crate::RequestLane, priority: crate::RequestPriority) -> bool {
    lane == crate::RequestLane::Inference && priority == crate::RequestPriority::Foreground
}

struct LeaseContext {
    scope: String,
    lease_id: u64,
    growth_eligible: bool,
    foreground_inference: bool,
    launch_id: String,
    budget_tokens: Option<u64>,
    capture: Option<crate::CaptureRequest>,
}

async fn observe_until_release(
    reader: &mut (impl AsyncRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
    scheduler: &Scheduler,
    context: LeaseContext,
) {
    let LeaseContext {
        scope,
        lease_id,
        growth_eligible,
        foreground_inference,
        launch_id,
        budget_tokens,
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
                usage,
            }) if observed_lease == lease_id => break Some((outcome, retry_after_ms, usage)),
            Ok(_) | Err(_) => break None,
        }
    };
    let Some((outcome, retry_after_ms, usage)) = observed else {
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
            .observe(ObservationRequest {
                scope: scope.clone(),
                outcome: AttemptOutcome::Cancelled,
                retry_after: None,
                growth_eligible: false,
                foreground_inference,
                headers_elapsed: None,
                launch_id: launch_id.clone(),
                budget_tokens,
                usage: None,
            })
            .await;
        scheduler.release(scope, foreground_inference);
        return;
    };
    let retry_after = retry_after_ms.map(Duration::from_millis);
    let observation = scheduler
        .observe(ObservationRequest {
            scope: scope.clone(),
            outcome,
            retry_after,
            growth_eligible,
            foreground_inference,
            headers_elapsed: headers_ms.map(Duration::from_millis),
            launch_id,
            budget_tokens,
            usage,
        })
        .await;
    let delay = observation.map_or(Duration::ZERO, |value| value.delay);
    if let Some(capture) = &capture {
        let event = serde_json::json!({
            "event": "attempt_observed",
            "retry_delay_ms": millis(delay),
            "retry_delay_source": retry_delay_source(outcome, retry_after, delay),
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

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum RetryDelaySource {
    ProviderHint,
    LocalPolicy,
}

fn retry_delay_source(
    outcome: AttemptOutcome,
    hint: Option<Duration>,
    delay: Duration,
) -> Option<RetryDelaySource> {
    is_retryable(outcome).then(|| {
        if matches!(
            outcome,
            AttemptOutcome::RateLimited | AttemptOutcome::ServerError
        ) && hint.is_some_and(|hint| hint >= delay)
        {
            RetryDelaySource::ProviderHint
        } else {
            RetryDelaySource::LocalPolicy
        }
    })
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

#[cfg(test)]
#[path = "connection/harness.rs"]
mod harness;

#[cfg(test)]
#[path = "connection/accounting_tests.rs"]
mod accounting_tests;

#[cfg(test)]
#[path = "connection/lease_ownership_tests.rs"]
mod lease_ownership_tests;

#[cfg(test)]
#[path = "connection/outcome_tests.rs"]
mod outcome_tests;

#[cfg(test)]
#[path = "connection/progress_tests.rs"]
mod progress_tests;

#[cfg(test)]
#[path = "connection/grant_write_failure_tests.rs"]
mod grant_write_failure_tests;
