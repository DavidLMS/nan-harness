use crate::commands::chatgpt_desktop::ChatGptDesktopError;
use crate::commands::chatgpt_desktop::process::{SupervisedApp, supervise_startup};
use crate::commands::chatgpt_desktop::startup::{STARTUP_GRACE, StartupPolicy, StartupWatch};
use nan_harness_runtime::{
    BridgeActivity, BridgeDiagnostic, BridgeDiagnosticReason, BridgeEndpoint, CancellationToken,
    SignalKind,
};
use std::future::pending;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::{broadcast, oneshot};

struct Controls {
    exit: oneshot::Sender<i32>,
    bridge_stop: oneshot::Sender<()>,
    authentication: broadcast::Sender<BridgeActivity>,
    diagnostics: UnboundedSender<BridgeDiagnostic>,
    stops: Arc<AtomicUsize>,
    cancellation: CancellationToken,
}

struct Sources {
    app: FakeApp,
    bridge_stop: oneshot::Receiver<()>,
    authentication: broadcast::Receiver<BridgeActivity>,
    diagnostics: UnboundedReceiver<BridgeDiagnostic>,
    cancellation: CancellationToken,
}

fn supervised() -> (Controls, Sources) {
    let (exit, exit_receiver) = oneshot::channel();
    let (bridge_stop, bridge_stop_receiver) = oneshot::channel();
    let (authentication, authentication_receiver) = broadcast::channel(16);
    let (diagnostics, diagnostics_receiver) = unbounded_channel();
    let stops = Arc::new(AtomicUsize::new(0));
    let cancellation = CancellationToken::new();
    (
        Controls {
            exit,
            bridge_stop,
            authentication,
            diagnostics,
            stops: Arc::clone(&stops),
            cancellation: cancellation.clone(),
        },
        Sources {
            app: FakeApp {
                exit: exit_receiver,
                stops,
            },
            bridge_stop: bridge_stop_receiver,
            authentication: authentication_receiver,
            diagnostics: diagnostics_receiver,
            cancellation,
        },
    )
}

fn spawn_supervision(
    sources: Sources,
    policy: StartupPolicy,
) -> tokio::task::JoinHandle<(Result<i32, ChatGptDesktopError>, Vec<BridgeDiagnostic>)> {
    tokio::spawn(async move {
        let Sources {
            mut app,
            bridge_stop,
            mut authentication,
            diagnostics: mut diagnostic_receiver,
            cancellation,
        } = sources;
        let mut diagnostics = Vec::new();
        let bridge_stopped = async {
            match bridge_stop.await {
                Ok(()) => ChatGptDesktopError::BridgeExited,
                Err(_) => pending().await,
            }
        };
        let result = supervise_startup(
            &mut app,
            bridge_stopped,
            &mut authentication,
            &mut diagnostic_receiver,
            &mut StartupWatch::new(policy),
            &cancellation,
            &mut diagnostics,
        )
        .await;
        (result, diagnostics)
    })
}

struct FakeApp {
    exit: oneshot::Receiver<i32>,
    stops: Arc<AtomicUsize>,
}

impl SupervisedApp for FakeApp {
    async fn wait(&mut self) -> Result<i32, ChatGptDesktopError> {
        match (&mut self.exit).await {
            Ok(code) => Ok(code),
            // No exit was scheduled: the app keeps running.
            Err(_) => pending().await,
        }
    }

    async fn stop(&mut self) -> Result<(), ChatGptDesktopError> {
        self.stops.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn diagnostic(code: &'static str) -> BridgeDiagnostic {
    BridgeDiagnostic {
        code,
        reason: BridgeDiagnosticReason::AuthenticationRejected,
        http_status: Some(401),
        endpoint: BridgeEndpoint::Responses,
        model_id: None,
        requested_reasoning: None,
        model_policy: None,
        timeout_phase: None,
        recovery_outcome: None,
        attempt: None,
        priority: None,
        cache_replay_detected: None,
        cache_bypass_attempted: None,
    }
}

#[tokio::test(start_paused = true)]
async fn an_interactive_launch_survives_authentication_long_after_the_grace_period() {
    let (controls, sources) = supervised();
    let supervision = spawn_supervision(sources, StartupPolicy::resolve(None, true));

    tokio::time::sleep(STARTUP_GRACE * 8).await;
    controls
        .authentication
        .send(BridgeActivity::AuthenticatedClient)
        .expect("the supervised launch should still be waiting");
    tokio::time::sleep(STARTUP_GRACE * 8).await;
    controls
        .exit
        .send(0)
        .expect("the app should still be alive");

    let (result, _) = supervision.await.expect("supervision should finish");
    assert_eq!(result.expect("a late handshake should still succeed"), 0);
    assert_eq!(
        controls.stops.load(Ordering::SeqCst),
        0,
        "an interactive launch must never kill the app on elapsed time"
    );
}

#[tokio::test(start_paused = true)]
async fn a_noninteractive_launch_fails_at_the_grace_period_and_stops_the_app() {
    let (controls, sources) = supervised();
    let supervision = spawn_supervision(sources, StartupPolicy::resolve(None, false));

    let (result, _) = supervision.await.expect("supervision should finish");
    assert!(matches!(
        result,
        Err(ChatGptDesktopError::BridgeHandshakeTimeout)
    ));
    assert_eq!(controls.stops.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn authentication_disarms_an_explicit_deadline() {
    let (controls, sources) = supervised();
    let supervision = spawn_supervision(sources, StartupPolicy::resolve(Some(STARTUP_GRACE), true));
    controls
        .authentication
        .send(BridgeActivity::AuthenticatedClient)
        .expect("activity receiver");
    tokio::time::sleep(STARTUP_GRACE * 8).await;
    controls
        .exit
        .send(0)
        .expect("authentication must keep the app alive");
    let (result, _) = supervision.await.expect("supervision task");
    assert_eq!(result.expect("app exit"), 0);
    assert_eq!(controls.stops.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn closed_bridge_channels_do_not_prevent_cancellation() {
    let (controls, sources) = supervised();
    let supervision = spawn_supervision(sources, StartupPolicy::resolve(None, true));
    drop(controls.authentication);
    drop(controls.diagnostics);
    tokio::time::sleep(STARTUP_GRACE * 2).await;
    controls.cancellation.cancel(SignalKind::Interrupt);
    let (result, _) = supervision.await.expect("supervision task");
    assert_eq!(
        result.expect("cancelled launch"),
        SignalKind::Interrupt.exit_code()
    );
    assert_eq!(controls.stops.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn an_explicit_timeout_bounds_an_interactive_launch() {
    let timeout = STARTUP_GRACE * 4;
    let (controls, sources) = supervised();
    let started = tokio::time::Instant::now();
    let supervision = spawn_supervision(sources, StartupPolicy::resolve(Some(timeout), true));

    let (result, _) = supervision.await.expect("supervision should finish");
    assert!(matches!(
        result,
        Err(ChatGptDesktopError::BridgeHandshakeTimeout)
    ));
    assert!(started.elapsed() >= timeout);
    assert_eq!(controls.stops.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn cancellation_while_waiting_stops_the_app_and_reports_the_signal() {
    let (controls, sources) = supervised();
    let supervision = spawn_supervision(sources, StartupPolicy::resolve(None, true));

    tokio::time::sleep(STARTUP_GRACE * 2).await;
    controls.cancellation.cancel(SignalKind::Interrupt);

    let (result, _) = supervision.await.expect("supervision should finish");
    assert_eq!(
        result.expect("cancellation is not a launch failure"),
        SignalKind::Interrupt.exit_code()
    );
    assert_eq!(controls.stops.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn a_stopped_bridge_fails_the_launch_while_waiting() {
    let (controls, sources) = supervised();
    let supervision = spawn_supervision(sources, StartupPolicy::resolve(None, true));

    tokio::time::sleep(STARTUP_GRACE * 2).await;
    controls
        .bridge_stop
        .send(())
        .expect("the supervised launch should still be waiting");

    let (result, _) = supervision.await.expect("supervision should finish");
    assert!(matches!(result, Err(ChatGptDesktopError::BridgeExited)));
    assert_eq!(controls.stops.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn an_app_exit_while_waiting_ends_supervision_with_its_exit_code() {
    let (controls, sources) = supervised();
    let supervision = spawn_supervision(sources, StartupPolicy::resolve(None, true));

    tokio::time::sleep(STARTUP_GRACE * 2).await;
    controls
        .exit
        .send(3)
        .expect("the app should still be alive");

    let (result, _) = supervision.await.expect("supervision should finish");
    assert_eq!(result.expect("an app exit is not a launch failure"), 3);
    assert_eq!(controls.stops.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn diagnostics_are_collected_once_while_waiting_and_when_the_app_exits() {
    let (controls, sources) = supervised();
    let supervision = spawn_supervision(sources, StartupPolicy::resolve(None, true));

    controls
        .diagnostics
        .send(diagnostic("NH-BRIDGE-001"))
        .expect("the diagnostic stream should be open");
    tokio::time::sleep(STARTUP_GRACE * 2).await;
    controls
        .diagnostics
        .send(diagnostic("NH-BRIDGE-001"))
        .expect("the diagnostic stream should be open");
    controls
        .diagnostics
        .send(diagnostic("NH-BRIDGE-002"))
        .expect("the diagnostic stream should be open");
    controls
        .exit
        .send(0)
        .expect("the app should still be alive");

    let (result, diagnostics) = supervision.await.expect("supervision should finish");
    assert_eq!(result.expect("the app exited normally"), 0);
    let codes: Vec<&str> = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert_eq!(codes, ["NH-BRIDGE-001", "NH-BRIDGE-002"]);
}
