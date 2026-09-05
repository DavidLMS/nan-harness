use super::events;
use super::state::StreamState;
use axum::response::sse::Event;
use std::time::Duration;

pub(super) const PROGRESS_INTERVAL: Duration = Duration::from_secs(30);

/// Paces the protocol-level `response.in_progress` heartbeat that keeps the
/// harness alive while an upstream send or a recovery attempt is in flight.
pub(super) struct ProgressTicker {
    interval: tokio::time::Interval,
    logical_response: StreamState,
}

impl ProgressTicker {
    /// Consumes the immediate first tick so the caller only observes beats
    /// after a full interval has elapsed.
    pub(super) async fn start(interval: Duration, logical_response: StreamState) -> Self {
        let mut interval = tokio::time::interval(interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        interval.tick().await;
        Self {
            interval,
            logical_response,
        }
    }

    /// Cancel-safe: dropping the returned future keeps the interval schedule.
    pub(super) async fn beat(&mut self) -> Event {
        self.interval.tick().await;
        events::in_progress(&self.logical_response)
    }
}
