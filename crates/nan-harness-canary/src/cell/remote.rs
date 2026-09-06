use retry::RemoteScriptAttempt;
use std::fs;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt as _;
use tokio::process::{Child, ChildStdin, Command};

mod retry;
#[cfg(all(test, unix))]
mod tests;

pub(crate) const SSH_RETRY_DELAY: Duration = Duration::from_secs(2);
const SSH_TRANSPORT_RETRY_DELAY: Duration = Duration::from_secs(5);
/// Bounds the cleanup that follows a timed-out or failed attempt: the child is
/// already unresponsive, so waiting for it cannot be open-ended.
const SSH_CHILD_CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) async fn wait_for_ssh(vm_name: &str, timeout: Duration) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let ip = command_text("tart", &["ip", vm_name], Duration::from_secs(10)).await;
        if let Ok(ip) = ip {
            let ip = ip.trim();
            if !ip.is_empty()
                && run_ssh_command(ip, "true", Duration::from_secs(10))
                    .await
                    .is_ok()
            {
                return Ok(ip.to_owned());
            }
        }
        if Instant::now() >= deadline {
            return Err("SSH readiness timed out".to_owned());
        }
        tokio::time::sleep(SSH_RETRY_DELAY).await;
    }
}

pub(crate) async fn run_remote_script(
    ip: &str,
    script: &str,
    log_path: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let runner = SshScriptAttempt {
        ip,
        script,
        log_path,
    };
    retry::run_with_retries(&runner, deadline, timeout, SSH_TRANSPORT_RETRY_DELAY).await
}

struct SshScriptAttempt<'a> {
    ip: &'a str,
    script: &'a str,
    log_path: &'a Path,
}

impl RemoteScriptAttempt for SshScriptAttempt<'_> {
    async fn attempt(
        &self,
        timeout: Duration,
        append_log: bool,
    ) -> Result<(), RemoteScriptAttemptError> {
        run_remote_script_attempt(self.ip, self.script, self.log_path, timeout, append_log).await
    }
}

async fn run_remote_script_attempt(
    ip: &str,
    script: &str,
    log_path: &Path,
    timeout: Duration,
    append_log: bool,
) -> Result<(), RemoteScriptAttemptError> {
    let stdout = if append_log {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
    } else {
        fs::File::create(log_path)
    }
    .map_err(|error| {
        RemoteScriptAttemptError::fatal(format!("could not create the private step log: {error}"))
    })?;
    let stderr = stdout.try_clone().map_err(|error| {
        RemoteScriptAttemptError::fatal(format!("could not clone the private step log: {error}"))
    })?;
    let mut command = ssh_command(ip);
    command
        .args(["bash", "-s"])
        .stdin(Stdio::piped())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| {
        RemoteScriptAttemptError::retryable(format!("could not start SSH: {error}"))
    })?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| RemoteScriptAttemptError::retryable("SSH stdin is unavailable"))?;
    classify_exit_status(send_script_and_wait(&mut child, stdin, script, timeout).await?)
}

/// One SSH failure as seen by the attempt that produced it. The retry policy
/// below only reads whether the failure is worth another attempt.
struct RemoteScriptAttemptError {
    detail: String,
    retryable: bool,
}

impl RemoteScriptAttemptError {
    fn retryable(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
            retryable: true,
        }
    }

    fn fatal(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
            retryable: false,
        }
    }
}

/// Sends the step script to an already spawned SSH child and waits for that
/// child to exit, all under a single budget. Sharing one budget is what keeps a
/// remote that never drains its stdin from outliving the attempt: blocking on
/// the pipe spends the same time that waiting for the exit would.
///
/// A timeout or an IO failure closes the script pipe and terminates the child
/// this attempt owns. Cleanup has its own bounded wait and retains
/// `kill_on_drop` as a fallback if the child has not exited.
async fn send_script_and_wait(
    child: &mut Child,
    stdin: ChildStdin,
    script: &str,
    timeout: Duration,
) -> Result<ExitStatus, RemoteScriptAttemptError> {
    let outcome = tokio::time::timeout(timeout, write_script_then_wait(child, stdin, script))
        .await
        .unwrap_or_else(|_| Err(RemoteScriptAttemptError::fatal("remote step timed out")));
    if outcome.is_err() {
        terminate_and_reap(child).await;
    }
    outcome
}

/// The unbounded core of an attempt: the script pipe closes on the way out of
/// this future, whether the write finished or the caller's budget expired.
async fn write_script_then_wait(
    child: &mut Child,
    mut stdin: ChildStdin,
    script: &str,
) -> Result<ExitStatus, RemoteScriptAttemptError> {
    stdin.write_all(script.as_bytes()).await.map_err(|error| {
        RemoteScriptAttemptError::retryable(format!("could not send the remote script: {error}"))
    })?;
    drop(stdin);
    child.wait().await.map_err(|error| {
        RemoteScriptAttemptError::retryable(format!("could not wait for SSH: {error}"))
    })
}

/// Stops the child of a failed attempt and collects it, reporting nothing: the
/// failure that triggered the cleanup is the one worth telling the caller
/// about, and the child may already have exited on its own. `kill_on_drop`
/// remains the fallback for the child that outlasts this bounded wait.
async fn terminate_and_reap(child: &mut Child) {
    let _ = child.start_kill();
    let _ = tokio::time::timeout(SSH_CHILD_CLEANUP_TIMEOUT, child.wait()).await;
}

fn classify_exit_status(status: ExitStatus) -> Result<(), RemoteScriptAttemptError> {
    if status.success() {
        Ok(())
    } else if status.code() == Some(255) {
        Err(RemoteScriptAttemptError::retryable(
            "SSH transport exited with status 255",
        ))
    } else {
        Err(RemoteScriptAttemptError::fatal(format!(
            "remote step exited with {}",
            status
                .code()
                .map_or_else(|| "a signal".to_owned(), |code| format!("status {code}"))
        )))
    }
}

async fn run_ssh_command(ip: &str, remote: &str, timeout: Duration) -> Result<(), String> {
    let mut command = ssh_command(ip);
    command
        .arg(remote)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let status = tokio::time::timeout(timeout, command.status())
        .await
        .map_err(|_| "SSH command timed out".to_owned())?
        .map_err(|error| format!("could not start SSH: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("SSH command failed".to_owned())
    }
}

pub(crate) fn ssh_command(ip: &str) -> Command {
    let mut command = Command::new("sshpass");
    command.args([
        "-p",
        "admin",
        "ssh",
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "UserKnownHostsFile=/dev/null",
        "-o",
        "LogLevel=ERROR",
        "-o",
        "IdentitiesOnly=yes",
        "-o",
        "PreferredAuthentications=password",
        "-o",
        "ConnectTimeout=10",
        &format!("admin@{ip}"),
    ]);
    command
}

pub(crate) async fn command_text(
    program: &str,
    arguments: &[&str],
    timeout: Duration,
) -> Result<String, String> {
    let output = tokio::time::timeout(
        timeout,
        Command::new(program)
            .args(arguments)
            .stdin(Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| format!("{program} timed out"))?
    .map_err(|error| format!("could not start {program}: {error}"))?;
    if !output.status.success() {
        return Err(format!("{program} failed"));
    }
    String::from_utf8(output.stdout).map_err(|_| format!("{program} returned non-UTF-8 output"))
}

pub(crate) async fn run_host_command(
    program: &str,
    arguments: &[&str],
    timeout: Duration,
) -> Result<Duration, CommandFailure> {
    let started = Instant::now();
    let result = tokio::time::timeout(
        timeout,
        Command::new(program)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status(),
    )
    .await;
    let duration = started.elapsed();
    match result {
        Ok(Ok(status)) if status.success() => Ok(duration),
        Ok(Ok(status)) => Err(CommandFailure {
            duration,
            detail: format!(
                "{program} exited with {}",
                status
                    .code()
                    .map_or_else(|| "a signal".to_owned(), |code| format!("status {code}"))
            ),
        }),
        Ok(Err(error)) => Err(CommandFailure {
            duration,
            detail: format!("could not start {program}: {error}"),
        }),
        Err(_) => Err(CommandFailure {
            duration,
            detail: format!("{program} timed out"),
        }),
    }
}

pub(crate) struct CommandFailure {
    pub(crate) duration: Duration,
    pub(crate) detail: String,
}
