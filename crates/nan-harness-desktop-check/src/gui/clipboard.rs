//! Bounded native clipboard transports for disposable hosted probes.

use crate::report::Reason;
use zeroize::Zeroizing;

#[cfg(any(target_os = "macos", target_os = "linux", test))]
const OUTPUT_LIMIT: u64 = 64 * 1024;

#[cfg(any(target_os = "macos", target_os = "linux", test))]
fn decode(bytes: &[u8]) -> Result<Zeroizing<String>, Reason> {
    if bytes.len() as u64 > OUTPUT_LIMIT {
        return Err(Reason::ActionUnsupported);
    }
    String::from_utf8(bytes.to_vec())
        .map(Zeroizing::new)
        .map_err(|_| Reason::ActionUnsupported)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod transport {
    use super::{Reason, Zeroizing};
    use std::io::{Read as _, Write as _};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    pub(super) fn run(input: Option<&str>) -> Result<Zeroizing<String>, Reason> {
        let executable = if cfg!(target_os = "linux") {
            "/usr/bin/xclip"
        } else if input.is_some() {
            "/usr/bin/pbcopy"
        } else {
            "/usr/bin/pbpaste"
        };
        let mut command = Command::new(executable);
        command.env_clear();
        #[cfg(target_os = "linux")]
        {
            command.args(["-selection", "clipboard", "-out"]);
            for key in ["DISPLAY", "XAUTHORITY"] {
                if let Some(value) = std::env::var_os(key) {
                    command.env(key, value);
                }
            }
        }
        let mut child = command
            .env("LC_CTYPE", "UTF-8")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| Reason::ActionUnsupported)?;
        let outcome = std::thread::scope(|scope| {
            let mut stdin = child.stdin.take().ok_or(Reason::ActionUnsupported)?;
            let stdout = child.stdout.take().ok_or(Reason::ActionUnsupported)?;
            let writer = scope.spawn(move || {
                if let Some(value) = input {
                    stdin.write_all(value.as_bytes())?;
                }
                Ok::<_, std::io::Error>(())
            });
            let reader = scope.spawn(move || {
                let mut bytes = Zeroizing::new(Vec::new());
                stdout
                    .take(super::OUTPUT_LIMIT + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| Reason::ActionUnsupported)?;
                super::decode(&bytes)
            });
            let deadline = Instant::now() + Duration::from_secs(3);
            let status = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break Some(status),
                    Err(_) => break None,
                    Ok(None) if Instant::now() >= deadline => break None,
                    Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                }
            };
            if status.is_none() {
                let _ = child.kill();
                let _ = child.wait();
            }
            let written = writer.join().map_err(|_| Reason::ActionUnsupported)?;
            let output = reader.join().map_err(|_| Reason::ActionUnsupported)?;
            if !status.is_some_and(|status| status.success()) {
                return Err(Reason::ActionUnsupported);
            }
            written.map_err(|_| Reason::ActionUnsupported)?;
            output
        });
        if outcome.is_err() {
            let _ = child.kill();
            let _ = child.wait();
        }
        outcome
    }
}

#[cfg(target_os = "linux")]
mod x11 {
    use super::Reason;
    use std::io::Write as _;
    use std::process::{Child, Command, Stdio};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    // X11 clipboard data lives in its selection owner. Keep this exact child
    // until replacement or confirmed empty cleanup; never detach a server.
    static OWNER: Mutex<Option<Child>> = Mutex::new(None);

    fn reap(child: &mut Child) {
        let _ = child.kill();
        let _ = child.wait();
    }

    pub(super) fn stop() -> Result<(), Reason> {
        let mut owner = OWNER.lock().map_err(|_| Reason::IsolationUnavailable)?;
        if let Some(mut child) = owner.take() {
            reap(&mut child);
        }
        Ok(())
    }

    pub(super) fn write(value: &str) -> Result<(), Reason> {
        stop()?;
        let mut command = Command::new("/usr/bin/xclip");
        command.env_clear();
        for key in ["DISPLAY", "XAUTHORITY"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let mut child = command
            .args(["-selection", "clipboard", "-in", "-quiet"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| Reason::ActionUnsupported)?;
        let written = child
            .stdin
            .take()
            .ok_or(Reason::ActionUnsupported)
            .and_then(|mut stdin| {
                stdin
                    .write_all(value.as_bytes())
                    .map_err(|_| Reason::ActionUnsupported)
            });
        if let Err(reason) = written {
            reap(&mut child);
            return Err(reason);
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if !matches!(child.try_wait(), Ok(None)) {
                reap(&mut child);
                return Err(Reason::ActionUnsupported);
            }
            if super::transport::run(None).is_ok_and(|actual| actual.as_str() == value) {
                let Ok(mut owner) = OWNER.lock() else {
                    reap(&mut child);
                    return Err(Reason::IsolationUnavailable);
                };
                *owner = Some(child);
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        reap(&mut child);
        Err(Reason::ActionUnsupported)
    }
}

pub(super) fn write(value: &str) -> Result<(), Reason> {
    if value.len() > 1024 {
        return Err(Reason::ActionUnsupported);
    }
    #[cfg(target_os = "macos")]
    {
        transport::run(Some(value)).map(|_| ())
    }
    #[cfg(target_os = "linux")]
    {
        x11::write(value)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err(Reason::ActionUnsupported)
    }
}

pub(super) fn read() -> Result<Zeroizing<String>, Reason> {
    #[cfg(target_os = "macos")]
    {
        transport::run(None)
    }
    #[cfg(target_os = "linux")]
    {
        let value = transport::run(None)?;
        if value.is_empty() {
            x11::stop()?;
        }
        Ok(value)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err(Reason::ActionUnsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_transport_rejects_invalid_and_over_budget_payloads() {
        assert!(write(&"x".repeat(1025)).is_err());
        assert!(decode(&[0xff]).is_err());
        assert!(decode(&vec![b'x'; 65537]).is_err());
        assert_eq!(decode(b"synthetic").unwrap().as_str(), "synthetic");
    }
}
