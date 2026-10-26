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
            .env("HERMES_HOME", &profile.native_home)
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
    let lazy_installs = original
        .get_envs()
        .find(|(name, _)| *name == "HERMES_DISABLE_LAZY_INSTALLS")
        .map_or_else(
            || std::env::var_os("HERMES_DISABLE_LAZY_INSTALLS"),
            |(_, value)| value.map(std::ffi::OsStr::to_os_string),
        );
    command.env_remove("NAN_HARNESS_HERMES_LAZY_INSTALLS");
    if let Some(value) = lazy_installs {
        command.env("NAN_HARNESS_HERMES_LAZY_INSTALLS", value);
    }
    // Bootstrap must keep the directly owned PID. Dependency activation still
    // runs; the helper restores the user's lazy-tool policy before native main.
    command.env("HERMES_DISABLE_LAZY_INSTALLS", "1");
    // Native identify otherwise attributes an inherited systemd/launchd/Desktop
    // marker to this new gateway instead of its actual external Rust owner.
    for name in [
        "INVOCATION_ID",
        "HERMES_SUPERVISED_CHILD",
        "HERMES_S6_SUPERVISED_CHILD",
        "HERMES_GATEWAY_EXTERNAL_SUPERVISOR",
        "HERMES_DESKTOP_MANAGED",
        "HERMES_LAUNCHD_LABEL",
        "LAUNCHD_SOCKET",
        "XPC_SERVICE_NAME",
    ] {
        command.env_remove(name);
    }
    if let Some(profile) = prepared.hermes() {
        let home = if action == "owner" || action == "frontend" {
            &profile.native_home
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
    let exit_code = result.as_ref().and_then(ExitStatus::code);
    if result.is_some_and(|status| status.success()) {
        return Ok(());
    }
    // Cancellation and timeout retain the handle until this helper is reaped.
    let stopped = timeout(Duration::from_secs(5), child.kill()).await;
    if !matches!(stopped, Ok(Ok(()))) {
        prepared.retain_workspace();
        return Err(failure("NH-HERMES-HELPER-CLEANUP"));
    }
    // Closed exit codes match READINESS_EXIT_CODES in the embedded native helper.
    Err(failure(match (action, exit_code) {
        ("ready", Some(90)) => "NH-HERMES-READINESS-CONTROL-ABSENT",
        ("ready", Some(91)) => "NH-HERMES-READINESS-CONTROL-ACCESS",
        ("ready", Some(92)) => "NH-HERMES-READINESS-CONTROL-TIMEOUT",
        ("ready", Some(93)) => "NH-HERMES-READINESS-CONTROL-PROTOCOL",
        ("ready", Some(94)) => "NH-HERMES-READINESS-IDENTITY-PID",
        ("ready", Some(95)) => "NH-HERMES-READINESS-IDENTITY-HOME",
        ("ready", Some(96)) => "NH-HERMES-READINESS-IDENTITY-SUPERVISOR",
        ("ready", Some(97)) => "NH-HERMES-READINESS-RUNTIME-PROTOCOL",
        ("ready", Some(98)) => "NH-HERMES-READINESS-RUNTIME-PROFILE",
        ("ready", Some(99)) => "NH-HERMES-READINESS-RUNTIME-AUTHORITY",
        ("ready", Some(100)) => "NH-HERMES-READINESS-RUNTIME-ENDPOINT",
        ("ready", Some(101)) => "NH-HERMES-READINESS-RUNTIME-STATE",
        ("stage", _) => "NH-HERMES-PROFILE",
        ("ready", _) => "NH-HERMES-READINESS",
        ("refresh", _) => "NH-HERMES-HISTORY",
        ("cleanup", _) => "NH-HERMES-CLEANUP",
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
