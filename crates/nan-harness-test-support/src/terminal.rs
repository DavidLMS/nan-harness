use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::Duration;
use thiserror::Error;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct TerminalCommand {
    program: PathBuf,
    arguments: Vec<OsString>,
    current_directory: PathBuf,
    environment: BTreeMap<OsString, OsString>,
    timeout: Duration,
}

impl TerminalCommand {
    #[must_use]
    pub fn new(program: impl Into<PathBuf>, current_directory: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            arguments: Vec::new(),
            current_directory: current_directory.into(),
            environment: BTreeMap::new(),
            timeout: Duration::from_mins(1),
        }
    }

    #[must_use]
    pub fn args<I, S>(mut self, arguments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.arguments.extend(arguments.into_iter().map(Into::into));
        self
    }

    #[must_use]
    pub fn env(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.environment.insert(name.into(), value.into());
        self
    }

    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Runs the command with captured output and a hard timeout.
    ///
    /// # Errors
    ///
    /// Returns [`TerminalError`] when the process cannot start, exceeds its timeout, or cannot be
    /// reaped.
    pub async fn run(self) -> Result<TerminalOutput, TerminalError> {
        let mut command = Command::new(&self.program);
        command
            .args(&self.arguments)
            .current_dir(&self.current_directory)
            .envs(&self.environment)
            .kill_on_drop(true);
        let output = tokio::time::timeout(self.timeout, command.output())
            .await
            .map_err(|_| TerminalError::Timeout {
                program: self.program.clone(),
                timeout: self.timeout,
            })?
            .map_err(|source| TerminalError::Execute {
                program: self.program,
                source,
            })?;
        Ok(TerminalOutput {
            status: output.status,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[derive(Debug)]
pub struct TerminalOutput {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

impl TerminalOutput {
    #[must_use]
    pub fn diagnostic(&self) -> String {
        format!(
            "status: {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.status, self.stdout, self.stderr
        )
    }
}

#[derive(Debug, Error)]
pub enum TerminalError {
    #[error("command '{}' exceeded its {timeout:?} timeout", program.display())]
    Timeout { program: PathBuf, timeout: Duration },
    #[error("could not execute '{}': {source}", program.display())]
    Execute {
        program: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub fn os(value: impl AsRef<OsStr>) -> OsString {
    value.as_ref().to_owned()
}

pub fn path(value: impl AsRef<Path>) -> OsString {
    value.as_ref().as_os_str().to_owned()
}
