use super::*;

pub(crate) async fn terminate_desktop_or_child(
    child: &mut Child,
) -> Result<(), HermesDesktopError> {
    if running_desktop()?.is_some() {
        terminate_desktop().await
    } else {
        child.start_kill().map_err(HermesDesktopError::Terminate)?;
        terminate_desktop().await
    }
}

pub(crate) async fn terminate_desktop() -> Result<(), HermesDesktopError> {
    let mut quiet_since = None;
    loop {
        if let Some(process) = running_desktop()? {
            let _ = desktop_quiescence_reached(
                &mut quiet_since,
                Instant::now(),
                true,
                DESKTOP_QUIESCENCE_INTERVAL,
            );
            terminate_desktop_process(&process).await?;
        } else {
            if desktop_quiescence_reached(
                &mut quiet_since,
                Instant::now(),
                false,
                DESKTOP_QUIESCENCE_INTERVAL,
            ) {
                return Ok(());
            }
            tokio::time::sleep(PROCESS_POLL_INTERVAL).await;
        }
    }
}

pub(crate) fn desktop_quiescence_reached(
    quiet_since: &mut Option<Instant>,
    now: Instant,
    process_running: bool,
    interval: Duration,
) -> bool {
    if process_running {
        *quiet_since = None;
        return false;
    }
    let since = quiet_since.get_or_insert(now);
    now.duration_since(*since) >= interval
}

pub(crate) async fn terminate_desktop_process(
    process: &DesktopProcess,
) -> Result<(), HermesDesktopError> {
    request_process_termination(process)?;
    for _ in 0..60 {
        if !process_is_same(process)? {
            return Ok(());
        }
        tokio::time::sleep(PROCESS_POLL_INTERVAL).await;
    }
    force_process_termination(process)?;
    for _ in 0..40 {
        if !process_is_same(process)? {
            return Ok(());
        }
        tokio::time::sleep(PROCESS_POLL_INTERVAL).await;
    }
    Err(HermesDesktopError::DidNotTerminate)
}

#[cfg(target_os = "macos")]
pub(crate) fn request_process_termination(
    process: &DesktopProcess,
) -> Result<(), HermesDesktopError> {
    let status = Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "tell application id \"com.nousresearch.hermes\" to quit",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(HermesDesktopError::Terminate)?;
    if status.success() {
        Ok(())
    } else {
        terminate_pid(process.pid, false)
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn request_process_termination(
    process: &DesktopProcess,
) -> Result<(), HermesDesktopError> {
    terminate_pid(process.pid, false)
}

#[cfg(windows)]
pub(crate) fn request_process_termination(
    process: &DesktopProcess,
) -> Result<(), HermesDesktopError> {
    terminate_pid(process.pid, false)
}

pub(crate) fn force_process_termination(
    process: &DesktopProcess,
) -> Result<(), HermesDesktopError> {
    terminate_pid(process.pid, true)
}

#[cfg(unix)]
pub(crate) fn terminate_pid(pid: u32, force: bool) -> Result<(), HermesDesktopError> {
    let signal = if force { "-KILL" } else { "-TERM" };
    let status = Command::new("/bin/kill")
        .args([signal, &pid.to_string()])
        .status()
        .map_err(HermesDesktopError::Terminate)?;
    if status.success() || !pid_is_alive(pid)? {
        Ok(())
    } else {
        Err(HermesDesktopError::TerminateFailed(status.code()))
    }
}

#[cfg(windows)]
pub(crate) fn terminate_pid(pid: u32, force: bool) -> Result<(), HermesDesktopError> {
    let mut command = Command::new("taskkill");
    command.args(["/PID", &pid.to_string(), "/T"]);
    if force {
        command.arg("/F");
    }
    let status = command.status().map_err(HermesDesktopError::Terminate)?;
    if status.success() || !pid_is_alive(pid)? {
        Ok(())
    } else {
        Err(HermesDesktopError::TerminateFailed(status.code()))
    }
}

#[cfg(unix)]
pub(crate) fn running_desktop() -> Result<Option<DesktopProcess>, HermesDesktopError> {
    let output = Command::new("/bin/ps")
        .args(["-ww", "-axo", "pid=,lstart=,command="])
        .output()
        .map_err(HermesDesktopError::ProcessCheck)?;
    if !output.status.success() {
        return Err(HermesDesktopError::ProcessCheckFailed(output.status.code()));
    }
    let listing = String::from_utf8_lossy(&output.stdout);
    for line in listing.lines() {
        let trimmed = line.trim_start();
        let Some((pid, rest)) = trimmed.split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(pid) = pid.parse::<u32>() else {
            continue;
        };
        let rest = rest.trim_start();
        if rest.len() < 24 {
            continue;
        }
        let started = rest[..24].trim().to_owned();
        let command = rest[24..].trim();
        if desktop_main_command(command) {
            return Ok(Some(DesktopProcess { pid, started }));
        }
    }
    Ok(None)
}

#[cfg(unix)]
pub(crate) fn desktop_main_command(command: &str) -> bool {
    !command.contains("--type=")
        && (command.contains("/Hermes.app/Contents/MacOS/Hermes")
            || command.contains("/apps/desktop/release/linux-")
                && (command.ends_with("/hermes") || command.ends_with("/Hermes"))
            || command.contains("/apps/desktop/node_modules/electron/")
                && command.contains("apps/desktop"))
}

#[cfg(windows)]
pub(crate) fn running_desktop() -> Result<Option<DesktopProcess>, HermesDesktopError> {
    let script = "Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'Hermes.exe' -or ($_.Name -eq 'electron.exe' -and $_.CommandLine -match '[\\/]apps[\\/]desktop') } | Select-Object ProcessId,CreationDate,Name,CommandLine | ConvertTo-Json -Compress";
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", script])
        .output()
        .map_err(HermesDesktopError::ProcessCheck)?;
    if !output.status.success() {
        return Err(HermesDesktopError::ProcessCheckFailed(output.status.code()));
    }
    let value = String::from_utf8_lossy(&output.stdout);
    if value.trim().is_empty() {
        return Ok(None);
    }
    parse_windows_process_listing(&value)
}

#[cfg(any(windows, test))]
pub(crate) fn parse_windows_process_listing(
    value: &str,
) -> Result<Option<DesktopProcess>, HermesDesktopError> {
    let parsed: serde_json::Value =
        serde_json::from_str(value).map_err(HermesDesktopError::ParseProcessListing)?;
    let records = match &parsed {
        serde_json::Value::Array(records) => records.iter().collect::<Vec<_>>(),
        serde_json::Value::Object(_) => vec![&parsed],
        _ => return Err(HermesDesktopError::InvalidProcessListing),
    };
    let mut main_processes = Vec::new();
    for record in records {
        let name = record["Name"]
            .as_str()
            .ok_or(HermesDesktopError::InvalidProcessListing)?;
        let command = record["CommandLine"]
            .as_str()
            .ok_or(HermesDesktopError::InvalidProcessListing)?;
        if !windows_desktop_main_process(name, command) {
            continue;
        }
        let pid = record["ProcessId"]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .ok_or(HermesDesktopError::InvalidProcessListing)?;
        let started = record["CreationDate"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        main_processes.push(DesktopProcess { pid, started });
    }
    match main_processes.len() {
        0 => Ok(None),
        1 => Ok(main_processes.pop()),
        _ => Err(HermesDesktopError::AmbiguousDesktopProcesses),
    }
}

#[cfg(any(windows, test))]
pub(crate) fn windows_desktop_main_process(name: &str, command: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let command = command.to_ascii_lowercase();
    if command.contains("--type=") {
        return false;
    }
    name == "hermes.exe"
        || name == "electron.exe"
            && (command.contains("/apps/desktop") || command.contains("\\apps\\desktop"))
}

pub(crate) fn process_is_same(process: &DesktopProcess) -> Result<bool, HermesDesktopError> {
    Ok(running_desktop()?.as_ref() == Some(process))
}

#[cfg(unix)]
pub(crate) fn pid_is_alive(pid: u32) -> Result<bool, HermesDesktopError> {
    let status = Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(HermesDesktopError::ProcessCheck)?;
    Ok(status.success())
}

#[cfg(windows)]
pub(crate) fn pid_is_alive(pid: u32) -> Result<bool, HermesDesktopError> {
    let output = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .map_err(HermesDesktopError::ProcessCheck)?;
    Ok(output.status.success()
        && String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\"")))
}

pub(crate) fn live_update_owner(path: &Path) -> Result<Option<u32>, HermesDesktopError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(HermesDesktopError::ReadUpdateMarker(error)),
    };
    let Some(pid) = contents
        .lines()
        .next()
        .and_then(|line| line.trim().parse::<u32>().ok())
    else {
        return Ok(None);
    };
    pid_is_alive(pid).map(|alive| alive.then_some(pid))
}

pub(crate) fn marker_fingerprint(path: &Path) -> Option<MarkerFingerprint> {
    let metadata = fs::metadata(path).ok()?;
    Some(MarkerFingerprint {
        modified: metadata.modified().ok(),
        length: metadata.len(),
    })
}
