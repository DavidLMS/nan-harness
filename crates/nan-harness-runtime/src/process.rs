use crate::prepared::PreparedLaunch;
use crate::searxng::SearxngCommand;
use nan_harness_core::launch_plan::{LaunchPlan, TerminalMode};
use nan_harness_core::{SecretError, SecretStore};
use nan_harness_private_fs::{PrivatePathKind, restrict_file, restrict_path};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::io;
use std::process::ExitStatus;
use std::process::Stdio;
#[cfg(windows)]
use std::time::Duration;
use thiserror::Error;
#[cfg(not(windows))]
use tokio::process::Child;
use tokio::process::Command;

const INTERNAL_CANARY_USAGE_FILE: &str = "NAN_HARNESS_INTERNAL_CANARY_USAGE_FILE";

/// Starts the child process described by a prepared launch plan.
///
/// # Errors
///
/// Returns [`ProcessError`] when a referenced secret is absent or the process cannot start.
pub(crate) fn spawn_child(
    plan: &LaunchPlan,
    prepared: &PreparedLaunch,
    secrets: &SecretStore,
) -> Result<ManagedChild, ProcessError> {
    spawn_managed(prepare_command(plan, prepared, secrets)?).map_err(ProcessError::Spawn)
}

/// A child process with platform-specific lifetime ownership.
///
/// Windows processes are assigned to a kill-on-drop Job Object before startup continues, so
/// descendants remain owned by the supervisor. Other platforms retain Tokio's child process with
/// kill-on-drop enabled.
pub(crate) struct ManagedChild {
    #[cfg(not(windows))]
    inner: Child,
    #[cfg(windows)]
    inner: Box<dyn process_wrap::tokio::ChildWrapper>,
}

impl ManagedChild {
    pub(crate) fn id(&self) -> Option<u32> {
        self.inner.id()
    }

    pub(crate) fn start_kill(&mut self) -> io::Result<()> {
        #[cfg(not(windows))]
        {
            self.inner.start_kill()
        }
        #[cfg(windows)]
        {
            process_wrap::tokio::ChildWrapper::start_kill(&mut *self.inner)
        }
    }

    pub(crate) async fn kill(&mut self) -> io::Result<()> {
        #[cfg(not(windows))]
        {
            self.inner.kill().await
        }
        #[cfg(windows)]
        {
            process_wrap::tokio::ChildWrapper::kill(&mut *self.inner).await
        }
    }

    pub(crate) async fn wait(&mut self) -> io::Result<ExitStatus> {
        #[cfg(not(windows))]
        {
            self.inner.wait().await
        }
        #[cfg(windows)]
        {
            // JobObjectChild::wait may wait in a blocking task while descendants drain. Polling
            // keeps the supervisor's cancellation select cancellable during that interval.
            loop {
                if let Some(status) = process_wrap::tokio::ChildWrapper::try_wait(&mut *self.inner)?
                {
                    return Ok(status);
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
}

#[cfg(not(windows))]
impl From<Child> for ManagedChild {
    fn from(inner: Child) -> Self {
        Self { inner }
    }
}

fn spawn_managed(command: Command) -> io::Result<ManagedChild> {
    #[cfg(not(windows))]
    {
        let mut command = command;
        Ok(ManagedChild {
            inner: command.kill_on_drop(true).spawn()?,
        })
    }
    #[cfg(windows)]
    {
        use process_wrap::tokio::{CommandWrap, JobObject, KillOnDrop};

        let inner = CommandWrap::from(command)
            .wrap(KillOnDrop)
            .wrap(JobObject)
            .spawn()?;
        Ok(ManagedChild { inner })
    }
}

/// Starts a standalone `SearXNG` command under the same kill-on-drop ownership
/// contract used for harness children.
pub(crate) fn spawn_searxng(
    command: &SearxngCommand,
    coordination_directory: &std::path::Path,
    endpoint: &str,
    shutdown_grace: std::time::Duration,
    grace_recheck: std::time::Duration,
    readiness_timeout: std::time::Duration,
    host_executable: Option<&std::path::Path>,
) -> io::Result<(ManagedChild, Option<std::path::PathBuf>)> {
    let mut stop_id = [0_u8; 8];
    getrandom::fill(&mut stop_id)
        .map_err(|_| io::Error::other("could not create SearXNG stop marker ID"))?;
    let mut stop_id_text = String::with_capacity(stop_id.len() * 2);
    for byte in stop_id {
        let _ = write!(&mut stop_id_text, "{byte:02x}");
    }
    let stop_path = coordination_directory.join(format!(
        ".nan-harness-searxng-stop-{}-{}",
        std::process::id(),
        stop_id_text
    ));
    let request = SearxngHostRequest {
        command: command.clone(),
        endpoint: endpoint.to_owned(),
        coordination_directory: coordination_directory.to_path_buf(),
        stop_path: stop_path.clone(),
        shutdown_grace,
        grace_recheck,
        readiness_timeout,
    };
    let executable = host_executable
        .map(std::path::Path::to_path_buf)
        .or_else(|| std::env::current_exe().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "current executable unavailable"))?;
    let use_host = host_executable.is_some()
        || executable
            .file_stem()
            .is_some_and(|stem| matches!(stem.to_str(), Some("nanh" | "nan-harness")));
    if !use_host {
        return spawn_searxng_backend_detached(command).map(|child| (child, None));
    }
    let request_path = write_host_request(coordination_directory, &request)?;
    let mut host = Command::new(executable);
    host.arg("__searxng-host")
        .arg(&request_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_detached(&mut host);
    match spawn_detached(host) {
        Ok(child) => Ok((child, Some(stop_path))),
        Err(error) => {
            let _ = std::fs::remove_file(&request_path);
            Err(error)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SearxngHostRequest {
    pub(crate) command: SearxngCommand,
    pub(crate) endpoint: String,
    pub(crate) coordination_directory: std::path::PathBuf,
    pub(crate) stop_path: std::path::PathBuf,
    pub(crate) shutdown_grace: std::time::Duration,
    pub(crate) grace_recheck: std::time::Duration,
    pub(crate) readiness_timeout: std::time::Duration,
}

pub(crate) fn spawn_searxng_backend(command: &SearxngCommand) -> io::Result<ManagedChild> {
    spawn_searxng_backend_with_drop_policy(command, true)
}

pub(crate) fn spawn_searxng_backend_detached(command: &SearxngCommand) -> io::Result<ManagedChild> {
    spawn_searxng_backend_with_drop_policy(command, false)
}

fn spawn_searxng_backend_with_drop_policy(
    command: &SearxngCommand,
    kill_on_drop: bool,
) -> io::Result<ManagedChild> {
    let mut process = Command::new(&command.program);
    process
        .args(&command.arguments)
        .current_dir(&command.current_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(settings) = &command.settings_path {
        process
            .env("SEARXNG_SETTINGS_PATH", settings)
            .env("SEARXNG_BIND_ADDRESS", "127.0.0.1")
            .env("SEARXNG_PORT", "8888")
            .env("SEARXNG_DEBUG", "false");
    }
    if kill_on_drop {
        spawn_managed(process)
    } else {
        spawn_detached(process)
    }
}

fn write_host_request(
    directory: &std::path::Path,
    request: &SearxngHostRequest,
) -> io::Result<std::path::PathBuf> {
    let mut temporary = tempfile::Builder::new()
        .prefix(".nan-harness-searxng-host-")
        .tempfile_in(directory)?;
    restrict_file(temporary.as_file_mut())?;
    serde_json::to_writer(&mut temporary, request)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    temporary.as_file().sync_all()?;
    let path = temporary.path().to_path_buf();
    temporary.keep().map_err(|error| error.error)?;
    restrict_path(&path, PrivatePathKind::File)?;
    Ok(path)
}

fn spawn_detached(mut command: Command) -> io::Result<ManagedChild> {
    #[cfg(not(windows))]
    {
        Ok(ManagedChild {
            inner: command.kill_on_drop(false).spawn()?,
        })
    }
    #[cfg(windows)]
    {
        use process_wrap::tokio::{CommandWrap, JobObject};

        let inner = CommandWrap::from(command).wrap(JobObject).spawn()?;
        Ok(ManagedChild { inner })
    }
}

#[cfg(unix)]
fn configure_detached(command: &mut Command) {
    command.process_group(0);
}

#[cfg(windows)]
fn configure_detached(command: &mut Command) {
    use std::os::windows::process::CommandExt as _;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    command.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
}

fn prepare_command(
    plan: &LaunchPlan,
    prepared: &PreparedLaunch,
    secrets: &SecretStore,
) -> Result<Command, ProcessError> {
    let mut command = Command::new(&plan.harness.executable);
    command
        .args(prepared.arguments())
        .current_dir(&plan.process.working_directory)
        .env_remove("NAN_API_KEY")
        .env_remove(INTERNAL_CANARY_USAGE_FILE);

    for variable in &plan.environment.remove {
        command.env_remove(variable);
    }
    for (variable, value) in prepared.public_environment() {
        command.env(variable, value);
    }
    for (variable, reference) in &plan.environment.secrets {
        prepared
            .with_secret(secrets, reference, |value| {
                command.env(variable, value);
            })
            .map_err(ProcessError::Secret)?;
    }

    command.env_remove(INTERNAL_CANARY_USAGE_FILE);

    match plan.process.terminal {
        TerminalMode::Inherit => {
            command
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
        }
        TerminalMode::Captured => {
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
        }
    }

    Ok(command)
}

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error(transparent)]
    Secret(SecretError),
    #[error("could not start harness process: {0}")]
    Spawn(io::Error),
}

#[cfg(test)]
mod tests {
    use super::{INTERNAL_CANARY_USAGE_FILE, prepare_command};
    use crate::prepared::PreparedLaunch;
    use nan_harness_core::{LaunchPlan, SecretStore};
    use std::ffi::OsStr;

    const DIRECT_PLAN: &str =
        include_str!("../../nan-harness-core/tests/fixtures/launch-plan.direct.json");

    #[test]
    fn command_preserves_argument_order_and_removes_inherited_provider_credentials() {
        let mut plan: LaunchPlan = serde_json::from_str(DIRECT_PLAN).expect("valid fixture");
        plan.environment.secrets.clear();
        let prepared = PreparedLaunch::prepare(&plan, "https://api.nan.builders/v1", None, None)
            .expect("launch should prepare");
        let command =
            prepare_command(&plan, &prepared, &SecretStore::new()).expect("command should build");
        let arguments = command.as_std().get_args().collect::<Vec<_>>();
        let nan_api_key = command
            .as_std()
            .get_envs()
            .find(|(name, _)| *name == OsStr::new("NAN_API_KEY"));
        let usage_file = command
            .as_std()
            .get_envs()
            .find(|(name, _)| *name == OsStr::new(INTERNAL_CANARY_USAGE_FILE));

        assert_eq!(
            arguments,
            [
                OsStr::new("run"),
                OsStr::new("--model"),
                OsStr::new("nan/qwen3.6")
            ]
        );
        assert!(nan_api_key.is_some_and(|(_, value)| value.is_none()));
        assert!(usage_file.is_some_and(|(_, value)| value.is_none()));
    }

    #[test]
    fn command_cannot_reintroduce_internal_usage_file_from_launch_environment() {
        let mut plan: LaunchPlan = serde_json::from_str(DIRECT_PLAN).expect("valid fixture");
        plan.environment.secrets.clear();
        plan.environment.public.insert(
            INTERNAL_CANARY_USAGE_FILE.to_owned(),
            "should-not-leak".to_owned(),
        );
        let prepared = PreparedLaunch::prepare(&plan, "https://api.nan.builders/v1", None, None)
            .expect("launch should prepare");
        let command =
            prepare_command(&plan, &prepared, &SecretStore::new()).expect("command should build");

        let usage_file = command
            .as_std()
            .get_envs()
            .find(|(name, _)| *name == OsStr::new(INTERNAL_CANARY_USAGE_FILE));
        assert!(usage_file.is_some_and(|(_, value)| value.is_none()));
    }
}
