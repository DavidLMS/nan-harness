use std::fs;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt as _;
use tokio::process::Command;

pub(crate) const SSH_RETRY_DELAY: Duration = Duration::from_secs(2);
const SSH_TRANSPORT_ATTEMPTS: u8 = 4;
const SSH_TRANSPORT_RETRY_DELAY: Duration = Duration::from_secs(5);

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
    for attempt in 1..=SSH_TRANSPORT_ATTEMPTS {
        match run_remote_script_attempt(
            ip,
            script,
            log_path,
            super::remaining(deadline, timeout),
            attempt > 1,
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(error) if error.retryable && attempt < SSH_TRANSPORT_ATTEMPTS => {
                tokio::time::sleep(super::remaining(deadline, SSH_TRANSPORT_RETRY_DELAY)).await;
            }
            Err(error) => return Err(error.detail),
        }
    }
    unreachable!("the bounded SSH transport loop always returns")
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
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| RemoteScriptAttemptError::retryable("SSH stdin is unavailable"))?;
    stdin.write_all(script.as_bytes()).await.map_err(|error| {
        RemoteScriptAttemptError::retryable(format!("could not send the remote script: {error}"))
    })?;
    drop(stdin);
    let status = tokio::time::timeout(timeout, child.wait())
        .await
        .map_err(|_| RemoteScriptAttemptError::fatal("remote step timed out"))?
        .map_err(|error| {
            RemoteScriptAttemptError::retryable(format!("could not wait for SSH: {error}"))
        })?;
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
