use super::constants::{PRIME_KILL_TIMEOUT, PRIME_STATUS_TIMEOUT, PRIME_TERM_TIMEOUT};
use crate::terminal::TerminalCommand;
use nan_harness_core::HarnessKind;
use serde_json::Value;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub(crate) struct PrimeDaemonGuard {
    socket: Option<PathBuf>,
}

impl PrimeDaemonGuard {
    pub(crate) fn for_harness(kind: HarnessKind, workspace: &Path) -> Result<Self, std::io::Error> {
        if kind != HarnessKind::PrimeAgent {
            return Ok(Self { socket: None });
        }
        let socket = workspace.join("home/prime-agent.sock");
        if let Some(parent) = socket.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(Self {
            socket: Some(socket),
        })
    }

    pub(crate) async fn cleanup(&mut self) -> Result<(), String> {
        let Some(socket) = self.socket.clone() else {
            return Ok(());
        };
        let pids = owned_prime_pids(&socket).await?;
        if pids.is_empty() {
            self.socket = None;
            return Ok(());
        }
        let mut targets = PrimeCleanupTargets::from_pids(&pids);
        signal_prime_targets(&targets, false).await?;
        match wait_for_prime_cleanup(&socket, false, PRIME_TERM_TIMEOUT).await {
            Ok(true) => {
                self.socket = None;
                return Ok(());
            }
            Ok(false) => {}
            Err(error) => {
                return Err(format!(
                    "Prime status inspection failed during cleanup: {error}"
                ));
            }
        }

        let remaining = match owned_prime_pids(&socket).await {
            Ok(pids) => pids,
            Err(error) => {
                return Err(format!(
                    "Prime status inspection failed during cleanup: {error}"
                ));
            }
        };
        if remaining.is_empty() {
            self.socket = None;
            return Ok(());
        }
        targets = PrimeCleanupTargets::from_pids(&remaining);
        signal_prime_targets(&targets, true).await?;
        match wait_for_prime_cleanup(&socket, true, PRIME_KILL_TIMEOUT).await {
            Ok(true) => {
                self.socket = None;
                Ok(())
            }
            Ok(false) => Err("owned Prime daemon remained after cleanup".to_owned()),
            Err(error) => Err(format!(
                "Prime status inspection failed during cleanup: {error}"
            )),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct PrimeCleanupTargets {
    pub(crate) pids: BTreeSet<u32>,
}

impl PrimeCleanupTargets {
    pub(crate) fn from_pids(pids: &[u32]) -> Self {
        Self {
            pids: pids.iter().copied().collect(),
        }
    }
}

async fn wait_for_prime_cleanup(
    socket: &Path,
    force: bool,
    timeout: Duration,
) -> Result<bool, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining == Duration::ZERO {
            return Ok(false);
        }
        let pids = tokio::time::timeout(remaining, owned_prime_pids(socket))
            .await
            .map_err(|_| {
                "Prime daemon status inspection exceeded its cleanup deadline".to_owned()
            })??;
        if pids.is_empty() {
            return Ok(true);
        }
        signal_prime_targets(&PrimeCleanupTargets::from_pids(&pids), force).await?;
        let sleep_for = remaining.min(Duration::from_millis(50));
        tokio::time::sleep(sleep_for).await;
    }
}

pub(crate) async fn owned_prime_pids(socket: &Path) -> Result<Vec<u32>, String> {
    let path = prime_status_path();
    let current_directory = socket.parent().unwrap_or_else(|| Path::new("."));
    let output = TerminalCommand::new("prime-agent", current_directory)
        .clear_environment()
        .env("PATH", path)
        .args(["status", "--json"])
        .timeout(PRIME_STATUS_TIMEOUT)
        .run()
        .await
        .map_err(|error| format!("could not inspect Prime daemons: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Prime daemon status command failed: {}",
            output.diagnostic()
        ));
    }
    let value: Value = serde_json::from_str(&output.stdout)
        .map_err(|error| format!("could not parse Prime daemon status: {error}"))?;
    owned_prime_pids_from_status(&value, socket)
}

pub(crate) fn prime_status_path() -> OsString {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = std::env::split_paths(&current).collect::<Vec<_>>();
    for fallback in PRIME_STATUS_PATH_FALLBACKS {
        let fallback = Path::new(fallback);
        if !paths.iter().any(|path| path == fallback) {
            paths.push(fallback.to_owned());
        }
    }
    std::env::join_paths(paths).unwrap_or(current)
}

#[cfg(unix)]
const PRIME_STATUS_PATH_FALLBACKS: &[&str] = &["/usr/sbin", "/sbin"];

#[cfg(not(unix))]
const PRIME_STATUS_PATH_FALLBACKS: &[&str] = &[];

pub(crate) fn owned_prime_pids_from_status(
    value: &Value,
    socket: &Path,
) -> Result<Vec<u32>, String> {
    let entries = value
        .as_array()
        .or_else(|| value.get("daemons").and_then(Value::as_array))
        .ok_or_else(|| "Prime daemon status did not contain a daemon list".to_owned())?;
    Ok(entries
        .iter()
        .filter(|entry| {
            entry.get("socketPath").and_then(Value::as_str)
                == Some(socket.to_string_lossy().as_ref())
        })
        .filter_map(|entry| entry.get("pid").and_then(Value::as_u64))
        .filter(|pid| *pid > 1)
        .filter_map(|pid| u32::try_from(pid).ok())
        .collect())
}

#[cfg(unix)]
pub(crate) fn signal_prime_targets_now(
    targets: &PrimeCleanupTargets,
    force: bool,
) -> Result<(), String> {
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let signal = if force {
        Signal::SIGKILL
    } else {
        Signal::SIGTERM
    };
    for pid in &targets.pids {
        let pid = i32::try_from(*pid).map_err(|_| "Prime pid was out of range".to_owned())?;
        match kill(Pid::from_raw(pid), signal) {
            Ok(()) | Err(Errno::ESRCH) => {}
            Err(error) => {
                return Err(format!("could not signal owned Prime daemon: {error}"));
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
async fn signal_prime_targets(targets: &PrimeCleanupTargets, force: bool) -> Result<(), String> {
    tokio::task::yield_now().await;
    signal_prime_targets_now(targets, force)
}

#[cfg(not(unix))]
async fn signal_prime_targets(targets: &PrimeCleanupTargets, _force: bool) -> Result<(), String> {
    for pid in &targets.pids {
        let output = TerminalCommand::new("taskkill", ".")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .timeout(PRIME_STATUS_TIMEOUT)
            .run()
            .await
            .map_err(|error| format!("could not terminate owned Prime daemon: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "taskkill could not terminate owned Prime daemon: {}",
                output.diagnostic()
            ));
        }
    }
    Ok(())
}
