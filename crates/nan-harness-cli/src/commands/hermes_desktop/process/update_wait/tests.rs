use super::*;
use std::collections::VecDeque;
use std::future::Future;
use std::sync::Mutex;
use tokio::sync::mpsc;

/// Zero makes a deadline or the poll branch ready immediately; the long
/// duration keeps it pending so exactly one branch of the select is ready.
const IMMEDIATE: Duration = Duration::ZERO;
const NEVER: Duration = Duration::from_hours(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Observation {
    Marker,
    Owner,
}

struct FakeUpdateState {
    markers: Mutex<VecDeque<bool>>,
    owners: Mutex<VecDeque<Result<bool, HermesDesktopError>>>,
    observations: Mutex<Vec<Observation>>,
}

impl FakeUpdateState {
    /// An exhausted script keeps reporting an updating app so a wait driven by
    /// signals or by the gateway can run as long as the test needs.
    fn new() -> Self {
        Self {
            markers: Mutex::new(VecDeque::new()),
            owners: Mutex::new(VecDeque::new()),
            observations: Mutex::new(Vec::new()),
        }
    }

    fn markers(self, markers: impl IntoIterator<Item = bool>) -> Self {
        *self.markers.lock().expect("marker script") = markers.into_iter().collect();
        self
    }

    fn owners(self, owners: impl IntoIterator<Item = Result<bool, HermesDesktopError>>) -> Self {
        *self.owners.lock().expect("owner script") = owners.into_iter().collect();
        self
    }

    fn observations(&self) -> Vec<Observation> {
        self.observations.lock().expect("observations").clone()
    }
}

impl UpdateState for FakeUpdateState {
    fn marker_exists(&self) -> bool {
        self.observations
            .lock()
            .expect("observations")
            .push(Observation::Marker);
        self.markers
            .lock()
            .expect("marker script")
            .pop_front()
            .unwrap_or(true)
    }

    fn live_owner_present(&self) -> Result<bool, HermesDesktopError> {
        self.observations
            .lock()
            .expect("observations")
            .push(Observation::Owner);
        self.owners
            .lock()
            .expect("owner script")
            .pop_front()
            .unwrap_or(Ok(true))
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

fn timing(
    poll_interval: Duration,
    total_timeout: Duration,
    stale_grace: Duration,
) -> UpdateWaitTiming {
    UpdateWaitTiming {
        poll_interval,
        total_timeout,
        stale_grace,
    }
}

fn unreadable_marker() -> HermesDesktopError {
    HermesDesktopError::ReadUpdateMarker(std::io::Error::other("marker unreadable"))
}

async fn bounded<T>(operation: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(2), operation)
        .await
        .expect("update wait policy should complete within the test deadline")
}

#[tokio::test]
async fn an_absent_marker_finishes_without_consulting_the_owner() {
    let state = FakeUpdateState::new().markers([false]);
    let mut gateway = FakeGateway::Pending;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let completion = bounded(wait_for_update(
        &state,
        &mut gateway,
        &mut signals,
        timing(NEVER, NEVER, NEVER),
    ))
    .await
    .expect("an absent marker should finish the wait");

    assert_eq!(
        completion,
        UpdateWaitCompletion::Finished {
            interrupt_seen: false
        }
    );
    assert_eq!(state.observations(), vec![Observation::Marker]);
}

#[tokio::test]
async fn a_live_owner_waits_until_the_marker_disappears() {
    let state = FakeUpdateState::new()
        .markers([true, false])
        .owners([Ok(true)]);
    let mut gateway = FakeGateway::Pending;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let completion = bounded(wait_for_update(
        &state,
        &mut gateway,
        &mut signals,
        timing(IMMEDIATE, NEVER, NEVER),
    ))
    .await
    .expect("a removed marker should finish the wait");

    assert_eq!(
        completion,
        UpdateWaitCompletion::Finished {
            interrupt_seen: false
        }
    );
    assert_eq!(
        state.observations(),
        vec![Observation::Marker, Observation::Owner, Observation::Marker]
    );
}

#[tokio::test]
async fn a_stale_marker_finishes_before_an_exhausted_total_timeout_is_consulted() {
    let state = FakeUpdateState::new().markers([true]).owners([Ok(false)]);
    let mut gateway = FakeGateway::Pending;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let completion = bounded(wait_for_update(
        &state,
        &mut gateway,
        &mut signals,
        timing(NEVER, IMMEDIATE, IMMEDIATE),
    ))
    .await
    .expect("a reached stale grace should finish the wait, not time it out");

    assert_eq!(
        completion,
        UpdateWaitCompletion::Finished {
            interrupt_seen: false
        }
    );
    assert_eq!(
        state.observations(),
        vec![Observation::Marker, Observation::Owner]
    );
}

#[tokio::test]
async fn a_live_owner_at_an_exhausted_total_timeout_reports_the_update_timeout() {
    let state = FakeUpdateState::new().markers([true]).owners([Ok(true)]);
    let mut gateway = FakeGateway::Pending;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let error = bounded(wait_for_update(
        &state,
        &mut gateway,
        &mut signals,
        timing(NEVER, IMMEDIATE, NEVER),
    ))
    .await
    .expect_err("an exhausted total timeout should fail the wait");

    assert!(matches!(error, HermesDesktopError::UpdateTimedOut));
    assert_eq!(
        state.observations(),
        vec![Observation::Marker, Observation::Owner]
    );
}

#[tokio::test]
async fn an_owner_lookup_error_propagates() {
    let state = FakeUpdateState::new()
        .markers([true])
        .owners([Err(unreadable_marker())]);
    let mut gateway = FakeGateway::Pending;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let error = bounded(wait_for_update(
        &state,
        &mut gateway,
        &mut signals,
        timing(NEVER, NEVER, NEVER),
    ))
    .await
    .expect_err("an owner lookup error should be reported");

    assert!(matches!(error, HermesDesktopError::ReadUpdateMarker(_)));
    assert_eq!(
        state.observations(),
        vec![Observation::Marker, Observation::Owner]
    );
}

/// A returning owner takes the branch that clears the recorded stale instant.
/// The grace is never reached here, so this asserts the observed sequence only,
/// not how long a restarted grace lasts.
#[tokio::test]
async fn a_returning_owner_keeps_an_ownerless_marker_waiting() {
    let state = FakeUpdateState::new()
        .markers([true, true, true, false])
        .owners([Ok(false), Ok(true), Ok(false)]);
    let mut gateway = FakeGateway::Pending;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let completion = bounded(wait_for_update(
        &state,
        &mut gateway,
        &mut signals,
        timing(IMMEDIATE, NEVER, NEVER),
    ))
    .await
    .expect("the wait should continue until the marker disappears");

    assert_eq!(
        completion,
        UpdateWaitCompletion::Finished {
            interrupt_seen: false
        }
    );
    assert_eq!(
        state.observations(),
        vec![
            Observation::Marker,
            Observation::Owner,
            Observation::Marker,
            Observation::Owner,
            Observation::Marker,
            Observation::Owner,
            Observation::Marker,
        ]
    );
}

#[tokio::test]
async fn a_first_interrupt_keeps_waiting_and_is_reported_when_the_update_finishes() {
    let state = FakeUpdateState::new()
        .markers([true, false])
        .owners([Ok(true)]);
    let mut gateway = FakeGateway::Pending;
    let (sender, mut signals) = mpsc::unbounded_channel();
    sender.send(130).expect("interrupt should queue");

    let completion = bounded(wait_for_update(
        &state,
        &mut gateway,
        &mut signals,
        timing(NEVER, NEVER, NEVER),
    ))
    .await
    .expect("a first interrupt should not end the wait");

    assert_eq!(
        completion,
        UpdateWaitCompletion::Finished {
            interrupt_seen: true
        }
    );
    assert_eq!(
        state.observations(),
        vec![Observation::Marker, Observation::Owner, Observation::Marker]
    );
}

#[tokio::test]
async fn a_second_interrupt_preserves_recovery() {
    let state = FakeUpdateState::new();
    let mut gateway = FakeGateway::Pending;
    let (sender, mut signals) = mpsc::unbounded_channel();
    sender.send(130).expect("first interrupt should queue");
    sender.send(130).expect("second interrupt should queue");

    let completion = bounded(wait_for_update(
        &state,
        &mut gateway,
        &mut signals,
        timing(NEVER, NEVER, NEVER),
    ))
    .await
    .expect("a second interrupt should preserve recovery");

    assert_eq!(completion, UpdateWaitCompletion::PreserveRecovery(130));
}

#[tokio::test]
async fn a_termination_signal_preserves_recovery_immediately() {
    let state = FakeUpdateState::new();
    let mut gateway = FakeGateway::Pending;
    let (sender, mut signals) = mpsc::unbounded_channel();
    sender.send(143).expect("signal should queue");

    let completion = bounded(wait_for_update(
        &state,
        &mut gateway,
        &mut signals,
        timing(NEVER, NEVER, NEVER),
    ))
    .await
    .expect("a termination signal should preserve recovery");

    assert_eq!(completion, UpdateWaitCompletion::PreserveRecovery(143));
}

#[tokio::test]
async fn a_closed_signal_channel_preserves_recovery_with_143() {
    let state = FakeUpdateState::new();
    let mut gateway = FakeGateway::Pending;
    let (sender, mut signals) = mpsc::unbounded_channel::<i32>();
    drop(sender);

    let completion = bounded(wait_for_update(
        &state,
        &mut gateway,
        &mut signals,
        timing(NEVER, NEVER, NEVER),
    ))
    .await
    .expect("a closed signal channel should preserve recovery");

    assert_eq!(completion, UpdateWaitCompletion::PreserveRecovery(143));
}

#[tokio::test]
async fn a_gateway_exit_fails_the_update_wait() {
    let state = FakeUpdateState::new();
    let mut gateway = FakeGateway::Exited;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let error = bounded(wait_for_update(
        &state,
        &mut gateway,
        &mut signals,
        timing(NEVER, NEVER, NEVER),
    ))
    .await
    .expect_err("a gateway exit should fail the wait");

    assert!(matches!(error, HermesDesktopError::GatewayExited));
}

#[tokio::test]
async fn a_gateway_failure_is_preserved() {
    let state = FakeUpdateState::new();
    let mut gateway = FakeGateway::Failed;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let error = bounded(wait_for_update(
        &state,
        &mut gateway,
        &mut signals,
        timing(NEVER, NEVER, NEVER),
    ))
    .await
    .expect_err("a gateway failure should fail the wait");

    assert!(matches!(error, HermesDesktopError::BindGateway(_)));
}
