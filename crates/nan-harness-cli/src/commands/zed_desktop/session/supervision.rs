use crate::commands::zed_desktop::ZedDesktopError;
use crate::commands::zed_desktop::process::SystemZedProcess;
use nan_harness_runtime::RunningChatCompletionsGateway;
use std::process::ExitStatus;
use std::time::{Duration, Instant};
use tokio::process::Child;

#[cfg(not(test))]
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(250);
#[cfg(test)]
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(5);
#[cfg(not(test))]
const QUIESCENCE_INTERVAL: Duration = Duration::from_secs(5);
#[cfg(test)]
const QUIESCENCE_INTERVAL: Duration = Duration::from_millis(25);

pub(super) trait ZedLifecycle {
    fn is_running(&self) -> Result<bool, ZedDesktopError>;

    async fn terminate_and_wait(&self) -> Result<(), ZedDesktopError>;
}

impl ZedLifecycle for SystemZedProcess {
    fn is_running(&self) -> Result<bool, ZedDesktopError> {
        SystemZedProcess::is_running(self)
    }

    async fn terminate_and_wait(&self) -> Result<(), ZedDesktopError> {
        SystemZedProcess::terminate_and_wait(self).await
    }
}

pub(super) trait GatewayLifecycle {
    async fn wait(&mut self) -> Result<(), ZedDesktopError>;
}

impl GatewayLifecycle for RunningChatCompletionsGateway {
    async fn wait(&mut self) -> Result<(), ZedDesktopError> {
        RunningChatCompletionsGateway::wait(self)
            .await
            .map_err(ZedDesktopError::Gateway)
    }
}

pub(super) async fn supervise(
    child: &mut Child,
    process: &impl ZedLifecycle,
    gateway: &mut impl GatewayLifecycle,
    signals: &mut tokio::sync::mpsc::UnboundedReceiver<i32>,
) -> Result<i32, ZedDesktopError> {
    let status = tokio::select! {
        status = child.wait() => status.map_err(ZedDesktopError::Wait)?,
        signal = signals.recv() => {
            let code = signal.unwrap_or(143);
            let _ = child.start_kill();
            process.terminate_and_wait().await?;
            return wait_for_quiescence(process, gateway, signals, code).await;
        }
        result = gateway.wait() => {
            let error = result.err().unwrap_or(ZedDesktopError::GatewayExited);
            let _ = child.start_kill();
            process.terminate_and_wait().await?;
            return Err(error);
        }
    };
    let code = exit_code(status);
    if code != 0 && !process.is_running()? {
        return Err(ZedDesktopError::DidNotStart);
    }
    wait_for_quiescence(process, gateway, signals, code).await
}

async fn wait_for_quiescence(
    process: &impl ZedLifecycle,
    gateway: &mut impl GatewayLifecycle,
    signals: &mut tokio::sync::mpsc::UnboundedReceiver<i32>,
    exit_code: i32,
) -> Result<i32, ZedDesktopError> {
    let mut quiet_since = None;
    loop {
        if process.is_running()? {
            quiet_since = None;
        } else {
            let since = quiet_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= QUIESCENCE_INTERVAL {
                return Ok(exit_code);
            }
        }
        tokio::select! {
            () = tokio::time::sleep(PROCESS_POLL_INTERVAL) => {}
            signal = signals.recv() => {
                let code = signal.unwrap_or(143);
                process.terminate_and_wait().await?;
                return Ok(code);
            }
            result = gateway.wait() => {
                let error = result.err().unwrap_or(ZedDesktopError::GatewayExited);
                if process.is_running()? {
                    process.terminate_and_wait().await?;
                }
                return Err(error);
            }
        }
    }
}

fn exit_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}

pub(super) fn termination_signals() -> tokio::sync::mpsc::UnboundedReceiver<i32> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            let Ok(mut interrupt) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
            else {
                return;
            };
            let Ok(mut terminate) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            else {
                return;
            };
            loop {
                tokio::select! {
                    value = interrupt.recv() => {
                        if value.is_none() || sender.send(130).is_err() { return; }
                    }
                    value = terminate.recv() => {
                        if value.is_none() || sender.send(143).is_err() { return; }
                    }
                }
            }
        }
        #[cfg(not(unix))]
        loop {
            if tokio::signal::ctrl_c().await.is_err() || sender.send(130).is_err() {
                return;
            }
        }
    });
    receiver
}

#[cfg(all(test, unix))]
mod tests {
    use super::{GatewayLifecycle, ZedLifecycle, supervise};
    use crate::commands::zed_desktop::ZedDesktopError;
    use std::collections::VecDeque;
    use std::future::pending;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::process::{Child, Command};
    use tokio::sync::mpsc;

    struct FakeProcess {
        running: Mutex<VecDeque<bool>>,
        running_checks: AtomicUsize,
        terminations: AtomicUsize,
    }

    impl FakeProcess {
        fn new(running: impl IntoIterator<Item = bool>) -> Self {
            Self {
                running: Mutex::new(running.into_iter().collect()),
                running_checks: AtomicUsize::new(0),
                terminations: AtomicUsize::new(0),
            }
        }
    }

    impl ZedLifecycle for FakeProcess {
        fn is_running(&self) -> Result<bool, ZedDesktopError> {
            self.running_checks.fetch_add(1, Ordering::Relaxed);
            Ok(self
                .running
                .lock()
                .expect("running sequence should not be poisoned")
                .pop_front()
                .unwrap_or(false))
        }

        async fn terminate_and_wait(&self) -> Result<(), ZedDesktopError> {
            self.terminations.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    enum FakeGateway {
        Pending,
        Exited,
    }

    impl GatewayLifecycle for FakeGateway {
        async fn wait(&mut self) -> Result<(), ZedDesktopError> {
            match self {
                Self::Pending => pending().await,
                Self::Exited => Ok(()),
            }
        }
    }

    #[tokio::test]
    async fn normal_close_waits_for_confirmed_process_quiescence() {
        let process = FakeProcess::new([]);
        let mut gateway = FakeGateway::Pending;
        let mut child = shell_child("exit 0");
        let (_sender, mut signals) = mpsc::unbounded_channel();

        let code = supervise(&mut child, &process, &mut gateway, &mut signals)
            .await
            .expect("normal close should succeed");

        assert_eq!(code, 0);
        assert!(process.running_checks.load(Ordering::Relaxed) >= 2);
        assert_eq!(process.terminations.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn failed_start_is_distinct_and_restorable() {
        let process = FakeProcess::new([false]);
        let mut gateway = FakeGateway::Pending;
        let mut child = shell_child("exit 7");
        let (_sender, mut signals) = mpsc::unbounded_channel();

        let error = supervise(&mut child, &process, &mut gateway, &mut signals)
            .await
            .expect_err("failed startup should be reported");

        assert!(matches!(error, ZedDesktopError::DidNotStart));
        assert_eq!(process.terminations.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn signal_terminates_zed_before_returning() {
        let process = FakeProcess::new([]);
        let mut gateway = FakeGateway::Pending;
        let mut child = shell_child("sleep 30");
        let (sender, mut signals) = mpsc::unbounded_channel();
        sender.send(130).expect("signal should queue");

        let code = supervise(&mut child, &process, &mut gateway, &mut signals)
            .await
            .expect("signal shutdown should finish");
        let _ = child.wait().await;

        assert_eq!(code, 130);
        assert_eq!(process.terminations.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn a_relaunch_resets_the_quiescence_window() {
        let process = FakeProcess::new([false, true, false]);
        let mut gateway = FakeGateway::Pending;
        let mut child = shell_child("exit 0");
        let (_sender, mut signals) = mpsc::unbounded_channel();

        let code = supervise(&mut child, &process, &mut gateway, &mut signals)
            .await
            .expect("relaunch should eventually quiesce");

        assert_eq!(code, 0);
        assert!(process.running_checks.load(Ordering::Relaxed) >= 6);
    }

    #[tokio::test]
    async fn gateway_exit_terminates_zed_and_preserves_the_failure() {
        let process = FakeProcess::new([]);
        let mut gateway = FakeGateway::Exited;
        let mut child = shell_child("sleep 30");
        let (_sender, mut signals) = mpsc::unbounded_channel();

        let error = supervise(&mut child, &process, &mut gateway, &mut signals)
            .await
            .expect_err("gateway exit should fail the session");
        let _ = child.wait().await;

        assert!(matches!(error, ZedDesktopError::GatewayExited));
        assert_eq!(process.terminations.load(Ordering::Relaxed), 1);
    }

    fn shell_child(script: &str) -> Child {
        Command::new("/bin/sh")
            .args(["-c", script])
            .spawn()
            .expect("test child should start")
    }
}
