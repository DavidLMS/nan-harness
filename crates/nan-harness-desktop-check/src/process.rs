use std::{io, process::ExitStatus};
use tokio::process::{Child, Command};

#[cfg(windows)]
use process_wrap::tokio::{ChildWrapper, CommandWrap, JobObject, KillOnDrop};

pub(crate) trait Observation {
    fn id(&self) -> Option<u32>;
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>>;
}

pub(crate) struct ProbeProcess {
    #[cfg(not(windows))]
    inner: Child,
    #[cfg(windows)]
    inner: Box<dyn ChildWrapper>,
}

impl ProbeProcess {
    pub(crate) fn spawn(mut command: Command) -> io::Result<Self> {
        #[cfg(not(windows))]
        {
            Ok(Self {
                inner: command.kill_on_drop(true).spawn()?,
            })
        }
        #[cfg(windows)]
        {
            // JobObject creates the process suspended, assigns it before resume, and waits on
            // the whole job, so launcher descendants cannot escape between spawn and assignment.
            let inner = CommandWrap::from(command)
                .wrap(KillOnDrop)
                .wrap(JobObject)
                .spawn()?;
            Ok(Self { inner })
        }
    }

    pub(crate) fn id(&self) -> Option<u32> {
        self.inner.id()
    }
    #[cfg(all(not(windows), test))]
    pub(crate) fn take_stdout(&mut self) -> Option<tokio::process::ChildStdout> {
        self.inner.stdout.take()
    }
    pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.inner.try_wait()
    }
    pub(crate) fn start_kill(&mut self) -> io::Result<()> {
        #[cfg(not(windows))]
        {
            self.inner.start_kill()
        }
        #[cfg(windows)]
        {
            self.inner.start_kill()
        }
    }
    pub(crate) async fn wait(&mut self) -> io::Result<ExitStatus> {
        #[cfg(not(windows))]
        {
            self.inner.wait().await
        }
        #[cfg(windows)]
        {
            loop {
                if let Some(status) = self.inner.try_wait()? {
                    return Ok(status);
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }
    }
    pub(crate) async fn kill(&mut self) -> io::Result<()> {
        self.start_kill()?;
        self.wait().await.map(|_| ())
    }
}

impl Observation for ProbeProcess {
    fn id(&self) -> Option<u32> {
        self.id()
    }
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.try_wait()
    }
}

impl Observation for Child {
    fn id(&self) -> Option<u32> {
        self.id()
    }
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.try_wait()
    }
}
