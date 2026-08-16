use crate::prepared::PreparedLaunch;
use nan_harness_core::launch_plan::{LaunchPlan, TerminalMode};
use nan_harness_core::{SecretError, SecretStore};
use std::process::Stdio;
use thiserror::Error;
use tokio::process::{Child, Command};

/// Starts the child process described by a prepared launch plan.
///
/// # Errors
///
/// Returns [`ProcessError`] when a referenced secret is absent or the process cannot start.
pub(crate) fn spawn_child(
    plan: &LaunchPlan,
    prepared: &PreparedLaunch,
    secrets: &SecretStore,
) -> Result<Child, ProcessError> {
    prepare_command(plan, prepared, secrets)?
        .spawn()
        .map_err(ProcessError::Spawn)
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
        .env_remove("NAN_API_KEY");

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
    Spawn(std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::prepare_command;
    use crate::prepared::PreparedLaunch;
    use nan_harness_core::{LaunchPlan, SecretStore};
    use std::ffi::OsStr;

    const DIRECT_PLAN: &str =
        include_str!("../../nan-harness-core/tests/fixtures/launch-plan.direct.json");

    #[test]
    fn command_preserves_argument_order_and_removes_inherited_provider_credentials() {
        let mut plan: LaunchPlan = serde_json::from_str(DIRECT_PLAN).expect("valid fixture");
        plan.environment.secrets.clear();
        let prepared = PreparedLaunch::prepare(&plan, None).expect("launch should prepare");
        let command =
            prepare_command(&plan, &prepared, &SecretStore::new()).expect("command should build");
        let arguments = command.as_std().get_args().collect::<Vec<_>>();
        let nan_api_key = command
            .as_std()
            .get_envs()
            .find(|(name, _)| *name == OsStr::new("NAN_API_KEY"));

        assert_eq!(
            arguments,
            [
                OsStr::new("run"),
                OsStr::new("--model"),
                OsStr::new("nan/qwen3.6")
            ]
        );
        assert!(nan_api_key.is_some_and(|(_, value)| value.is_none()));
    }
}
