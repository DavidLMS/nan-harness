use std::io;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;
#[cfg(windows)]
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::process::Command;
#[cfg(not(windows))]
use tokio::process::{ChildStderr, ChildStdout};

#[cfg(not(windows))]
type Pipes = (ChildStdout, ChildStderr);
#[cfg(windows)]
type Pipes = (NamedPipeServer, NamedPipeServer);

#[cfg(windows)]
#[path = "windows_pipes.rs"]
mod windows_pipes;

pub(super) struct ProbeChild {
    #[cfg(unix)]
    process_group: Option<nix::unistd::Pid>,
    #[cfg(not(windows))]
    inner: tokio::process::Child,
    #[cfg(windows)]
    inner: Box<dyn process_wrap::tokio::ChildWrapper>,
    #[cfg(windows)]
    pipes: Option<Pipes>,
}

impl ProbeChild {
    pub(super) fn spawn(executable: &Path, arguments: &[&str]) -> io::Result<Self> {
        let mut command = Command::new(executable);
        command.args(arguments).stdin(Stdio::null());
        #[cfg(not(windows))]
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        let pipes = windows_pipes::prepare(&mut command)?;
        #[cfg(unix)]
        command.process_group(0);
        #[cfg(not(windows))]
        let inner = command.kill_on_drop(true).spawn()?;
        #[cfg(unix)]
        let process_group = inner
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .map(nix::unistd::Pid::from_raw);
        #[cfg(windows)]
        let inner = {
            use process_wrap::tokio::{CommandWrap, JobObject, KillOnDrop};
            // The wrapper creates the process suspended, assigns the job, then resumes it, so
            // descendants cannot escape ownership between spawn and job assignment.
            CommandWrap::from(command)
                .wrap(KillOnDrop)
                .wrap(JobObject)
                .spawn()?
        };
        Ok(Self {
            #[cfg(unix)]
            process_group,
            inner,
            #[cfg(windows)]
            pipes: Some(pipes),
        })
    }

    pub(super) fn take_pipes(&mut self) -> io::Result<Pipes> {
        #[cfg(not(windows))]
        let pipes = (self.inner.stdout.take(), self.inner.stderr.take());
        #[cfg(windows)]
        let pipes = {
            let (stdout, stderr) = self
                .pipes
                .take()
                .ok_or_else(|| io::Error::other("probe pipes were not created"))?;
            (Some(stdout), Some(stderr))
        };
        match pipes {
            (Some(stdout), Some(stderr)) => Ok((stdout, stderr)),
            _ => Err(io::Error::other("probe pipes were not created")),
        }
    }

    pub(super) fn terminate(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        let group_result = if let Some(id) = self.inner.id() {
            use nix::sys::signal::{Signal, killpg};
            use nix::unistd::Pid;
            let id = i32::try_from(id).map_err(io::Error::other)?;
            match killpg(Pid::from_raw(id), Signal::SIGKILL) {
                Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
                Err(error) => Err(io::Error::new(
                    io::Error::from(error).kind(),
                    format!("could not signal probe process group: {error}"),
                )),
            }
        } else {
            Ok(())
        };
        let killed = self.inner.start_kill();
        #[cfg(windows)]
        if killed.is_err() {
            // Still try to reap the direct child if job termination fails; the job handle also
            // retains its kill-on-close fallback. Preserve the original job error for the caller.
            let _ = self.inner.inner_mut().start_kill();
        }
        #[cfg(unix)]
        group_result?;
        killed.map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("could not kill direct probe child: {error}"),
            )
        })
    }

    pub(super) async fn reap(&mut self) -> io::Result<ExitStatus> {
        // In particular, do not use the Windows job wrapper's wait(): it may create an
        // uncancellable blocking waiter. try_wait polls the job and reaps the direct child.
        loop {
            if let Some(status) = self.inner.try_wait()? {
                return Ok(status);
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    #[cfg(unix)]
    pub(super) fn confirm_termination(&self, result: io::Result<()>) -> io::Result<()> {
        if result
            .as_ref()
            .is_err_and(|error| error.kind() == io::ErrorKind::PermissionDenied)
            && self.process_group.is_some_and(|group| {
                nix::sys::signal::killpg(group, None) == Err(nix::errno::Errno::ESRCH)
            })
        {
            // macOS can return EPERM for a group containing only the unreaped, exited child.
            // After reaping, accept that race only if the group is demonstrably gone. Never
            // send another killing signal using the saved group id once its leader was reaped.
            return Ok(());
        }
        result
    }
}
