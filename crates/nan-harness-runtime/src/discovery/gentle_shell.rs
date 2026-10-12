use super::{DiscoveryError, executable, exit_signal, probe};
use std::path::{Path, PathBuf};

/// Launcher-owned environment discovered without provisioning or loading extensions.
pub struct GentleShellEnvironment {
    pub agent_home: PathBuf,
    pub launcher_version: String,
}

/// Resolve Gentle Shell's effective home without launching Pi or provisioning packages.
///
/// # Errors
/// Returns a discovery error if the bounded launcher probe fails or omits its home.
pub fn inspect_gentle_shell(
    executable: &Path,
    selectors: &[String],
) -> Result<GentleShellEnvironment, DiscoveryError> {
    let mut arguments = vec!["--version"];
    arguments.extend(selectors.iter().map(String::as_str));
    let output = executable::run_command(executable, &arguments).map_err(|error| match error {
        probe::ProbeError::Timeout => DiscoveryError::VersionProbeTimeout,
        probe::ProbeError::OutputLimit => DiscoveryError::VersionProbeOutputLimit,
        probe::ProbeError::Io(source) => DiscoveryError::VersionCommand {
            command: "gentle-shell --version".into(),
            source,
        },
    })?;
    if output.status.success() {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if let Some(home) = line
                .strip_prefix("home ")
                .and_then(|line| line.split_once(' '))
                .map(|(_, home)| home)
                && !home.is_empty()
            {
                return Ok(GentleShellEnvironment {
                    agent_home: home.into(),
                    launcher_version: executable::first_non_empty_line(
                        &output.stdout,
                        &output.stderr,
                    ),
                });
            }
        }
    }
    Err(DiscoveryError::VersionCommandFailed {
        command: "gentle-shell --version".to_owned(),
        exit_code: output.status.code(),
        signal: exit_signal(output.status),
    })
}

/// Resolve a native user-owned credential command without publishing its output.
/// Failed, empty, oversized and timed-out resolutions return no credential.
#[must_use]
pub fn resolve_gentle_shell_command(command: &str) -> Option<nan_harness_core::SecretValue> {
    probe::secret_command(command)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn native_credential_commands_are_private_and_fail_closed() {
        let key = resolve_gentle_shell_command("printf 'synthetic-key\\n'").unwrap();
        key.with_secret(|value| assert_eq!(value, "synthetic-key"));
        assert!(resolve_gentle_shell_command("exit 7").is_none());
        assert!(resolve_gentle_shell_command("printf ''").is_none());
    }
}
