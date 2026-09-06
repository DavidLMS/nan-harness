use super::{
    HermesDesktopError, UpdateWaitCompletion, live_update_owner, supervision::SupervisedGateway,
    update_interrupt_requests_exit,
};
use std::path::Path;
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests;

/// How long a marker may outlive its owning updater process before the wait
/// treats the update as finished.
pub(super) const UPDATE_STALE_GRACE: Duration = Duration::from_secs(5);

/// Observation of the update marker and of the updater that owns it, kept
/// separate from the wait policy below so the policy can be exercised without a
/// real updater.
pub(super) trait UpdateState {
    fn marker_exists(&self) -> bool;

    fn live_owner_present(&self) -> Result<bool, HermesDesktopError>;
}

pub(super) struct SystemUpdateState<'a> {
    pub(super) marker: &'a Path,
}

impl UpdateState for SystemUpdateState<'_> {
    fn marker_exists(&self) -> bool {
        self.marker.exists()
    }

    fn live_owner_present(&self) -> Result<bool, HermesDesktopError> {
        live_update_owner(self.marker).map(|owner| owner.is_some())
    }
}

/// How often the wait observes the update, how long it waits in total, and how
/// long it tolerates a marker whose owner is gone.
#[derive(Debug, Clone, Copy)]
pub(super) struct UpdateWaitTiming {
    pub(super) poll_interval: Duration,
    pub(super) total_timeout: Duration,
    pub(super) stale_grace: Duration,
}

pub(super) async fn wait_for_update(
    state: &impl UpdateState,
    gateway: &mut impl SupervisedGateway,
    signals: &mut tokio::sync::mpsc::UnboundedReceiver<i32>,
    timing: UpdateWaitTiming,
) -> Result<UpdateWaitCompletion, HermesDesktopError> {
    let started = Instant::now();
    let mut interrupt_seen = false;
    let mut stale_since = None;
    loop {
        if !state.marker_exists() {
            return Ok(UpdateWaitCompletion::Finished { interrupt_seen });
        }
        if state.live_owner_present()? {
            stale_since = None;
        } else {
            let since = stale_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= timing.stale_grace {
                return Ok(UpdateWaitCompletion::Finished { interrupt_seen });
            }
        }
        if started.elapsed() >= timing.total_timeout {
            return Err(HermesDesktopError::UpdateTimedOut);
        }
        tokio::select! {
            () = tokio::time::sleep(timing.poll_interval) => {}
            signal = signals.recv() => {
                let code = signal.unwrap_or(143);
                if update_interrupt_requests_exit(code, &mut interrupt_seen) {
                    eprintln!("NaN is exiting while the Hermes Desktop updater continues. Run `nanh hermes-desktop --restore` after the update finishes.");
                    return Ok(UpdateWaitCompletion::PreserveRecovery(code));
                }
                eprintln!("Hermes Desktop is still updating. Press Ctrl+C again to exit NaN while the updater continues.");
            }
            gateway_result = gateway.wait() => {
                return Err(gateway_result.err().unwrap_or(HermesDesktopError::GatewayExited));
            }
        }
    }
}
