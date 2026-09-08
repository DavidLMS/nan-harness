use nix::fcntl::{FcntlArg, FdFlag, OFlag, fcntl};
use nix::pty::openpty;
use std::fs::File;
use std::io::{self, IsTerminal as _, Read as _};
use std::process::Stdio;
use tokio::io::unix::AsyncFd;
use tokio::process::Command;
use tokio::task::JoinHandle;

pub(super) struct OutputDrain(JoinHandle<()>);

impl Drop for OutputDrain {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(super) fn prepare(command: &mut Command) -> io::Result<Option<OutputDrain>> {
    if io::stdout().is_terminal() {
        return Ok(None);
    }
    prepare_redirected(command).map(Some)
}

fn prepare_redirected(command: &mut Command) -> io::Result<OutputDrain> {
    // Zed reloads the login-shell environment when stdout is not a terminal.
    // That can replace our launch-scoped NAN_API_KEY. Keep the native process
    // on a private terminal even when the launcher is run by a GUI or CI.
    let terminal = openpty(None, None)?;
    for descriptor in [&terminal.master, &terminal.slave] {
        fcntl(descriptor, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))?;
    }
    fcntl(&terminal.master, FcntlArg::F_SETFL(OFlag::O_NONBLOCK))?;
    let reader = AsyncFd::new(File::from(terminal.master))?;
    command
        .stdout(Stdio::from(terminal.slave))
        .stderr(Stdio::null());
    Ok(OutputDrain(tokio::spawn(async move {
        // Drain without retaining native logs, which can contain user payloads.
        let mut buffer = [0u8; 8192];
        while let Ok(mut ready) = reader.readable().await {
            if let Ok(Ok(0) | Err(_)) = ready.try_io(|reader| {
                let mut file = reader.get_ref();
                file.read(&mut buffer)
            }) {
                break;
            }
        }
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn redirected_zed_keeps_its_token_and_drains_large_native_output() {
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "test -t 1 || exit 2; test \"$NAN_API_KEY\" = synthetic-session || exit 3; dd if=/dev/zero bs=65536 count=8 2>/dev/null",
            ])
            .env("NAN_API_KEY", "synthetic-session")
            .stdin(Stdio::null());
        let output = prepare_redirected(&mut command).expect("private terminal");
        let mut child = command.spawn().expect("synthetic Zed");
        drop(command);
        let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
            .await
            .expect("native output must not block")
            .expect("child exit");
        assert!(status.success());
        drop(output);
    }
}
