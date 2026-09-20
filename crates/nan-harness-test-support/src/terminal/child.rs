use std::io;
use std::process::ExitStatus;
use tokio::process::{ChildStderr, ChildStdout, Command};

/// Own the complete Windows process tree, including helpers that inherit output handles.
pub(super) struct TerminalChild {
    #[cfg(not(windows))]
    inner: tokio::process::Child,
    #[cfg(windows)]
    inner: Box<dyn process_wrap::tokio::ChildWrapper>,
}

impl TerminalChild {
    pub(super) fn spawn(command: Command) -> io::Result<Self> {
        #[cfg(not(windows))]
        let inner = {
            let mut command = command;
            command.spawn()?
        };
        #[cfg(windows)]
        let inner = {
            use process_wrap::tokio::{CommandWrap, JobObject, KillOnDrop};
            CommandWrap::from(command)
                .wrap(KillOnDrop)
                .wrap(JobObject)
                .spawn()?
        };
        Ok(Self { inner })
    }

    pub(super) fn id(&self) -> Option<u32> {
        self.inner.id()
    }

    pub(super) fn take_stdout(&mut self) -> Option<ChildStdout> {
        #[cfg(not(windows))]
        {
            self.inner.stdout.take()
        }
        #[cfg(windows)]
        {
            self.inner.stdout().take()
        }
    }

    pub(super) fn take_stderr(&mut self) -> Option<ChildStderr> {
        #[cfg(not(windows))]
        {
            self.inner.stderr.take()
        }
        #[cfg(windows)]
        {
            self.inner.stderr().take()
        }
    }

    pub(super) fn start_kill(&mut self) -> io::Result<()> {
        self.inner.start_kill()
    }

    pub(super) async fn wait(&mut self) -> io::Result<ExitStatus> {
        #[cfg(not(windows))]
        {
            self.inner.wait().await
        }
        #[cfg(windows)]
        {
            // JobObject::wait can leave an uncancellable blocking waiter behind.
            loop {
                if let Some(status) = self.inner.try_wait()? {
                    return Ok(status);
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }
    }

    #[cfg(windows)]
    pub(super) fn close_descendants(&mut self) -> io::Result<()> {
        self.inner.start_kill()
    }
}
