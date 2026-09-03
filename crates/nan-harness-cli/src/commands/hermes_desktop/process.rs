#[allow(clippy::wildcard_imports)]
use super::*;

mod platform;
pub(super) use platform::*;

pub(super) fn spawn_desktop(
    executable: &str,
    arguments: &[String],
    paths: &DesktopPaths,
    working_directory: &Path,
) -> Result<Child, HermesDesktopError> {
    let mut command = TokioCommand::new(executable);
    command
        .args(arguments)
        .current_dir(working_directory)
        .env("HERMES_HOME", &paths.hermes_home)
        .env_remove("NAN_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("OPENAI_BASE_URL")
        .env_remove("CUSTOM_BASE_URL")
        .env_remove("HERMES_INFERENCE_MODEL")
        .env_remove("HERMES_INFERENCE_PROVIDER")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    command.spawn().map_err(HermesDesktopError::Launch)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LifecycleCompletion {
    Closed(i32),
    PreserveRecovery(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UpdateWaitCompletion {
    Finished { interrupt_seen: bool },
    PreserveRecovery(i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RelaunchWaitCompletion {
    Running(DesktopProcess),
    PreserveRecovery(i32),
    TimedOut,
}

pub(super) async fn supervise_desktop(
    child: &mut Child,
    mut gateway: Option<&mut RunningChatCompletionsGateway>,
    paths: &DesktopPaths,
    marker_before_launch: Option<MarkerFingerprint>,
    signals: &mut tokio::sync::mpsc::UnboundedReceiver<i32>,
) -> Result<LifecycleCompletion, HermesDesktopError> {
    let initial_status = tokio::select! {
        status = child.wait() => status.map_err(HermesDesktopError::Wait)?,
        signal = signals.recv() => {
            let exit_code = signal.unwrap_or(143);
            terminate_desktop_or_child(child).await?;
            return Ok(LifecycleCompletion::Closed(exit_code));
        }
        gateway_result = wait_for_gateway(&mut gateway) => {
            let error = gateway_result.err().unwrap_or(HermesDesktopError::GatewayExited);
            terminate_desktop_or_child(child).await?;
            return Err(error);
        }
    };

    if !update_started(paths, marker_before_launch) {
        if let Some(process) = running_desktop()? {
            eprintln!("Hermes Desktop's launcher exited; continuing to supervise the running app.");
            return supervise_running_desktop(process, &mut gateway, signals).await;
        }
        return Ok(LifecycleCompletion::Closed(exit_code(initial_status)));
    }

    eprintln!(
        "Hermes Desktop is updating. NaN will keep the local gateway and managed profile active."
    );
    let interrupt_seen = match wait_for_update(paths, &mut gateway, signals).await? {
        UpdateWaitCompletion::Finished { interrupt_seen } => interrupt_seen,
        UpdateWaitCompletion::PreserveRecovery(exit_code) => {
            return Ok(LifecycleCompletion::PreserveRecovery(exit_code));
        }
    };
    let process = match wait_for_relaunch(&mut gateway, signals, interrupt_seen).await? {
        RelaunchWaitCompletion::Running(process) => process,
        RelaunchWaitCompletion::PreserveRecovery(exit_code) => {
            return Ok(LifecycleCompletion::PreserveRecovery(exit_code));
        }
        RelaunchWaitCompletion::TimedOut => return Err(HermesDesktopError::DidNotRelaunch),
    };
    eprintln!("Hermes Desktop update completed; continuing the same NaN session.");
    supervise_running_desktop(process, &mut gateway, signals).await
}

pub(super) async fn supervise_running_desktop(
    mut process: DesktopProcess,
    gateway: &mut Option<&mut RunningChatCompletionsGateway>,
    signals: &mut tokio::sync::mpsc::UnboundedReceiver<i32>,
) -> Result<LifecycleCompletion, HermesDesktopError> {
    loop {
        tokio::select! {
            () = tokio::time::sleep(PROCESS_POLL_INTERVAL) => {
                if !process_is_same(&process)? {
                    if let Some(replacement) = running_desktop()? {
                        process = replacement;
                    } else {
                        return Ok(LifecycleCompletion::Closed(0));
                    }
                }
            }
            signal = signals.recv() => {
                let exit_code = signal.unwrap_or(143);
                terminate_desktop().await?;
                return Ok(LifecycleCompletion::Closed(exit_code));
            }
            gateway_result = wait_for_gateway(gateway) => {
                let error = gateway_result.err().unwrap_or(HermesDesktopError::GatewayExited);
                terminate_desktop().await?;
                return Err(error);
            }
        }
    }
}

pub(super) async fn wait_for_gateway(
    gateway: &mut Option<&mut RunningChatCompletionsGateway>,
) -> Result<(), HermesDesktopError> {
    match gateway.as_deref_mut() {
        Some(gateway) => gateway.wait().await.map_err(HermesDesktopError::Gateway),
        None => std::future::pending().await,
    }
}

pub(super) async fn wait_for_update(
    paths: &DesktopPaths,
    gateway: &mut Option<&mut RunningChatCompletionsGateway>,
    signals: &mut tokio::sync::mpsc::UnboundedReceiver<i32>,
) -> Result<UpdateWaitCompletion, HermesDesktopError> {
    let started = Instant::now();
    let mut interrupt_seen = false;
    let mut stale_since = None;
    loop {
        if !paths.update_marker.exists() {
            return Ok(UpdateWaitCompletion::Finished { interrupt_seen });
        }
        if live_update_owner(&paths.update_marker)?.is_some() {
            stale_since = None;
        } else {
            let since = stale_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= Duration::from_secs(5) {
                return Ok(UpdateWaitCompletion::Finished { interrupt_seen });
            }
        }
        if started.elapsed() >= UPDATE_WAIT_TIMEOUT {
            return Err(HermesDesktopError::UpdateTimedOut);
        }
        tokio::select! {
            () = tokio::time::sleep(UPDATE_POLL_INTERVAL) => {}
            signal = signals.recv() => {
                let code = signal.unwrap_or(143);
                if update_interrupt_requests_exit(code, &mut interrupt_seen) {
                    eprintln!("NaN is exiting while the Hermes Desktop updater continues. Run `nanh hermes-desktop --restore` after the update finishes.");
                    return Ok(UpdateWaitCompletion::PreserveRecovery(code));
                }
                eprintln!("Hermes Desktop is still updating. Press Ctrl+C again to exit NaN while the updater continues.");
            }
            gateway_result = wait_for_gateway(gateway) => {
                return Err(gateway_result.err().unwrap_or(HermesDesktopError::GatewayExited));
            }
        }
    }
}

pub(super) async fn wait_for_relaunch(
    gateway: &mut Option<&mut RunningChatCompletionsGateway>,
    signals: &mut tokio::sync::mpsc::UnboundedReceiver<i32>,
    mut interrupt_seen: bool,
) -> Result<RelaunchWaitCompletion, HermesDesktopError> {
    let started = Instant::now();
    loop {
        if let Some(process) = running_desktop()? {
            return Ok(RelaunchWaitCompletion::Running(process));
        }
        if started.elapsed() >= RELAUNCH_WAIT_TIMEOUT {
            return Ok(RelaunchWaitCompletion::TimedOut);
        }
        tokio::select! {
            () = tokio::time::sleep(PROCESS_POLL_INTERVAL) => {}
            signal = signals.recv() => {
                let code = signal.unwrap_or(143);
                if update_interrupt_requests_exit(code, &mut interrupt_seen) {
                    eprintln!("NaN is exiting before Hermes Desktop relaunches. Run `nanh hermes-desktop --restore` after the update finishes.");
                    return Ok(RelaunchWaitCompletion::PreserveRecovery(code));
                }
                eprintln!("Hermes has finished updating and is relaunching. Press Ctrl+C again to exit NaN and preserve recovery state.");
            }
            gateway_result = wait_for_gateway(gateway) => {
                return Err(gateway_result.err().unwrap_or(HermesDesktopError::GatewayExited));
            }
        }
    }
}

pub(super) fn update_interrupt_requests_exit(code: i32, interrupt_seen: &mut bool) -> bool {
    if code != 130 || *interrupt_seen {
        true
    } else {
        *interrupt_seen = true;
        false
    }
}

pub(super) fn update_started(
    paths: &DesktopPaths,
    marker_before_launch: Option<MarkerFingerprint>,
) -> bool {
    if live_update_owner(&paths.update_marker)
        .ok()
        .flatten()
        .is_some()
    {
        return true;
    }
    let after = marker_fingerprint(&paths.update_marker);
    after.is_some() && after != marker_before_launch
}

pub(super) fn exit_code(status: ExitStatus) -> i32 {
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
