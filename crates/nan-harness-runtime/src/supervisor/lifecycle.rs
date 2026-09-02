use super::RuntimeError;
use super::report::Completion;
use crate::prepared::PreparedLaunch;
use crate::process::spawn_child;
use crate::signals::{CancellationToken, SignalKind};
use nan_harness_bridge::{BridgeDiagnostic, ProviderUsageSnapshot, RunningBridge};
use nan_harness_core::{LaunchPlan, SecretStore};
use std::process::ExitStatus;
use std::time::Duration;
use tokio::process::Child;

pub(super) struct BridgeExecution {
    pub(super) completion: Completion,
    pub(super) diagnostics: Vec<BridgeDiagnostic>,
    pub(super) provider_usage: ProviderUsageSnapshot,
}

pub(super) async fn run_bridged_child(
    plan: &LaunchPlan,
    prepared: &PreparedLaunch,
    secrets: &SecretStore,
    cancellation: &CancellationToken,
    bridge: &mut RunningBridge,
) -> Result<BridgeExecution, RuntimeError> {
    let mut child = match spawn_child(plan, prepared, secrets) {
        Ok(child) => child,
        Err(error) => {
            bridge.shutdown();
            bridge.wait().await?;
            return Err(RuntimeError::Process(error));
        }
    };

    let mut diagnostics = Vec::new();
    let completion =
        supervise_pair(&mut child, bridge, plan, cancellation, &mut diagnostics).await?;
    let provider_usage = bridge.usage();
    Ok(BridgeExecution {
        completion,
        diagnostics,
        provider_usage,
    })
}

async fn supervise_pair(
    child: &mut Child,
    bridge: &mut RunningBridge,
    plan: &LaunchPlan,
    cancellation: &CancellationToken,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<Completion, RuntimeError> {
    let mut diagnostics_rx = bridge.take_diagnostics();
    loop {
        tokio::select! {
            status = child.wait() => {
                let status = status.map_err(RuntimeError::WaitForProcess)?;
                bridge.shutdown();
                bridge.wait().await?;
                drain_bridge_diagnostics(&mut diagnostics_rx, bridge_diagnostics);
                return Ok(Completion::Exited(status));
            }
            signal = cancellation.cancelled() => {
                terminate_child(child, plan, signal, cancellation).await?;
                bridge.shutdown();
                bridge.wait().await?;
                drain_bridge_diagnostics(&mut diagnostics_rx, bridge_diagnostics);
                return Ok(Completion::Cancelled(signal));
            }
            bridge_result = bridge.wait() => {
                let bridge_error = bridge_result.err();
                terminate_child(child, plan, SignalKind::Terminate, cancellation).await?;
                return match bridge_error {
                    Some(error) => Err(RuntimeError::Bridge(error)),
                    None => Err(RuntimeError::BridgeExited),
                };
            }
            diagnostic = diagnostics_rx.recv() => {
                if let Some(diagnostic) = diagnostic {
                    push_bridge_diagnostic(bridge_diagnostics, diagnostic);
                }
            }
        }
    }
}

fn drain_bridge_diagnostics(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<BridgeDiagnostic>,
    diagnostics: &mut Vec<BridgeDiagnostic>,
) {
    while let Ok(diagnostic) = receiver.try_recv() {
        push_bridge_diagnostic(diagnostics, diagnostic);
    }
}

fn push_bridge_diagnostic(diagnostics: &mut Vec<BridgeDiagnostic>, diagnostic: BridgeDiagnostic) {
    if !diagnostics.contains(&diagnostic) {
        diagnostics.push(diagnostic);
    }
}

pub(super) async fn wait_for_child(
    child: &mut Child,
    plan: &LaunchPlan,
    cancellation: &CancellationToken,
) -> Result<Completion, RuntimeError> {
    tokio::select! {
        status = child.wait() => status
            .map(Completion::Exited)
            .map_err(RuntimeError::WaitForProcess),
        signal = cancellation.cancelled() => {
            terminate_child(child, plan, signal, cancellation).await?;
            Ok(Completion::Cancelled(signal))
        }
    }
}

async fn terminate_child(
    child: &mut Child,
    plan: &LaunchPlan,
    signal: SignalKind,
    cancellation: &CancellationToken,
) -> Result<(), RuntimeError> {
    if plan.process.forward_signals {
        forward_signal(child, signal)?;
    } else if let Err(error) = child.start_kill()
        && !is_process_gone_error(&error)
    {
        return Err(RuntimeError::TerminateProcess(error));
    }
    let grace = Duration::from_millis(u64::from(plan.cleanup.grace_period_ms));
    tokio::select! {
        result = child.wait() => reap_result(result),
        () = cancellation.force_cancelled() => kill_and_reap(child).await,
        () = tokio::time::sleep(grace) => kill_and_reap(child).await,
    }
}

fn reap_result(result: std::io::Result<ExitStatus>) -> Result<(), RuntimeError> {
    match result {
        Ok(_) => Ok(()),
        Err(error) if is_process_gone_error(&error) => Ok(()),
        Err(error) => Err(RuntimeError::WaitForProcess(error)),
    }
}

async fn kill_and_reap(child: &mut Child) -> Result<(), RuntimeError> {
    match child.kill().await {
        Ok(()) => Ok(()),
        Err(error) if is_process_gone_error(&error) => reap_child(child).await,
        Err(error) => Err(RuntimeError::TerminateProcess(error)),
    }
}

async fn reap_child(child: &mut Child) -> Result<(), RuntimeError> {
    match child.wait().await {
        Ok(_) => Ok(()),
        Err(error) if is_process_gone_error(&error) => Ok(()),
        Err(error) => Err(RuntimeError::WaitForProcess(error)),
    }
}

fn is_process_gone_error(error: &std::io::Error) -> bool {
    if error.kind() == std::io::ErrorKind::InvalidInput {
        return true;
    }

    #[cfg(unix)]
    {
        matches!(error.raw_os_error(), Some(code) if
            code == nix::libc::ECHILD || code == nix::libc::ESRCH
        )
    }

    #[cfg(not(unix))]
    {
        false
    }
}

#[cfg(unix)]
fn forward_signal(child: &mut Child, signal: SignalKind) -> Result<(), RuntimeError> {
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let Some(process_id) = child.id() else {
        return Ok(());
    };
    let process_id = i32::try_from(process_id).map_err(|_| RuntimeError::MissingProcessId)?;
    let native_signal = match signal {
        SignalKind::Interrupt => Signal::SIGINT,
        SignalKind::Terminate => Signal::SIGTERM,
    };
    match kill(Pid::from_raw(process_id), native_signal) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(error) => Err(RuntimeError::TerminateProcess(
            std::io::Error::from_raw_os_error(error as i32),
        )),
    }
}

#[cfg(not(unix))]
fn forward_signal(child: &mut Child, _signal: SignalKind) -> Result<(), RuntimeError> {
    child
        .start_kill()
        .or_else(|error| {
            if is_process_gone_error(&error) {
                Ok(())
            } else {
                Err(error)
            }
        })
        .map_err(RuntimeError::TerminateProcess)
}

#[cfg(all(test, unix))]
mod tests {
    use super::{is_process_gone_error, run_bridged_child, terminate_child};
    use crate::prepared::PreparedLaunch;
    use crate::signals::{CancellationToken, SignalKind};
    use crate::supervisor::RuntimeError;
    use nan_harness_bridge::{ChatCompletionsBridgeConfig, spawn_chat_completions};
    use nan_harness_core::{LaunchPlan, SecretRef, SecretStore, SecretValue};
    use std::sync::Arc;

    #[tokio::test]
    async fn child_spawn_failure_shuts_down_bridge_and_releases_prepared_resources() {
        let mut plan: LaunchPlan = serde_json::from_str(include_str!(
            "../../../nan-harness-core/tests/fixtures/launch-plan.direct.json"
        ))
        .expect("fixture should be valid");
        plan.harness.executable = "/definitely/missing/nan-harness-test".to_owned();
        let prepared = PreparedLaunch::prepare(&plan, "http://127.0.0.1:9/v1", None, None)
            .expect("launch should prepare");
        let temporary_root = prepared
            .temporary_root(true)
            .expect("fixture should prepare a temporary artifact");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bridge should bind");
        let mut bridge = spawn_chat_completions(
            listener,
            ChatCompletionsBridgeConfig {
                provider_base_url: "http://127.0.0.1:9/v1".to_owned(),
                model_id: plan.model.resolved_id.clone(),
                provider_api_key: Arc::new(
                    SecretValue::new("provider-key").expect("valid provider key"),
                ),
                session_token: Arc::new(
                    SecretValue::new("launch-scoped-token").expect("valid session token"),
                ),
                web_search_enabled: false,
            },
        )
        .expect("bridge should start");
        let provider_credential_ref =
            SecretRef::new("nan_api_key").expect("valid provider credential reference");
        let mut secrets = SecretStore::new();
        secrets.insert(
            provider_credential_ref,
            SecretValue::new("provider-key").expect("valid provider key"),
        );

        let result = run_bridged_child(
            &plan,
            &prepared,
            &secrets,
            &CancellationToken::new(),
            &mut bridge,
        )
        .await;

        assert!(matches!(result, Err(RuntimeError::Process(_))));
        assert!(bridge.is_finished());
        drop(prepared);
        assert!(!temporary_root.exists());
    }

    #[tokio::test]
    async fn terminate_child_treats_an_already_reaped_process_as_success() {
        let mut child = tokio::process::Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .expect("child should spawn");
        child.wait().await.expect("child should be reaped");
        let plan: LaunchPlan = serde_json::from_str(include_str!(
            "../../../nan-harness-core/tests/fixtures/launch-plan.direct.json"
        ))
        .expect("fixture should be valid");

        terminate_child(
            &mut child,
            &plan,
            SignalKind::Interrupt,
            &CancellationToken::new(),
        )
        .await
        .expect("cancellation should tolerate a reaped child");
    }

    #[test]
    fn process_gone_errors_are_recognized() {
        assert!(is_process_gone_error(&std::io::Error::from_raw_os_error(
            nix::libc::ESRCH
        )));
        assert!(is_process_gone_error(&std::io::Error::from_raw_os_error(
            nix::libc::ECHILD
        )));
        assert!(!is_process_gone_error(&std::io::Error::from_raw_os_error(
            nix::libc::EPERM
        )));
    }
}
