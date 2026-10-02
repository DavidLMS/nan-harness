//! Bounded native clipboard transports for disposable hosted probes.

use crate::report::Reason;
use zeroize::Zeroizing;

#[cfg(any(target_os = "macos", target_os = "linux", windows, test))]
const OUTPUT_LIMIT: u64 = 64 * 1024;

#[cfg(any(target_os = "macos", target_os = "linux", windows, test))]
fn decode(bytes: &[u8]) -> Result<Zeroizing<String>, Reason> {
    if bytes.len() as u64 > OUTPUT_LIMIT {
        return Err(Reason::ActionUnsupported);
    }
    std::str::from_utf8(bytes)
        .map(|value| Zeroizing::new(value.to_owned()))
        .map_err(|_| Reason::ActionUnsupported)
}

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
mod transport {
    use super::{Reason, Zeroizing};
    use std::io::{Read as _, Write as _};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    pub(super) fn run(input: Option<&str>) -> Result<Zeroizing<String>, Reason> {
        let started = Instant::now();
        #[cfg(windows)]
        let native = crate::native::Native::new().map_err(|reason| {
            save_failure(input, "executable", started.elapsed());
            reason
        })?;
        #[cfg(windows)]
        let executable = native.executable();
        #[cfg(not(windows))]
        let executable = if cfg!(target_os = "linux") {
            "/usr/bin/xclip"
        } else if input.is_some() {
            "/usr/bin/pbcopy"
        } else {
            "/usr/bin/pbpaste"
        };
        let outcome = run_once(input, std::path::Path::new(executable));
        #[cfg(windows)]
        if let Err(stage) = outcome.as_ref() {
            save_failure(input, stage, started.elapsed());
        }
        #[cfg(not(windows))]
        let _ = started;
        outcome.map_err(|_| Reason::ActionUnsupported)
    }

    fn run_once(
        input: Option<&str>,
        executable: &std::path::Path,
    ) -> Result<Zeroizing<String>, &'static str> {
        let mut command = Command::new(executable);
        command.env_clear();
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            command.creation_flags(0x0800_0000);
            command.arg(match input {
                None => "--clipboard-read",
                Some("") => "--clipboard-clear",
                Some(_) => "--clipboard-write",
            });
            if let Some(root) = std::env::var_os("SystemRoot") {
                command.env("SystemRoot", root);
            }
        }
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
            .map_err(|_| "spawn")?;
        let outcome = std::thread::scope(|scope| {
            let mut stdin = child.stdin.take().ok_or("pipe")?;
            let stdout = child.stdout.take().ok_or("pipe")?;
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
                    .map_err(|_| "read")?;
                super::decode(&bytes).map_err(|_| "decode")
            });
            // Keep one bounded native attempt, including a cold hosted process.
            let deadline = Instant::now() + Duration::from_secs(if cfg!(windows) { 15 } else { 3 });
            let mut wait_failed = false;
            let status = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break Some(status),
                    Err(_) => {
                        wait_failed = true;
                        break None;
                    }
                    Ok(None) if Instant::now() >= deadline => break None,
                    Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                }
            };
            if status.is_none() {
                let _ = child.kill();
                let _ = child.wait();
            }
            let written = writer.join().map_err(|_| "thread")?;
            let output = reader.join().map_err(|_| "thread")?;
            if wait_failed {
                return Err("wait");
            }
            if status.is_none() {
                return Err("wait-timeout");
            }
            if !status.is_some_and(|status| status.success()) {
                #[cfg(windows)]
                return Err(match status.and_then(|status| status.code()) {
                    Some(2) => "invalid-input",
                    Some(4) => "output-invalid",
                    Some(5) => "clipboard-api",
                    _ => "nonzero",
                });
                #[cfg(not(windows))]
                return Err("nonzero");
            }
            written.map_err(|_| "write")?;
            output
        });
        if outcome.is_err() {
            let _ = child.kill();
            let _ = child.wait();
        }
        outcome
    }
    #[cfg(any(windows, test))]
    pub(super) fn failure_facts(
        input: Option<&str>,
        stage: &str,
        elapsed: Duration,
    ) -> serde_json::Value {
        let operation = match input {
            None => "read",
            Some("") => "clear",
            Some(_) => "write",
        };
        let elapsed = if elapsed < Duration::from_secs(1) {
            "under-1s"
        } else if elapsed < Duration::from_secs(3) {
            "1-to-3s"
        } else {
            "at-least-3s"
        };
        serde_json::json!({"schemaVersion": 1, "mechanism": "zed-clipboard-transport",
            "diagnosticsOnly": true, "operation": operation, "stage": stage, "elapsed": elapsed})
    }

    #[cfg(windows)]
    fn save_failure(input: Option<&str>, stage: &str, elapsed: Duration) {
        if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        {
            return;
        }
        let Some(directory) = std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS") else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        if !directory.is_absolute()
            || !std::fs::symlink_metadata(&directory)
                .is_ok_and(|metadata| metadata.file_type().is_dir())
        {
            return;
        }
        let mut nonce = [0; 8];
        if getrandom::fill(&mut nonce).is_err() {
            return;
        }
        let value = failure_facts(input, stage, elapsed);
        let path = directory.join(format!("clipboard-{}.json", u64::from_le_bytes(nonce)));
        if let Ok(bytes) = serde_json::to_vec(&value)
            && let Ok(mut file) = nan_harness_private_fs::open_private_new(&path)
        {
            let _ = file.write_all(&bytes).and_then(|()| file.sync_all());
        }
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
    #[cfg(any(target_os = "macos", windows))]
    {
        transport::run(Some(value)).map(|_| ())
    }
    #[cfg(target_os = "linux")]
    {
        x11::write(value)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        Err(Reason::ActionUnsupported)
    }
}

pub(super) fn read() -> Result<Zeroizing<String>, Reason> {
    #[cfg(any(target_os = "macos", windows))]
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
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        Err(Reason::ActionUnsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    #[ignore = "Requires an explicitly selected disposable GitHub-hosted Windows session"]
    fn hosted_native_clipboard_contract() {
        assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
        assert_eq!(
            std::env::var("RUNNER_ENVIRONMENT").as_deref(),
            Ok("github-hosted")
        );
        struct Clear;
        impl Drop for Clear {
            fn drop(&mut self) {
                let _ = write("");
            }
        }
        let _clear = Clear;
        write("").unwrap();
        assert!(read().unwrap().is_empty());
        let text = "NaNH synthetic clipboard: café λ 🧪\r\nsecond line";
        write(text).unwrap();
        assert_eq!(read().unwrap().as_str(), text);
        assert!(transport::run(Some("invalid\0text")).is_err());
        assert_eq!(read().unwrap().as_str(), text);
        assert!(transport::run(Some(&"x".repeat(1025))).is_err());
        assert_eq!(read().unwrap().as_str(), text);
        write("").unwrap();
        assert!(read().unwrap().is_empty());
    }

    #[test]
    fn failure_facts_disclose_only_closed_operation_and_elapsed_buckets() {
        use std::time::Duration;
        for (input, operation) in [
            (None, "read"),
            (Some(""), "clear"),
            (Some("private synthetic payload"), "write"),
        ] {
            for (seconds, bucket) in [(0, "under-1s"), (1, "1-to-3s"), (3, "at-least-3s")] {
                let facts =
                    transport::failure_facts(input, "wait-timeout", Duration::from_secs(seconds));
                assert_eq!(facts["operation"], operation);
                assert_eq!(facts["elapsed"], bucket);
                assert_eq!(facts["stage"], "wait-timeout");
                assert_eq!(facts["diagnosticsOnly"], true);
                assert_eq!(facts.as_object().unwrap().len(), 6);
                assert!(!facts.to_string().contains("private synthetic payload"));
            }
        }
    }

    #[test]
    fn clipboard_transport_rejects_invalid_and_over_budget_payloads() {
        assert!(write(&"x".repeat(1025)).is_err());
        assert!(decode(&[0xff]).is_err());
        assert!(decode(&vec![b'x'; 65537]).is_err());
        assert_eq!(decode(b"synthetic").unwrap().as_str(), "synthetic");
    }
}
