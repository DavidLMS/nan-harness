use super::RuntimeError;
use crate::prepared::PreparedLaunch;
use crate::process::{ManagedChild, ProcessError, prepare_command, spawn_managed};
use crate::signals::CancellationToken;
use nan_harness_core::{LaunchPlan, SecretStore};
use std::io;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;
use tokio::io::AsyncReadExt as _;
use tokio::process::{ChildStdin, Command};
use tokio::time::timeout;

const HELPER_TIMEOUT: Duration = Duration::from_secs(70);
const STOP_GRACE: Duration = Duration::from_secs(20);

/// Native foreground authority and client share a private persistent home, but
/// have independent process handles/Jobs. The bridge remains alive through drain.
pub(super) struct HermesGateway {
    command: Vec<String>,
    child: ManagedChild,
    lifetime: Option<ChildStdin>,
}

impl HermesGateway {
    pub(super) async fn start(
        plan: &LaunchPlan,
        prepared: &PreparedLaunch,
        secrets: &SecretStore,
        cancellation: &CancellationToken,
    ) -> Result<Option<Self>, RuntimeError> {
        let Some(profile) = prepared.hermes() else {
            return Ok(None);
        };
        // Future cancellation/Drop must preserve artifacts unless the complete
        // owned-process cleanup explicitly disarms this default retention.
        prepared.arm_cleanup();
        let command = resolve_command(plan, prepared, secrets).await?;
        helper(
            &command,
            "absent",
            &[],
            plan,
            prepared,
            secrets,
            cancellation,
        )
        .await?;
        let staged = match profile.stage_files() {
            Ok(()) => {
                helper(
                    &command,
                    "stage",
                    &[],
                    plan,
                    prepared,
                    secrets,
                    cancellation,
                )
                .await
            }
            Err(error) => Err(ProcessError::Spawn(error).into()),
        };
        if let Err(error) = staged {
            clear_profile(&command, plan, prepared, secrets).await?;
            return Err(error);
        }
        let mut owner = native_command(&command, "owner", &[], plan, prepared, secrets)?;
        owner
            .env("HERMES_HOME", &profile.home)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        owner.process_group(0);
        let mut child = match spawn_managed(owner) {
            Ok(child) => child,
            Err(error) => {
                // Windows assignment failure can occur after OS process creation.
                prepared.retain_workspace();
                return Err(ProcessError::Spawn(error).into());
            }
        };
        let lifetime = child.take_stdin();
        let mut gateway = Self {
            command,
            child,
            lifetime,
        };
        let result = gateway
            .initialize(plan, prepared, secrets, cancellation)
            .await;
        if let Err(error) = result {
            let cleanup = gateway.stop(plan, prepared, secrets).await;
            cleanup?;
            return Err(error);
        }
        Ok(Some(gateway))
    }

    async fn initialize(
        &mut self,
        plan: &LaunchPlan,
        prepared: &PreparedLaunch,
        secrets: &SecretStore,
        cancellation: &CancellationToken,
    ) -> Result<(), RuntimeError> {
        let pid = self
            .child
            .id()
            .ok_or(RuntimeError::MissingProcessId)?
            .to_string();
        for action in ["ready", "refresh"] {
            let mut arguments = vec![pid.as_str()];
            if action == "refresh" {
                arguments.extend(prepared.arguments().iter().map(String::as_str));
            }
            let command =
                native_command(&self.command, action, &arguments, plan, prepared, secrets)?;
            let mut child = spawn_helper(command, prepared)?;
            wait_helper(
                &mut child,
                action,
                cancellation,
                Some(&mut self.child),
                prepared,
            )
            .await?;
        }
        Ok(())
    }

    pub(super) fn spawn_frontend(
        &self,
        plan: &LaunchPlan,
        prepared: &PreparedLaunch,
        secrets: &SecretStore,
    ) -> Result<ManagedChild, RuntimeError> {
        let arguments = prepared
            .arguments()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let command = native_command(
            &self.command,
            "frontend",
            &arguments,
            plan,
            prepared,
            secrets,
        )?;
        spawn_managed(command).map_err(|error| {
            // Job assignment can fail after Windows creates the frontend. A
            // successful gateway drain cannot certify that failed child spawn.
            prepared.retain_workspace();
            ProcessError::Spawn(error).into()
        })
    }

    pub(super) async fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.wait().await
    }

    pub(super) async fn stop(
        &mut self,
        plan: &LaunchPlan,
        prepared: &PreparedLaunch,
        secrets: &SecretStore,
    ) -> Result<(), RuntimeError> {
        // EOF asks this exact Python process to mark its own native planned stop.
        self.lifetime.take();
        let graceful = timeout(STOP_GRACE, self.child.wait()).await;
        let drained = matches!(&graceful, Ok(Ok(status)) if status.success());
        let stopped = match graceful {
            Ok(result) => result.map(|_| ()),
            Err(_) => match timeout(Duration::from_secs(5), self.child.kill()).await {
                Ok(result) => result,
                Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, "NH-HERMES-CLEANUP")),
            },
        };
        if let Err(error) = stopped {
            prepared.retain_workspace();
            return Err(RuntimeError::WaitForProcess(error));
        }
        if !drained {
            // A killed or unexpectedly exited primary is not proof that every
            // native worker drained. Keep its configuration and fail closed.
            prepared.retain_workspace();
            return Err(failure("NH-HERMES-GATEWAY-DRAIN"));
        }
        clear_profile(&self.command, plan, prepared, secrets).await
    }
}

fn failure(code: &'static str) -> RuntimeError {
    ProcessError::Spawn(io::Error::other(code)).into()
}

fn native_command(
    prefix: &[String],
    action: &str,
    arguments: &[&str],
    plan: &LaunchPlan,
    prepared: &PreparedLaunch,
    secrets: &SecretStore,
) -> Result<Command, RuntimeError> {
    let original = prepare_command(plan, prepared, secrets)?.into_std();
    let mut command = Command::new(&prefix[0]);
    command
        .args(&prefix[1..])
        .arg(action)
        .args(arguments)
        .current_dir(&plan.process.working_directory);
    for (name, value) in original.get_envs() {
        if let Some(value) = value {
            command.env(name, value);
        } else {
            command.env_remove(name);
        }
    }
    if let Some(profile) = prepared.hermes() {
        let home = if action == "owner" || action == "frontend" {
            &profile.home
        } else {
            &profile.bootstrap
        };
        command.env("HERMES_HOME", home);
    }
    if action != "frontend"
        || plan.process.terminal == nan_harness_core::launch_plan::TerminalMode::Captured
    {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
    }
    Ok(command)
}

async fn helper(
    prefix: &[String],
    action: &str,
    arguments: &[&str],
    plan: &LaunchPlan,
    prepared: &PreparedLaunch,
    secrets: &SecretStore,
    cancellation: &CancellationToken,
) -> Result<(), RuntimeError> {
    let command = native_command(prefix, action, arguments, plan, prepared, secrets)?;
    let mut child = spawn_helper(command, prepared)?;
    wait_helper(&mut child, action, cancellation, None, prepared).await
}

fn spawn_helper(command: Command, prepared: &PreparedLaunch) -> Result<ManagedChild, RuntimeError> {
    spawn_managed(command).map_err(|error| {
        prepared.retain_workspace();
        ProcessError::Spawn(error).into()
    })
}

async fn wait_helper(
    child: &mut ManagedChild,
    action: &str,
    cancellation: &CancellationToken,
    authority: Option<&mut ManagedChild>,
    prepared: &PreparedLaunch,
) -> Result<(), RuntimeError> {
    let result = tokio::select! {
        result = timeout(HELPER_TIMEOUT, child.wait()) => result.ok().and_then(Result::ok),
        _signal = cancellation.cancelled() => None,
        _status = async {
            match authority {
                Some(child) => child.wait().await,
                None => std::future::pending().await,
            }
        } => None,
    };
    if result.is_some_and(|status| status.success()) {
        return Ok(());
    }
    // Cancellation and timeout retain the handle until this helper is reaped.
    let stopped = timeout(Duration::from_secs(5), child.kill()).await;
    if !matches!(stopped, Ok(Ok(()))) {
        prepared.retain_workspace();
        return Err(failure("NH-HERMES-HELPER-CLEANUP"));
    }
    Err(failure(match action {
        "stage" => "NH-HERMES-PROFILE",
        "ready" => "NH-HERMES-READINESS",
        "refresh" => "NH-HERMES-HISTORY",
        "cleanup" => "NH-HERMES-CLEANUP",
        _ => "NH-HERMES-HELPER",
    }))
}

async fn clear_profile(
    command: &[String],
    plan: &LaunchPlan,
    prepared: &PreparedLaunch,
    secrets: &SecretStore,
) -> Result<(), RuntimeError> {
    if prepared.cleanup_uncertain() {
        return Err(failure("NH-HERMES-CLEANUP"));
    }
    let result = helper(
        command,
        "cleanup",
        &[],
        plan,
        prepared,
        secrets,
        &CancellationToken::new(),
    )
    .await;
    if result.is_err() {
        prepared.retain_workspace();
    } else {
        prepared.release_workspace();
    }
    result
}

async fn resolve_command(
    plan: &LaunchPlan,
    prepared: &PreparedLaunch,
    secrets: &SecretStore,
) -> Result<Vec<String>, RuntimeError> {
    let profile = prepared
        .hermes()
        .ok_or_else(|| failure("NH-HERMES-PROFILE"))?;
    let scripts = profile.scripts.to_string_lossy();
    let script = profile.scripts.join("main.py");
    let script = script.to_string_lossy();
    let mut command = native_command(
        std::slice::from_ref(&plan.harness.executable),
        "--print-runtime-command",
        &["--module", "pm._venv_entry", "--", &scripts, &script],
        plan,
        prepared,
        secrets,
    )?;
    command.stdout(Stdio::piped());
    let mut child = spawn_helper(command, prepared)?;
    let collected = timeout(Duration::from_secs(10), async {
        let stdout = child
            .take_stdout()
            .ok_or_else(|| failure("NH-HERMES-LAUNCHER"))?;
        let mut bytes = Vec::new();
        stdout
            .take(65537)
            .read_to_end(&mut bytes)
            .await
            .map_err(ProcessError::Spawn)?;
        if bytes.len() > 65536 {
            return Err(failure("NH-HERMES-LAUNCHER"));
        }
        let status = child.wait().await.map_err(RuntimeError::WaitForProcess)?;
        if !status.success() {
            return Err(failure("NH-HERMES-LAUNCHER"));
        }
        Ok(bytes)
    })
    .await
    .unwrap_or_else(|_| Err(failure("NH-HERMES-LAUNCHER")));
    let bytes = match collected {
        Ok(bytes) => bytes,
        Err(error) => {
            if !matches!(
                timeout(Duration::from_secs(5), child.kill()).await,
                Ok(Ok(()))
            ) {
                prepared.retain_workspace();
            }
            return Err(error);
        }
    };
    let prefix: Vec<String> =
        serde_json::from_slice(&bytes).map_err(|_| failure("NH-HERMES-LAUNCHER"))?;
    if prefix.is_empty()
        || prefix.iter().any(|value| value.contains('\0'))
        || !Path::new(&prefix[0]).is_absolute()
        || !Path::new(&prefix[0]).is_file()
    {
        return Err(failure("NH-HERMES-LAUNCHER"));
    }
    Ok(prefix)
}
