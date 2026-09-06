use super::*;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc;

/// Zero makes the poll branch ready immediately; the long interval keeps it
/// pending so exactly one other branch of the select is ready per test.
const IMMEDIATE: Duration = Duration::ZERO;
const NEVER: Duration = Duration::from_hours(1);

struct FakeLifecycle {
    identities: Mutex<VecDeque<Result<bool, HermesDesktopError>>>,
    discoveries: Mutex<VecDeque<Result<Option<DesktopProcess>, HermesDesktopError>>>,
    checked: Mutex<Vec<DesktopProcess>>,
    terminations: AtomicUsize,
    termination_fails: bool,
}

impl FakeLifecycle {
    fn new() -> Self {
        Self {
            identities: Mutex::new(VecDeque::new()),
            discoveries: Mutex::new(VecDeque::new()),
            checked: Mutex::new(Vec::new()),
            terminations: AtomicUsize::new(0),
            termination_fails: false,
        }
    }

    fn identities(
        self,
        identities: impl IntoIterator<Item = Result<bool, HermesDesktopError>>,
    ) -> Self {
        *self.identities.lock().expect("identity script") = identities.into_iter().collect();
        self
    }

    fn discoveries(
        self,
        discoveries: impl IntoIterator<Item = Result<Option<DesktopProcess>, HermesDesktopError>>,
    ) -> Self {
        *self.discoveries.lock().expect("discovery script") = discoveries.into_iter().collect();
        self
    }

    fn failing_termination(mut self) -> Self {
        self.termination_fails = true;
        self
    }

    fn checked(&self) -> Vec<DesktopProcess> {
        self.checked.lock().expect("checked identities").clone()
    }

    fn terminations(&self) -> usize {
        self.terminations.load(Ordering::Relaxed)
    }
}

impl DesktopLifecycle for FakeLifecycle {
    fn running(&self) -> Result<Option<DesktopProcess>, HermesDesktopError> {
        self.discoveries
            .lock()
            .expect("discovery script")
            .pop_front()
            .unwrap_or(Ok(None))
    }

    fn is_same(&self, process: &DesktopProcess) -> Result<bool, HermesDesktopError> {
        self.checked
            .lock()
            .expect("checked identities")
            .push(process.clone());
        self.identities
            .lock()
            .expect("identity script")
            .pop_front()
            .unwrap_or(Ok(true))
    }

    async fn terminate(&self) -> Result<(), HermesDesktopError> {
        self.terminations.fetch_add(1, Ordering::Relaxed);
        if self.termination_fails {
            return Err(HermesDesktopError::DidNotTerminate);
        }
        Ok(())
    }
}

enum FakeGateway {
    Pending,
    Exited,
    Failed,
}

impl SupervisedGateway for FakeGateway {
    async fn wait(&mut self) -> Result<(), HermesDesktopError> {
        match self {
            Self::Pending => std::future::pending().await,
            Self::Exited => Ok(()),
            Self::Failed => Err(HermesDesktopError::BindGateway(std::io::Error::other(
                "gateway stopped",
            ))),
        }
    }
}

fn desktop(pid: u32) -> DesktopProcess {
    DesktopProcess {
        pid,
        started: format!("Mon Jan  1 00:00:0{pid} 2035"),
    }
}

#[tokio::test]
async fn supervision_follows_a_replacement_and_closes_only_when_both_are_gone() {
    let old = desktop(1);
    let replacement = desktop(2);
    let lifecycle = FakeLifecycle::new()
        .identities([Ok(true), Ok(true), Ok(false), Ok(true), Ok(false)])
        .discoveries([Ok(Some(replacement.clone())), Ok(None)]);
    let mut gateway = FakeGateway::Pending;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let completion = supervise_running(
        old.clone(),
        &lifecycle,
        &mut gateway,
        &mut signals,
        IMMEDIATE,
    )
    .await
    .expect("supervision should end when no desktop remains");

    assert_eq!(completion, LifecycleCompletion::Closed(0));
    assert_eq!(
        lifecycle.checked(),
        vec![
            old.clone(),
            old.clone(),
            old,
            replacement.clone(),
            replacement,
        ]
    );
    assert_eq!(lifecycle.terminations(), 0);
}

#[tokio::test]
async fn an_absent_gateway_stays_pending_while_supervising() {
    let lifecycle = FakeLifecycle::new()
        .identities([Ok(false)])
        .discoveries([Ok(None)]);
    let mut gateway: Option<&mut RunningChatCompletionsGateway> = None;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let completion = supervise_running(
        desktop(1),
        &lifecycle,
        &mut gateway,
        &mut signals,
        IMMEDIATE,
    )
    .await
    .expect("an absent gateway should not end supervision");

    assert_eq!(completion, LifecycleCompletion::Closed(0));
}

#[tokio::test]
async fn an_explicit_signal_terminates_the_desktop_once() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Pending;
    let (sender, mut signals) = mpsc::unbounded_channel();
    sender.send(130).expect("signal should queue");

    let completion = supervise_running(desktop(1), &lifecycle, &mut gateway, &mut signals, NEVER)
        .await
        .expect("a signal should close the session");

    assert_eq!(completion, LifecycleCompletion::Closed(130));
    assert_eq!(lifecycle.terminations(), 1);
}

#[tokio::test]
async fn a_closed_signal_channel_terminates_the_desktop_with_143() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Pending;
    let (sender, mut signals) = mpsc::unbounded_channel::<i32>();
    drop(sender);

    let completion = supervise_running(desktop(1), &lifecycle, &mut gateway, &mut signals, NEVER)
        .await
        .expect("a closed signal channel should close the session");

    assert_eq!(completion, LifecycleCompletion::Closed(143));
    assert_eq!(lifecycle.terminations(), 1);
}

#[tokio::test]
async fn a_signal_termination_failure_is_propagated() {
    let lifecycle = FakeLifecycle::new().failing_termination();
    let mut gateway = FakeGateway::Pending;
    let (sender, mut signals) = mpsc::unbounded_channel();
    sender.send(143).expect("signal should queue");

    let error = supervise_running(desktop(1), &lifecycle, &mut gateway, &mut signals, NEVER)
        .await
        .expect_err("a failed termination should be reported");

    assert!(matches!(error, HermesDesktopError::DidNotTerminate));
    assert_eq!(lifecycle.terminations(), 1);
}

#[tokio::test]
async fn a_normal_gateway_exit_terminates_the_desktop_and_reports_it_stopped() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Exited;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let error = supervise_running(desktop(1), &lifecycle, &mut gateway, &mut signals, NEVER)
        .await
        .expect_err("a gateway exit should fail the session");

    assert!(matches!(error, HermesDesktopError::GatewayExited));
    assert_eq!(lifecycle.terminations(), 1);
}

#[tokio::test]
async fn a_gateway_failure_is_preserved_and_still_terminates_the_desktop() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Failed;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let error = supervise_running(desktop(1), &lifecycle, &mut gateway, &mut signals, NEVER)
        .await
        .expect_err("a gateway failure should fail the session");

    assert!(matches!(error, HermesDesktopError::BindGateway(_)));
    assert_eq!(lifecycle.terminations(), 1);
}

#[tokio::test]
async fn a_termination_failure_outranks_the_gateway_failure() {
    let lifecycle = FakeLifecycle::new().failing_termination();
    let mut gateway = FakeGateway::Failed;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let error = supervise_running(desktop(1), &lifecycle, &mut gateway, &mut signals, NEVER)
        .await
        .expect_err("a failed termination should be reported");

    assert!(matches!(error, HermesDesktopError::DidNotTerminate));
}

#[tokio::test]
async fn an_identity_error_propagates_without_terminating() {
    let lifecycle =
        FakeLifecycle::new().identities([Err(HermesDesktopError::ProcessCheckFailed(Some(2)))]);
    let mut gateway = FakeGateway::Pending;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let error = supervise_running(
        desktop(1),
        &lifecycle,
        &mut gateway,
        &mut signals,
        IMMEDIATE,
    )
    .await
    .expect_err("an identity error should be reported");

    assert!(matches!(
        error,
        HermesDesktopError::ProcessCheckFailed(Some(2))
    ));
    assert_eq!(lifecycle.terminations(), 0);
}

#[tokio::test]
async fn a_replacement_discovery_error_propagates_without_terminating() {
    let lifecycle = FakeLifecycle::new()
        .identities([Ok(false)])
        .discoveries([Err(HermesDesktopError::ProcessCheckFailed(Some(3)))]);
    let mut gateway = FakeGateway::Pending;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let error = supervise_running(
        desktop(1),
        &lifecycle,
        &mut gateway,
        &mut signals,
        IMMEDIATE,
    )
    .await
    .expect_err("a discovery error should be reported");

    assert!(matches!(
        error,
        HermesDesktopError::ProcessCheckFailed(Some(3))
    ));
    assert_eq!(lifecycle.terminations(), 0);
}

#[tokio::test]
async fn a_present_process_is_returned_before_the_timeout_is_consulted() {
    let process = desktop(7);
    let lifecycle = FakeLifecycle::new().discoveries([Ok(Some(process.clone()))]);
    let mut gateway = FakeGateway::Pending;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let completion = wait_for_relaunch(
        &lifecycle,
        &mut gateway,
        &mut signals,
        false,
        NEVER,
        IMMEDIATE,
    )
    .await
    .expect("an already running desktop should be returned");

    assert_eq!(completion, RelaunchWaitCompletion::Running(process));
}

#[tokio::test]
async fn no_process_within_a_zero_timeout_times_out_without_waiting() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Pending;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let completion = wait_for_relaunch(
        &lifecycle,
        &mut gateway,
        &mut signals,
        false,
        NEVER,
        IMMEDIATE,
    )
    .await
    .expect("an exhausted timeout should be reported");

    assert_eq!(completion, RelaunchWaitCompletion::TimedOut);
}

#[tokio::test]
async fn a_pending_relaunch_can_still_find_the_process() {
    let process = desktop(8);
    let lifecycle =
        FakeLifecycle::new().discoveries([Ok(None), Ok(None), Ok(Some(process.clone()))]);
    let mut gateway = FakeGateway::Pending;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let completion = wait_for_relaunch(
        &lifecycle,
        &mut gateway,
        &mut signals,
        false,
        IMMEDIATE,
        NEVER,
    )
    .await
    .expect("a later relaunch should be observed");

    assert_eq!(completion, RelaunchWaitCompletion::Running(process));
}

#[tokio::test]
async fn the_first_relaunch_interrupt_continues_and_the_second_preserves_recovery() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Pending;
    let (sender, mut signals) = mpsc::unbounded_channel();
    sender.send(130).expect("first interrupt should queue");
    sender.send(130).expect("second interrupt should queue");

    let completion = wait_for_relaunch(&lifecycle, &mut gateway, &mut signals, false, NEVER, NEVER)
        .await
        .expect("a second interrupt should preserve recovery");

    assert_eq!(completion, RelaunchWaitCompletion::PreserveRecovery(130));
    assert_eq!(lifecycle.terminations(), 0);
}

#[tokio::test]
async fn an_interrupt_after_an_earlier_one_exits_immediately() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Pending;
    let (sender, mut signals) = mpsc::unbounded_channel();
    sender.send(130).expect("interrupt should queue");

    let completion = wait_for_relaunch(&lifecycle, &mut gateway, &mut signals, true, NEVER, NEVER)
        .await
        .expect("an already seen interrupt should preserve recovery");

    assert_eq!(completion, RelaunchWaitCompletion::PreserveRecovery(130));
}

#[tokio::test]
async fn a_termination_signal_during_relaunch_preserves_recovery() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Pending;
    let (sender, mut signals) = mpsc::unbounded_channel();
    sender.send(143).expect("signal should queue");

    let completion = wait_for_relaunch(&lifecycle, &mut gateway, &mut signals, false, NEVER, NEVER)
        .await
        .expect("a termination signal should preserve recovery");

    assert_eq!(completion, RelaunchWaitCompletion::PreserveRecovery(143));
}

#[tokio::test]
async fn a_closed_signal_channel_during_relaunch_preserves_recovery_with_143() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Pending;
    let (sender, mut signals) = mpsc::unbounded_channel::<i32>();
    drop(sender);

    let completion = wait_for_relaunch(&lifecycle, &mut gateway, &mut signals, false, NEVER, NEVER)
        .await
        .expect("a closed signal channel should preserve recovery");

    assert_eq!(completion, RelaunchWaitCompletion::PreserveRecovery(143));
}

#[tokio::test]
async fn a_relaunch_discovery_error_propagates() {
    let lifecycle =
        FakeLifecycle::new().discoveries([Err(HermesDesktopError::ProcessCheckFailed(Some(4)))]);
    let mut gateway = FakeGateway::Pending;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let error = wait_for_relaunch(&lifecycle, &mut gateway, &mut signals, false, NEVER, NEVER)
        .await
        .expect_err("a discovery error should be reported");

    assert!(matches!(
        error,
        HermesDesktopError::ProcessCheckFailed(Some(4))
    ));
}

#[tokio::test]
async fn a_gateway_exit_during_relaunch_does_not_terminate_the_desktop() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Exited;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let error = wait_for_relaunch(&lifecycle, &mut gateway, &mut signals, false, NEVER, NEVER)
        .await
        .expect_err("a gateway exit should fail the relaunch wait");

    assert!(matches!(error, HermesDesktopError::GatewayExited));
    assert_eq!(lifecycle.terminations(), 0);
}

#[tokio::test]
async fn a_gateway_failure_during_relaunch_is_preserved() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Failed;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let error = wait_for_relaunch(&lifecycle, &mut gateway, &mut signals, false, NEVER, NEVER)
        .await
        .expect_err("a gateway failure should fail the relaunch wait");

    assert!(matches!(error, HermesDesktopError::BindGateway(_)));
    assert_eq!(lifecycle.terminations(), 0);
}
