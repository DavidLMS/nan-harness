use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt as _};
use tokio::process::{Child, Command};

const MAX_CAPTURE_BYTES: usize = 64 * 1024;
const PROCESS_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

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
    clear_environment: bool,
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
            clear_environment: false,
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
    pub const fn clear_environment(mut self) -> Self {
        self.clear_environment = true;
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

    /// Runs the command with bounded captured output and a hard timeout.
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
        if self.clear_environment {
            command.env_clear();
        }
        command
            .current_dir(&self.current_directory)
            .envs(&self.environment)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        {
            command.process_group(0);
        }
        let mut child = command.spawn().map_err(|source| TerminalError::Execute {
            program: self.program.clone(),
            source,
        })?;
        let pid = child.id();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| TerminalError::MissingOutput {
                stream: "stdout",
                program: self.program.clone(),
            })?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| TerminalError::MissingOutput {
                stream: "stderr",
                program: self.program.clone(),
            })?;
        let stdout_task = tokio::spawn(capture_output(stdout));
        let stderr_task = tokio::spawn(capture_output(stderr));
        let status = if let Ok(result) = tokio::time::timeout(self.timeout, child.wait()).await {
            result.map_err(|source| TerminalError::Execute {
                program: self.program.clone(),
                source,
            })?
        } else {
            terminate_owned_process(&mut child, pid).await;
            tokio::join!(
                reap_capture_bounded(stdout_task),
                reap_capture_bounded(stderr_task)
            );
            return Err(TerminalError::Timeout {
                program: self.program,
                timeout: self.timeout,
            });
        };
        let stdout = join_capture_bounded(stdout_task, &mut child, pid, &self.program).await;
        let stderr = join_capture_bounded(stderr_task, &mut child, pid, &self.program).await;
        let stdout = stdout?;
        let stderr = stderr?;
        Ok(TerminalOutput {
            status,
            stdout,
            stderr,
        })
    }
}

async fn reap_capture_bounded(mut task: tokio::task::JoinHandle<Result<String, std::io::Error>>) {
    if tokio::time::timeout(PROCESS_CLEANUP_TIMEOUT, &mut task)
        .await
        .is_err()
    {
        task.abort();
        let _ = task.await;
    }
}

async fn capture_output<R>(reader: R) -> Result<String, std::io::Error>
where
    R: AsyncRead + Unpin,
{
    let mut bytes = Vec::new();
    reader
        .take((MAX_CAPTURE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > MAX_CAPTURE_BYTES {
        bytes.truncate(MAX_CAPTURE_BYTES);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

async fn join_capture_bounded(
    mut task: tokio::task::JoinHandle<Result<String, std::io::Error>>,
    child: &mut Child,
    pid: Option<u32>,
    program: &Path,
) -> Result<String, TerminalError> {
    if let Ok(result) = tokio::time::timeout(PROCESS_CLEANUP_TIMEOUT, &mut task).await {
        result
            .map_err(|source| TerminalError::CaptureJoin {
                program: program.to_owned(),
                source,
            })?
            .map_err(|source| TerminalError::Capture {
                program: program.to_owned(),
                source,
            })
    } else {
        terminate_owned_process(child, pid).await;
        task.abort();
        let _ = task.await;
        Err(TerminalError::DescendantCleanup {
            program: program.to_owned(),
        })
    }
}

async fn terminate_owned_process(child: &mut Child, pid: Option<u32>) {
    #[cfg(not(unix))]
    let _ = pid;
    #[cfg(unix)]
    if let Some(pid) = pid.and_then(|pid| i32::try_from(pid).ok()) {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;

        let process_group = Pid::from_raw(-pid);
        let _ = kill(process_group, Signal::SIGTERM);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = kill(process_group, Signal::SIGKILL);
    }
    let _ = child.start_kill();
    let _ = tokio::time::timeout(PROCESS_CLEANUP_TIMEOUT, child.wait()).await;
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
    #[error(
        "could not capture {stream} for '{}': output pipe was unavailable",
        program.display()
    )]
    MissingOutput {
        stream: &'static str,
        program: PathBuf,
    },
    #[error("could not join output capture for '{}': {source}", program.display())]
    CaptureJoin {
        program: PathBuf,
        #[source]
        source: tokio::task::JoinError,
    },
    #[error("could not capture output for '{}': {source}", program.display())]
    Capture {
        program: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("owned descendants of '{}' did not close their output pipes", program.display())]
    DescendantCleanup { program: PathBuf },
}

pub fn os(value: impl AsRef<OsStr>) -> OsString {
    value.as_ref().to_owned()
}

pub fn path(value: impl AsRef<Path>) -> OsString {
    value.as_ref().as_os_str().to_owned()
}

#[cfg(test)]
mod tests {
    use super::TerminalCommand;
    #[cfg(unix)]
    use std::process::Command;
    #[cfg(unix)]
    use std::time::Duration;

    #[cfg(target_os = "macos")]
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

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_terminates_descendants_without_killing_unrelated_processes() {
        use nix::errno::Errno;
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;
        use std::os::unix::fs::PermissionsExt;

        let workspace = tempfile::tempdir().expect("workspace should exist");
        let child_pid_path = workspace.path().join("child.pid");
        let script = workspace.path().join("spawn-descendant.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nsh -c 'sleep 30' &\nprintf '%s' \"$!\" > '{}'\nsleep 30\n",
                child_pid_path.display()
            ),
        )
        .expect("script should be written");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))
            .expect("script should be executable");
        let mut unrelated = Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("unrelated process should start");

        let result = TerminalCommand::new(&script, workspace.path())
            .timeout(Duration::from_secs(2))
            .run()
            .await;
        assert!(result.is_err(), "the command should time out");
        let child_pid = std::fs::read_to_string(&child_pid_path)
            .expect("descendant pid should be recorded")
            .parse::<i32>()
            .expect("descendant pid should be numeric");
        for _ in 0..20 {
            if kill(Pid::from_raw(child_pid), None).is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert_eq!(kill(Pid::from_raw(child_pid), None), Err(Errno::ESRCH));
        assert!(
            unrelated
                .try_wait()
                .expect("unrelated status should work")
                .is_none()
        );
        let _ = kill(
            Pid::from_raw(i32::try_from(unrelated.id()).expect("pid should fit")),
            Signal::SIGKILL,
        );
        let _ = unrelated.wait();
    }

    #[tokio::test]
    async fn captured_output_is_bounded() {
        let workspace = tempfile::tempdir().expect("workspace should exist");
        let output = TerminalCommand::new("/bin/sh", workspace.path())
            .args(["-c", "head -c 100000 /dev/zero"])
            .run()
            .await
            .expect("command should complete");
        assert!(output.stdout.len() <= super::MAX_CAPTURE_BYTES);
    }
}
