use super::installation::ChatGptInstallation;
use super::platform::{chatgpt_is_running, request_quit};
use super::startup::{StartupPolicy, StartupSignal, StartupWatch, print_startup_notice};
use super::{ChatGptDesktopError, SESSION_TOKEN_ENVIRONMENT, SHUTDOWN_GRACE};
use nan_harness_runtime::{
    BridgeActivity, BridgeDiagnostic, CodexDesktopBridgeError, RunningCodexDesktopBridge,
};
use std::future::pending;
use std::process::{ExitStatus, Stdio};
use tokio::process::{Child, Command};
use tokio::sync::broadcast::error::RecvError;

use super::profile::ManagedProfile;

/// The supervised application process.
///
/// The trait keeps the supervision loop testable without launching the real
/// app. Every implementation of [`SupervisedApp::wait`] must be cancel safe,
/// because the loop polls it from a `select!` arm.
pub(super) trait SupervisedApp {
    async fn wait(&mut self) -> Result<i32, ChatGptDesktopError>;
    async fn stop(&mut self) -> Result<(), ChatGptDesktopError>;
}

/// The bridge's own lifetime. The bridge stopping is always a launch failure,
/// so the future resolves to the error that describes it.
pub(super) trait BridgeLifetime {
    async fn wait_for_stop(&mut self) -> ChatGptDesktopError;
}

/// Authentication of the app to the managed bridge, which ends the startup
/// wait. The future must never resolve when authentication can no longer be
/// reported, so that process supervision continues instead of spinning.
pub(super) trait ClientAuthentication {
    async fn wait_for_authentication(&mut self);
}

/// The bridge's diagnostic stream.
pub(super) trait DiagnosticSource {
    async fn next(&mut self) -> BridgeDiagnostic;
    fn drain_into(&mut self, collected: &mut Vec<BridgeDiagnostic>);
}

/// The bridge-side signals of one managed launch, held as separate fields so
/// that the supervision loop can await all of them concurrently.
pub(super) struct BridgeSignals<L, C, D> {
    pub(super) lifetime: L,
    pub(super) authentication: C,
    pub(super) diagnostics: D,
}

pub(super) async fn supervise_desktop(
    installation: &ChatGptInstallation,
    profile: &ManagedProfile,
    bridge: &mut RunningCodexDesktopBridge,
    debug: bool,
    policy: StartupPolicy,
    cancellation: &nan_harness_runtime::CancellationToken,
    diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<i32, ChatGptDesktopError> {
    let mut command = Command::new(&installation.executable);
    let activities = bridge.subscribe_activities();
    command
        .env("CODEX_HOME", &profile.root)
        .env_remove("CODEX_API_KEY")
        .env_remove("NAN_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_CI")
        .env_remove("CODEX_THREAD_ID")
        .kill_on_drop(true);
    bridge.with_session_token(|token| {
        command.env(SESSION_TOKEN_ENVIRONMENT, token);
    });
    if debug {
        command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    } else {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let mut child = command.spawn().map_err(ChatGptDesktopError::StartApp)?;
    detect_singleton_race(&mut child).await?;
    let diagnostic_receiver = bridge.take_diagnostics();
    let mut app = DesktopApp { child };
    let mut signals = BridgeSignals {
        lifetime: BridgeProcess(bridge),
        authentication: ActivityAuthentication(activities),
        diagnostics: DiagnosticChannel(diagnostic_receiver),
    };
    supervise_startup(
        &mut app,
        &mut signals,
        &mut StartupWatch::new(policy),
        cancellation,
        diagnostics,
    )
    .await
}

/// Supervises one managed launch until the app exits, the user cancels, the
/// bridge stops, or the startup deadline elapses.
///
/// The startup wait never kills a running app on elapsed time unless the
/// policy carries a deadline; every exit path stops the app it started.
pub(super) async fn supervise_startup<A, L, C, D>(
    app: &mut A,
    signals: &mut BridgeSignals<L, C, D>,
    watch: &mut StartupWatch,
    cancellation: &nan_harness_runtime::CancellationToken,
    diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<i32, ChatGptDesktopError>
where
    A: SupervisedApp,
    L: BridgeLifetime,
    C: ClientAuthentication,
    D: DiagnosticSource,
{
    let mut authenticated = false;
    loop {
        tokio::select! {
            exit_code = app.wait() => {
                signals.diagnostics.drain_into(diagnostics);
                return exit_code;
            }
            signal = cancellation.cancelled() => {
                app.stop().await?;
                signals.diagnostics.drain_into(diagnostics);
                return Ok(signal.exit_code());
            }
            error = signals.lifetime.wait_for_stop() => {
                app.stop().await?;
                signals.diagnostics.drain_into(diagnostics);
                return Err(error);
            }
            diagnostic = signals.diagnostics.next() => {
                if !diagnostics.contains(&diagnostic) {
                    diagnostics.push(diagnostic);
                }
            }
            () = signals.authentication.wait_for_authentication(), if !authenticated => {
                authenticated = true;
            }
            startup = watch.signal(), if !authenticated => {
                match startup {
                    StartupSignal::Notice => print_startup_notice(),
                    StartupSignal::TimedOut => {
                        app.stop().await?;
                        signals.diagnostics.drain_into(diagnostics);
                        return Err(ChatGptDesktopError::BridgeHandshakeTimeout);
                    }
                }
            }
        }
    }
}

struct DesktopApp {
    child: Child,
}

impl SupervisedApp for DesktopApp {
    async fn wait(&mut self) -> Result<i32, ChatGptDesktopError> {
        self.child
            .wait()
            .await
            .map(exit_code)
            .map_err(ChatGptDesktopError::WaitForApp)
    }

    async fn stop(&mut self) -> Result<(), ChatGptDesktopError> {
        stop_chatgpt(&mut self.child).await
    }
}

struct BridgeProcess<'bridge>(&'bridge mut RunningCodexDesktopBridge);

impl BridgeLifetime for BridgeProcess<'_> {
    async fn wait_for_stop(&mut self) -> ChatGptDesktopError {
        match self.0.wait().await {
            Ok(()) => ChatGptDesktopError::BridgeExited,
            Err(error) => ChatGptDesktopError::Bridge(CodexDesktopBridgeError::Bridge(error)),
        }
    }
}

struct ActivityAuthentication(tokio::sync::broadcast::Receiver<BridgeActivity>);

impl ClientAuthentication for ActivityAuthentication {
    async fn wait_for_authentication(&mut self) {
        loop {
            match self.0.recv().await {
                Ok(BridgeActivity::AuthenticatedClient) => return,
                Ok(_) | Err(RecvError::Lagged(_)) => (),
                // A closed activity stream can never report authentication, so
                // the launch keeps supervising the app instead of spinning.
                Err(RecvError::Closed) => pending().await,
            }
        }
    }
}

struct DiagnosticChannel(tokio::sync::mpsc::UnboundedReceiver<BridgeDiagnostic>);

impl DiagnosticSource for DiagnosticChannel {
    async fn next(&mut self) -> BridgeDiagnostic {
        match self.0.recv().await {
            Some(diagnostic) => diagnostic,
            // A closed diagnostic channel has nothing left to report.
            None => pending().await,
        }
    }

    fn drain_into(&mut self, collected: &mut Vec<BridgeDiagnostic>) {
        while let Ok(diagnostic) = self.0.try_recv() {
            if !collected.contains(&diagnostic) {
                collected.push(diagnostic);
            }
        }
    }
}

async fn detect_singleton_race(child: &mut Child) -> Result<(), ChatGptDesktopError> {
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    if let Some(status) = child.try_wait().map_err(ChatGptDesktopError::WaitForApp)? {
        return Err(classify_early_exit(status.success(), chatgpt_is_running()?));
    }
    Ok(())
}

pub(super) const fn classify_early_exit(success: bool, app_running: bool) -> ChatGptDesktopError {
    if success && app_running {
        ChatGptDesktopError::SingletonRace
    } else {
        ChatGptDesktopError::AppExitedDuringStartup
    }
}

async fn stop_chatgpt(child: &mut Child) -> Result<(), ChatGptDesktopError> {
    request_quit().await;
    match tokio::time::timeout(SHUTDOWN_GRACE, child.wait()).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(error)) => Err(ChatGptDesktopError::WaitForApp(error)),
        Err(_) => match child.kill().await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
            Err(error) => Err(ChatGptDesktopError::StopApp(error)),
        },
    }
}

pub(super) fn require_app_stopped() -> Result<(), ChatGptDesktopError> {
    if chatgpt_is_running()? {
        Err(ChatGptDesktopError::AppAlreadyRunning)
    } else {
        Ok(())
    }
}

fn exit_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}
