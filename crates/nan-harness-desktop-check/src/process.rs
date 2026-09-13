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
    inner: Option<Box<dyn ChildWrapper>>,
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
            Ok(Self { inner: Some(inner) })
        }
    }

    #[cfg(not(windows))]
    pub(crate) fn id(&self) -> Option<u32> {
        self.inner.id()
    }
    #[cfg(windows)]
    pub(crate) fn id(&self) -> Option<u32> {
        self.inner.as_ref().and_then(|inner| inner.id())
    }
    #[cfg(all(not(windows), test))]
    pub(crate) fn take_stdout(&mut self) -> Option<tokio::process::ChildStdout> {
        self.inner.stdout.take()
    }
    #[cfg(all(windows, test))]
    pub(crate) fn take_stdout(&mut self) -> Option<tokio::process::ChildStdout> {
        self.inner.stdout().take()
    }
    #[cfg(not(windows))]
    pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.inner.try_wait()
    }
    #[cfg(windows)]
    pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.inner.as_mut().map_or(Ok(None), ChildWrapper::try_wait)
    }
    pub(crate) fn start_kill(&mut self) -> io::Result<()> {
        #[cfg(not(windows))]
        {
            self.inner.start_kill()
        }
        #[cfg(windows)]
        {
            self.inner.as_mut().map_or(Ok(()), ChildWrapper::start_kill)
        }
    }
    pub(crate) async fn wait_launcher(&mut self) -> io::Result<ExitStatus> {
        #[cfg(not(windows))]
        {
            self.inner.wait().await
        }
        #[cfg(windows)]
        {
            process_wrap::tokio::ChildWrapper::wait(
                self.inner
                    .as_mut()
                    .ok_or_else(|| io::Error::other("owned process already closed"))?
                    .inner_mut(),
            )
            .await
        }
    }
    #[cfg(windows)]
    pub(crate) fn close_job(&mut self) {
        // Closing the JobObject handle has kill-on-close semantics and is the final
        // ownership boundary after the launcher has been reaped.
        self.inner.take();
    }
    pub(crate) async fn kill(&mut self) -> io::Result<()> {
        self.start_kill()?;
        self.wait_launcher().await.map(|_| ())
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
