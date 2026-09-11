use crate::prepared::PreparedLaunch;
use crate::searxng::SearxngCommand;
use nan_harness_core::launch_plan::{LaunchPlan, TerminalMode};
use nan_harness_core::{SecretError, SecretStore};
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
pub(crate) fn spawn_searxng(command: &SearxngCommand) -> io::Result<ManagedChild> {
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
    spawn_managed(process)
}

/// Keeps Python tied to the host's pipe, including abrupt host termination on Unix.
/// The owned standalone recipe always invokes a Python module with `-m`.
pub(crate) fn spawn_hosted_searxng(
    command: &SearxngCommand,
) -> io::Result<(ManagedChild, tokio::process::ChildStdin)> {
    if command.arguments.first().map(String::as_str) != Some("-m") || command.arguments.len() < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected a Python module command",
        ));
    }
    let mut process = Command::new(&command.program);
    process
        .arg("-c")
        .arg(include_str!("search_supervisor/hosted_python.py"))
        .args(&command.arguments[1..])
        .current_dir(&command.current_directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(settings) = &command.settings_path {
        process
            .env("SEARXNG_SETTINGS_PATH", settings)
            .env("SEARXNG_BIND_ADDRESS", "127.0.0.1")
            .env("SEARXNG_PORT", "8888")
            .env("SEARXNG_DEBUG", "false");
    }
    let mut child = spawn_managed(process)?;
    #[cfg(not(windows))]
    let input = child.inner.stdin.take();
    #[cfg(windows)]
    let input = child.inner.stdin().take();
    let input = input.ok_or_else(|| io::Error::other("missing backend lifetime pipe"))?;
    Ok((child, input))
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
