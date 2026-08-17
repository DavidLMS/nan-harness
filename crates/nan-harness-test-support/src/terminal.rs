use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::Duration;
use thiserror::Error;
use tokio::process::Command;

#[derive(Debug, Clone)]
struct TerminalResponse {
    prompt: String,
    response: String,
}

#[derive(Debug, Clone)]
pub struct TerminalCommand {
    program: PathBuf,
    arguments: Vec<OsString>,
    current_directory: PathBuf,
    environment: BTreeMap<OsString, OsString>,
    terminal_response: Option<TerminalResponse>,
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
            terminal_response: None,
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
    pub fn respond_when(mut self, prompt: impl Into<String>, response: impl Into<String>) -> Self {
        self.terminal_response = Some(TerminalResponse {
            prompt: prompt.into(),
            response: response.into(),
        });
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
        let mut command = if let Some(response) = &self.terminal_response {
            let mut command = Command::new("/usr/bin/expect");
            command
                .arg("-c")
                .arg(expect_script(&self.program, &self.arguments, response));
            command
        } else {
            let mut command = Command::new(&self.program);
            command.args(&self.arguments);
            command
        };
        command
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

fn expect_script(program: &Path, arguments: &[OsString], response: &TerminalResponse) -> String {
    let mut spawn = format!("spawn {}", tcl_word(program.as_os_str()));
    for argument in arguments {
        spawn.push(' ');
        spawn.push_str(&tcl_word(argument));
    }
    format!(
        concat!(
            "set timeout 5\n",
            "{}\n",
            "expect {{\n",
            "  {} {{\n",
            "    send -- \"{}\"\n",
            "    after 100\n",
            "    send -- \"\\r\"\n",
            "    exp_continue\n",
            "  }}\n",
            "  timeout {{\n",
            "    send -- \"{}\"\n",
            "    after 100\n",
            "    send -- \"\\r\"\n",
            "    exp_continue\n",
            "  }}\n",
            "  eof {{}}\n",
            "}}\n",
            "catch wait result\n",
            "exit [lindex $result 3]\n"
        ),
        spawn,
        tcl_word(OsStr::new(&response.prompt)),
        tcl_double_quoted(&response.response),
        tcl_double_quoted(&response.response),
    )
}

fn tcl_word(value: &OsStr) -> String {
    let value = value.to_string_lossy();
    format!("{{{}}}", value.replace('{', "\\{").replace('}', "\\}"))
}

fn tcl_double_quoted(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('[', "\\[")
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::TerminalCommand;

    #[tokio::test]
    async fn responds_to_an_interactive_terminal_prompt() {
        let workspace = tempfile::tempdir().expect("workspace should exist");
        let output = TerminalCommand::new("/bin/sh", workspace.path())
            .args([
                "-c",
                "printf prompt; read value; printf 'received:%s' \"$value\"",
            ])
            .respond_when("prompt", "answer")
            .run()
            .await
            .expect("command should complete");

        assert!(output.status.success());
        assert!(output.stdout.contains("received:answer"));
    }
}
