#[allow(clippy::wildcard_imports)]
use super::*;

#[cfg(test)]
mod tests;

/// Observation and termination of the running desktop app, kept separate from
/// the supervision policy below so the policy can be exercised without real
/// processes.
pub(super) trait DesktopLifecycle {
    fn running(&self) -> Result<Option<DesktopProcess>, HermesDesktopError>;

    fn is_same(&self, process: &DesktopProcess) -> Result<bool, HermesDesktopError>;

    async fn terminate(&self) -> Result<(), HermesDesktopError>;
}

pub(super) struct SystemDesktopLifecycle;

impl DesktopLifecycle for SystemDesktopLifecycle {
    fn running(&self) -> Result<Option<DesktopProcess>, HermesDesktopError> {
        running_desktop()
    }

    fn is_same(&self, process: &DesktopProcess) -> Result<bool, HermesDesktopError> {
        process_is_same(process)
    }

    async fn terminate(&self) -> Result<(), HermesDesktopError> {
        terminate_desktop().await
    }
}

/// The gateway the session must keep alive; an absent gateway stays pending.
pub(super) trait SupervisedGateway {
    async fn wait(&mut self) -> Result<(), HermesDesktopError>;
}

impl SupervisedGateway for Option<&mut RunningChatCompletionsGateway> {
    async fn wait(&mut self) -> Result<(), HermesDesktopError> {
        wait_for_gateway(self).await
    }
}

pub(super) async fn supervise_running(
    mut process: DesktopProcess,
    lifecycle: &impl DesktopLifecycle,
    gateway: &mut impl SupervisedGateway,
    signals: &mut tokio::sync::mpsc::UnboundedReceiver<i32>,
    poll_interval: Duration,
) -> Result<LifecycleCompletion, HermesDesktopError> {
    loop {
        tokio::select! {
            () = tokio::time::sleep(poll_interval) => {
                if !lifecycle.is_same(&process)? {
                    if let Some(replacement) = lifecycle.running()? {
                        process = replacement;
                    } else {
                        return Ok(LifecycleCompletion::Closed(0));
                    }
                }
            }
            signal = signals.recv() => {
                let exit_code = signal.unwrap_or(143);
                lifecycle.terminate().await?;
                return Ok(LifecycleCompletion::Closed(exit_code));
            }
            gateway_result = gateway.wait() => {
                let error = gateway_result.err().unwrap_or(HermesDesktopError::GatewayExited);
                lifecycle.terminate().await?;
                return Err(error);
            }
        }
    }
}

pub(super) async fn wait_for_relaunch(
    lifecycle: &impl DesktopLifecycle,
    gateway: &mut impl SupervisedGateway,
    signals: &mut tokio::sync::mpsc::UnboundedReceiver<i32>,
    mut interrupt_seen: bool,
    poll_interval: Duration,
    timeout: Duration,
) -> Result<RelaunchWaitCompletion, HermesDesktopError> {
    let started = Instant::now();
    loop {
        if let Some(process) = lifecycle.running()? {
            return Ok(RelaunchWaitCompletion::Running(process));
        }
        if started.elapsed() >= timeout {
            return Ok(RelaunchWaitCompletion::TimedOut);
        }
        tokio::select! {
            () = tokio::time::sleep(poll_interval) => {}
            signal = signals.recv() => {
                let code = signal.unwrap_or(143);
                if update_interrupt_requests_exit(code, &mut interrupt_seen) {
                    eprintln!("NaN is exiting before Hermes Desktop relaunches. Run `nanh hermes-desktop --restore` after the update finishes.");
                    return Ok(RelaunchWaitCompletion::PreserveRecovery(code));
                }
                eprintln!("Hermes has finished updating and is relaunching. Press Ctrl+C again to exit NaN and preserve recovery state.");
            }
            gateway_result = gateway.wait() => {
                return Err(gateway_result.err().unwrap_or(HermesDesktopError::GatewayExited));
            }
        }
    }
}
