//! Publication readiness for tiny, side-effect-free synthetic scripts only.

use std::{
    io,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

/// Execute a pure version/exit-code fixture before using it in assertions.
///
/// Linux rejects execution while any writer still holds the new file open.
/// Retry only that publication error, never an assertion or a started process.
/// The fixture's exit code is deliberately left to the caller's actual test.
/// Do not use this with real programs or scripts with side effects.
///
/// # Errors
/// Returns non-publication spawn errors immediately, persistent busy-file errors,
/// wait errors, or a timeout if the fixture does not exit within the budget.
pub fn wait_until_ready(executable: &Path) -> io::Result<()> {
    ready_until(executable, Instant::now() + Duration::from_secs(2))
}

fn ready_until(executable: &Path, deadline: Instant) -> io::Result<()> {
    let mut child = loop {
        match Command::new(executable)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => break child,
            Err(error)
                if error.kind() == io::ErrorKind::ExecutableFileBusy
                    && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(error),
        }
    };
    let outcome = loop {
        match child.try_wait() {
            Ok(Some(_)) => break Ok(()),
            Ok(None) => {}
            Err(error) => break Err(error),
        }
        if Instant::now() >= deadline {
            break Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "synthetic fixture did not exit",
            ));
        }
        thread::sleep(Duration::from_millis(5));
    };
    if outcome.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt as _};

    #[test]
    fn fixture_readiness_does_not_require_a_success_status_or_accept_missing_files() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("fixture");
        fs::write(&executable, "#!/bin/sh\nexit 7\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        wait_until_ready(&executable).unwrap();
        assert_eq!(
            wait_until_ready(&root.path().join("missing"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            wait_until_ready(&executable).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_retained_writer_blocks_publication_until_it_closes() {
        use std::{io::Write as _, sync::mpsc};

        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("fixture");
        let mut writer = fs::File::create(&executable).unwrap();
        writer.write_all(b"#!/bin/sh\nexit 7\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            ready_until(&executable, Instant::now()).unwrap_err().kind(),
            io::ErrorKind::ExecutableFileBusy
        );
        let (sent, received) = mpsc::channel();
        let waiting = thread::spawn(move || sent.send(wait_until_ready(&executable)).unwrap());
        assert!(matches!(
            received.recv_timeout(Duration::from_millis(30)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        drop(writer);
        received
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
        waiting.join().unwrap();
    }
}
