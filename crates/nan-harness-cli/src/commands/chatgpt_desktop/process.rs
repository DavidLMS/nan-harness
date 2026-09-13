use super::installation::ChatGptInstallation;
use super::platform::{chatgpt_is_running, request_quit};
use super::startup::{StartupPolicy, StartupSignal, StartupWatch, print_startup_notice};
use super::{ChatGptDesktopError, SESSION_TOKEN_ENVIRONMENT, SHUTDOWN_GRACE};
use nan_harness_runtime::{
    BridgeActivity, BridgeDiagnostic, CodexDesktopBridgeError, RunningCodexDesktopBridge,
};
use std::future::Future;
use std::process::{ExitStatus, Stdio};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::broadcast::error::RecvError;

use super::profile::ManagedProfile;

// wait must be cancel safe: select polls it alongside bridge activity.
pub(super) trait SupervisedApp {
    async fn wait(&mut self) -> Result<i32, ChatGptDesktopError>;
    async fn stop(&mut self) -> Result<(), ChatGptDesktopError>;
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
    let mut activities = bridge.subscribe_activities();
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
    let capture_stderr = crate::native_diagnostic::enabled(debug);
    if debug {
        command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    } else if capture_stderr {
        command.stdout(Stdio::null()).stderr(Stdio::piped());
    } else {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    }
    enable_linux_renderer_accessibility(&mut command);
    let mut child = command.spawn().map_err(ChatGptDesktopError::StartApp)?;
    let mut stderr_capture = child.stderr.take().map(start_stderr_capture);
    if let Err(error) = detect_singleton_race(&mut child, &mut stderr_capture).await {
        return Err(error);
    }
    let mut diagnostic_receiver = bridge.take_diagnostics();
    let bridge_stopped = async {
        match bridge.wait().await {
            Ok(()) => ChatGptDesktopError::BridgeExited,
            Err(error) => ChatGptDesktopError::Bridge(CodexDesktopBridgeError::Bridge(error)),
        }
    };
    let result = supervise_startup(
        &mut child,
        bridge_stopped,
        &mut activities,
        &mut diagnostic_receiver,
        &mut StartupWatch::new(policy),
        cancellation,
        diagnostics,
    )
    .await;
    if let Some(capture) = stderr_capture {
        let _ = finish_stderr_capture(capture).await;
    }
    result
}

fn enable_linux_renderer_accessibility(command: &mut Command) {
    #[cfg(target_os = "linux")]
    command.arg("--force-renderer-accessibility");
    #[cfg(not(target_os = "linux"))]
    let _ = command;
}

pub(super) async fn supervise_startup<A: SupervisedApp>(
    app: &mut A,
    bridge_stopped: impl Future<Output = ChatGptDesktopError>,
    activities: &mut tokio::sync::broadcast::Receiver<BridgeActivity>,
    diagnostic_receiver: &mut tokio::sync::mpsc::UnboundedReceiver<BridgeDiagnostic>,
    watch: &mut StartupWatch,
    cancellation: &nan_harness_runtime::CancellationToken,
    diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<i32, ChatGptDesktopError> {
    tokio::pin!(bridge_stopped);
    let mut authenticated = false;
    let mut activities_closed = false;
    let mut diagnostics_closed = false;
    loop {
        tokio::select! {
            exit_code = app.wait() => {
                drain_diagnostics(diagnostic_receiver, diagnostics);
                return exit_code;
            }
            signal = cancellation.cancelled() => {
                app.stop().await?;
                drain_diagnostics(diagnostic_receiver, diagnostics);
                return Ok(signal.exit_code());
            }
            error = &mut bridge_stopped => {
                app.stop().await?;
                drain_diagnostics(diagnostic_receiver, diagnostics);
                return Err(error);
            }
            diagnostic = diagnostic_receiver.recv(), if !diagnostics_closed => {
                match diagnostic {
                    Some(diagnostic) if !diagnostics.contains(&diagnostic) => diagnostics.push(diagnostic),
                    Some(_) => (),
                    None => diagnostics_closed = true,
                }
            }
            activity = activities.recv(), if !authenticated && !activities_closed => {
                match activity {
                    Ok(BridgeActivity::AuthenticatedClient) => authenticated = true,
                    Ok(_) | Err(RecvError::Lagged(_)) => (),
                    Err(RecvError::Closed) => activities_closed = true,
                }
            }
            startup = watch.signal(), if !authenticated => {
                match startup {
                    StartupSignal::Notice => print_startup_notice(),
                    StartupSignal::TimedOut => {
                        app.stop().await?;
                        drain_diagnostics(diagnostic_receiver, diagnostics);
                        return Err(ChatGptDesktopError::BridgeHandshakeTimeout);
                    }
                }
            }
        }
    }
}

impl SupervisedApp for Child {
    async fn wait(&mut self) -> Result<i32, ChatGptDesktopError> {
        Child::wait(self)
            .await
            .map(exit_code)
            .map_err(ChatGptDesktopError::WaitForApp)
    }

    async fn stop(&mut self) -> Result<(), ChatGptDesktopError> {
        stop_chatgpt(self).await
    }
}

fn drain_diagnostics(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<BridgeDiagnostic>,
    collected: &mut Vec<BridgeDiagnostic>,
) {
    while let Ok(diagnostic) = receiver.try_recv() {
        if !collected.contains(&diagnostic) {
            collected.push(diagnostic);
        }
    }
}

struct StderrCapture {
    task: tokio::task::JoinHandle<crate::native_diagnostic::Stderr>,
}

fn start_stderr_capture(
    mut stderr: impl tokio::io::AsyncRead + Unpin + Send + 'static,
) -> StderrCapture {
    StderrCapture {
        task: tokio::spawn(async move {
            const LIMIT: usize = 64 * 1024;
            let mut bytes = Vec::with_capacity(LIMIT);
            let mut buffer = [0_u8; 4096];
            let mut overflow = false;
            loop {
                match stderr.read(&mut buffer).await {
                    Ok(0) => break,
                    Ok(size) => {
                        if bytes.len() < LIMIT {
                            let retained = size.min(LIMIT - bytes.len());
                            bytes.extend_from_slice(&buffer[..retained]);
                            overflow |= retained < size;
                        } else {
                            overflow = true;
                        }
                    }
                    Err(_) => {
                        overflow = true;
                        break;
                    }
                }
            }
            crate::native_diagnostic::Stderr { bytes, overflow }
        }),
    }
}

async fn finish_stderr_capture(capture: StderrCapture) -> Option<crate::native_diagnostic::Stderr> {
    let mut task = capture.task;
    match tokio::time::timeout(std::time::Duration::from_secs(1), &mut task).await {
        Ok(Ok(result)) => Some(result),
        Ok(Err(_)) => None,
        Err(_) => {
            task.abort();
            let _ = task.await;
            None
        }
    }
}

async fn detect_singleton_race(
    child: &mut Child,
    capture: &mut Option<StderrCapture>,
) -> Result<(), ChatGptDesktopError> {
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    if let Some(status) = child.try_wait().map_err(ChatGptDesktopError::WaitForApp)? {
        let error = classify_early_exit(status.success(), chatgpt_is_running()?);
        if let Some(capture) = capture.take() {
            let stderr = finish_stderr_capture(capture).await;
            crate::native_diagnostic::emit_startup(
                crate::native_diagnostic::Failure::NativeAppExited,
                status,
                stderr,
            );
        }
        return Err(error);
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

#[cfg(test)]
mod tests {
    use super::enable_linux_renderer_accessibility;
    use tokio::process::Command;

    #[test]
    fn chatgpt_launch_requests_renderer_accessibility_on_linux() {
        let mut command = Command::new("/synthetic/chatgpt");
        enable_linux_renderer_accessibility(&mut command);
        #[cfg(target_os = "linux")]
        assert_eq!(
            command.as_std().get_args().collect::<Vec<_>>(),
            ["--force-renderer-accessibility"]
        );
        #[cfg(not(target_os = "linux"))]
        assert!(command.as_std().get_args().next().is_none());
    }
}
