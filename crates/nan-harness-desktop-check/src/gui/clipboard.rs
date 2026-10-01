//! Bounded macOS clipboard transport for the disposable hosted experiment.

use crate::report::Reason;
use zeroize::Zeroizing;

#[cfg(any(target_os = "macos", test))]
const OUTPUT_LIMIT: u64 = 64 * 1024;

#[cfg(any(target_os = "macos", test))]
fn decode(bytes: &[u8]) -> Result<Zeroizing<String>, Reason> {
    if bytes.len() as u64 > OUTPUT_LIMIT {
        return Err(Reason::ActionUnsupported);
    }
    String::from_utf8(bytes.to_vec())
        .map(Zeroizing::new)
        .map_err(|_| Reason::ActionUnsupported)
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{Reason, Zeroizing};
    use std::io::{Read as _, Write as _};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    pub(super) fn run(input: Option<&str>) -> Result<Zeroizing<String>, Reason> {
        let executable = if input.is_some() {
            "/usr/bin/pbcopy"
        } else {
            "/usr/bin/pbpaste"
        };
        let mut child = Command::new(executable)
            .env_clear()
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

pub(super) fn write(value: &str) -> Result<(), Reason> {
    if value.len() > 1024 {
        return Err(Reason::ActionUnsupported);
    }
    #[cfg(target_os = "macos")]
    {
        macos::run(Some(value)).map(|_| ())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(Reason::ActionUnsupported)
    }
}

pub(super) fn read() -> Result<Zeroizing<String>, Reason> {
    #[cfg(target_os = "macos")]
    {
        macos::run(None)
    }
    #[cfg(not(target_os = "macos"))]
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
