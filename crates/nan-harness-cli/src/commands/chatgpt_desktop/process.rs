use super::installation::ChatGptInstallation;
use super::platform::{chatgpt_is_running, request_quit};
use super::{
    BRIDGE_HANDSHAKE_TIMEOUT, ChatGptDesktopError, SESSION_TOKEN_ENVIRONMENT, SHUTDOWN_GRACE,
};
use nan_harness_runtime::{BridgeDiagnostic, CodexDesktopBridgeError, RunningCodexDesktopBridge};
use std::process::{ExitStatus, Stdio};
use tokio::process::{Child, Command};

use super::profile::ManagedProfile;

pub(super) async fn supervise_desktop(
    installation: &ChatGptInstallation,
    profile: &ManagedProfile,
    bridge: &mut RunningCodexDesktopBridge,
    debug: bool,
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
    if debug {
        command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    } else {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let mut child = command.spawn().map_err(ChatGptDesktopError::StartApp)?;
    detect_singleton_race(&mut child).await?;
    let mut diagnostic_receiver = bridge.take_diagnostics();
    let handshake_deadline = tokio::time::sleep(BRIDGE_HANDSHAKE_TIMEOUT);
    tokio::pin!(handshake_deadline);
    let mut authenticated = false;
    loop {
        tokio::select! {
            status = child.wait() => {
                let status = status.map_err(ChatGptDesktopError::WaitForApp)?;
                drain_diagnostics(&mut diagnostic_receiver, diagnostics);
                return Ok(exit_code(status));
            }
            signal = cancellation.cancelled() => {
                stop_chatgpt(&mut child).await?;
                drain_diagnostics(&mut diagnostic_receiver, diagnostics);
                return Ok(signal.exit_code());
            }
            bridge_result = bridge.wait() => {
                stop_chatgpt(&mut child).await?;
                drain_diagnostics(&mut diagnostic_receiver, diagnostics);
                return match bridge_result {
                    Ok(()) => Err(ChatGptDesktopError::BridgeExited),
                    Err(error) => Err(ChatGptDesktopError::Bridge(CodexDesktopBridgeError::Bridge(error))),
                };
            }
            diagnostic = diagnostic_receiver.recv() => {
                if let Some(diagnostic) = diagnostic
                    && !diagnostics.contains(&diagnostic)
                {
                    diagnostics.push(diagnostic);
                }
            }
            activity = activities.recv(), if !authenticated => {
                if matches!(
                    activity,
                    Ok(nan_harness_runtime::BridgeActivity::AuthenticatedClient)
                ) {
                    authenticated = true;
                }
            }
            () = &mut handshake_deadline, if !authenticated => {
                stop_chatgpt(&mut child).await?;
                return Err(ChatGptDesktopError::BridgeHandshakeTimeout);
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

fn drain_diagnostics(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<BridgeDiagnostic>,
    diagnostics: &mut Vec<BridgeDiagnostic>,
) {
    while let Ok(diagnostic) = receiver.try_recv() {
        if !diagnostics.contains(&diagnostic) {
            diagnostics.push(diagnostic);
        }
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
