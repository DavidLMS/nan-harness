//! Read-only process absence supplements window absence before restoration.
use crate::report::Reason;
#[cfg(windows)]
use nan_harness_core::DesktopHarnessKind;

#[cfg(windows)]
pub(super) fn ensure_absent(kind: DesktopHarnessKind) -> Result<(), Reason> {
    if kind == DesktopHarnessKind::ChatGpt {
        use std::time::{Duration, Instant};
        return wait_absent(
            inspect,
            Instant::now,
            std::thread::sleep,
            Instant::now() + Duration::from_secs(2),
        );
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn wait_absent(
    mut query: impl FnMut(std::time::Instant) -> Result<bool, Reason>,
    mut now: impl FnMut() -> std::time::Instant,
    mut pause: impl FnMut(std::time::Duration),
    deadline: std::time::Instant,
) -> Result<(), Reason> {
    loop {
        if now() >= deadline {
            return Err(Reason::CleanupFailed);
        }
        let present = query(deadline)?;
        if now() >= deadline {
            return Err(Reason::CleanupFailed);
        }
        if !present {
            return Ok(());
        }
        pause(std::time::Duration::from_millis(50).min(deadline.saturating_duration_since(now())));
    }
}

#[cfg(any(windows, test))]
fn csv_presence(bytes: &[u8], inspector_pid: u32) -> Result<bool, Reason> {
    if bytes.is_empty() || bytes.len() > 65536 {
        return Err(Reason::DesktopUnavailable);
    }
    let mut found = false;
    let mut inspector = false;
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let mut fields = Vec::new();
        let mut position = 0;
        while position < line.len() {
            if line[position] != b'"' {
                return Err(Reason::DesktopUnavailable);
            }
            position += 1;
            let start = position;
            while position < line.len() && line[position] != b'"' {
                position += 1;
            }
            if position == line.len() {
                return Err(Reason::DesktopUnavailable);
            }
            fields.push(&line[start..position]);
            position += 1;
            if position < line.len() {
                if line[position] != b',' || position + 1 == line.len() {
                    return Err(Reason::DesktopUnavailable);
                }
                position += 1;
            }
        }
        if fields.len() != 5 {
            return Err(Reason::DesktopUnavailable);
        }
        found |= fields[0].eq_ignore_ascii_case(b"ChatGPT.exe");
        inspector |= fields[0].eq_ignore_ascii_case(b"tasklist.exe")
            && fields[1] == inspector_pid.to_string().as_bytes();
    }
    if !inspector {
        return Err(Reason::DesktopUnavailable);
    }
    Ok(found)
}

#[cfg(windows)]
fn inspect(deadline: std::time::Instant) -> Result<bool, Reason> {
    use std::io::{Read as _, Seek as _};
    use std::os::windows::process::CommandExt as _;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let unavailable = || Reason::DesktopUnavailable;
    let root = std::path::PathBuf::from(std::env::var_os("SystemRoot").ok_or_else(unavailable)?);
    let canonical_root = root.canonicalize().map_err(|_| unavailable())?;
    let path = root.join("System32/tasklist.exe");
    let executable = path.canonicalize().map_err(|_| unavailable())?;
    if !root.is_absolute()
        || !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file())
        || !executable.starts_with(&canonical_root)
    {
        return Err(unavailable());
    }
    let mut output = tempfile::tempfile().map_err(|_| unavailable())?;
    if Instant::now() >= deadline {
        return Err(unavailable());
    }
    let mut child = Command::new(&executable)
        .args(["/FO", "CSV", "/NH"])
        .env_clear()
        .env("SystemRoot", &root)
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .stdout(output.try_clone().map_err(|_| unavailable())?)
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| unavailable())?;
    let outcome = loop {
        if Instant::now() >= deadline
            || output
                .metadata()
                .ok()
                .is_none_or(|metadata| metadata.len() > 65536)
        {
            break Err(unavailable());
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break Ok(()),
            Ok(Some(_)) | Err(_) => break Err(unavailable()),
            Ok(None) => std::thread::sleep(
                Duration::from_millis(10).min(deadline.saturating_duration_since(Instant::now())),
            ),
        }
    };
    if outcome.is_err() {
        // Only this inspector Child is terminated; app termination belongs to
        // the pre-existing owned JobObject cleanup, never an enumerated PID.
        let _ = child.kill();
        let _ = child.wait();
    }
    outcome?;
    output.rewind().map_err(|_| unavailable())?;
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    output
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| unavailable())?;
    csv_presence(&bytes, child.id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        time::{Duration, Instant},
    };
    const INSPECTOR: &[u8] = b"\"tasklist.exe\",\"123\",\"Console\",\"1\",\"1,000 K\"\r\n";
    #[test]
    fn exact_image_and_owned_inspector_are_required() {
        assert_eq!(csv_presence(INSPECTOR, 123), Ok(false));
        let mut rows = INSPECTOR.to_vec();
        rows.extend_from_slice(b"\"cHaTgPt.exe\",\"456\",\"Console\",\"1\",\"100 K\"\r\n");
        assert_eq!(csv_presence(&rows, 123), Ok(true));
        assert!(csv_presence(&rows, 124).is_err());
        let mut foreign = INSPECTOR.to_vec();
        foreign.extend_from_slice(
            b"\"ChatGPTHelper.exe\",\"456\",\"ChatGPT.exe\",\"1\",\"100 K\"\r\n",
        );
        assert_eq!(csv_presence(&foreign, 123), Ok(false));
        for row in [
            b"".as_slice(),
            b"INFO: No tasks are running",
            b"INFORMATION: Keine Aufgaben",
            b"\"ChatGPT.exe\",\"456\"",
            b"\"broken",
        ] {
            assert!(csv_presence(row, 123).is_err());
        }
        assert!(csv_presence(&vec![b'x'; 65537], 123).is_err());
    }
    #[test]
    fn polling_uses_one_deadline_and_never_queries_after_it() {
        let start = Instant::now();
        let clock = Cell::new(start);
        let calls = Cell::new(0);
        let deadline = start + Duration::from_millis(100);
        let result = wait_absent(
            |bound| {
                assert_eq!(bound, deadline);
                calls.set(calls.get() + 1);
                Ok(true)
            },
            || clock.get(),
            |duration| clock.set(clock.get() + duration),
            deadline,
        );
        assert_eq!(result, Err(Reason::CleanupFailed));
        assert_eq!(calls.get(), 2);
    }
    #[test]
    fn late_absence_cannot_pass_or_reset_deadline() {
        let start = Instant::now();
        let clock = Cell::new(start);
        let deadline = start + Duration::from_millis(100);
        assert_eq!(
            wait_absent(
                |bound| {
                    assert_eq!(bound, deadline);
                    clock.set(deadline);
                    Ok(false)
                },
                || clock.get(),
                |_| panic!("no retry"),
                deadline
            ),
            Err(Reason::CleanupFailed)
        );
    }
    #[test]
    fn delayed_absence_succeeds_but_query_failure_never_does() {
        let start = Instant::now();
        let clock = Cell::new(start);
        let calls = Cell::new(0);
        assert_eq!(
            wait_absent(
                |_| {
                    calls.set(calls.get() + 1);
                    Ok(calls.get() < 2)
                },
                || clock.get(),
                |duration| clock.set(clock.get() + duration),
                start + Duration::from_secs(2)
            ),
            Ok(())
        );
        assert_eq!(calls.get(), 2);
        assert_eq!(
            wait_absent(
                |_| Err(Reason::DesktopUnavailable),
                || start,
                |_| panic!("no retry"),
                start + Duration::from_secs(2)
            ),
            Err(Reason::DesktopUnavailable)
        );
    }
}
