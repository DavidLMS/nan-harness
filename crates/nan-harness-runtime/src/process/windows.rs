use process_wrap::tokio::{ChildWrapper, CommandWrap, CommandWrapper, JobObject, KillOnDrop};
use std::io;
use std::os::windows::io::AsRawHandle;
use std::process::ExitStatus;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::{ChildStdin, ChildStdout, Command};
use win32job::{ExtendedLimitInfo, Job};

const POLL_INTERVAL: Duration = Duration::from_millis(5);
const REAP_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) struct WindowsChild {
    child: Box<dyn ChildWrapper>,
    custody: Arc<Job>,
}

impl WindowsChild {
    pub(super) fn spawn(command: Command) -> io::Result<Self> {
        let mut limits = ExtendedLimitInfo::new();
        limits.limit_kill_on_job_close();
        let custody = Arc::new(Job::create_with_limit_info(&limits).map_err(io::Error::from)?);
        // JobObject suspends the root before any wrap_child hook. Assign the outer
        // custody Job first, then let JobObject assign its nested Job and resume.
        let child = CommandWrap::from(command)
            .wrap(KillOnDrop)
            .wrap(Custody(Arc::clone(&custody)))
            .wrap(JobObject)
            .spawn()?;
        Ok(Self { child, custody })
    }

    pub(super) fn stdout(&mut self) -> &mut Option<ChildStdout> {
        self.child.stdout()
    }

    pub(super) fn stdin(&mut self) -> &mut Option<ChildStdin> {
        self.child.stdin()
    }

    pub(super) fn id(&self) -> Option<u32> {
        self.child.id()
    }

    pub(super) fn start_kill(&mut self) -> io::Result<()> {
        self.child.start_kill()
    }

    pub(super) async fn wait(&mut self) -> io::Result<ExitStatus> {
        // JobObject is the only child wrapper: KillOnDrop and Custody are
        // transparent. Poll Tokio directly, leaving the completion port untouched.
        // process-wrap 10.0.0 can block forever when wait follows try_wait.
        loop {
            if let Some(status) = self.child.inner_mut().try_wait()? {
                return Ok(status);
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    pub(super) async fn wait_for_descendants(&self) -> io::Result<()> {
        loop {
            // Query the retained Job handle, never a rediscovered PID. This also
            // includes nested Jobs; query failure cannot establish emptiness.
            if self
                .custody
                .query_process_id_list()
                .map_err(io::Error::other)?
                .is_empty()
            {
                return Ok(());
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    pub(super) async fn kill(&mut self) -> io::Result<()> {
        self.start_kill()?;
        tokio::time::timeout(REAP_TIMEOUT, async {
            self.wait().await?;
            self.wait_for_descendants().await
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "owned Windows Job did not drain"))?
    }
}

#[derive(Debug)]
struct Custody(Arc<Job>);

impl CommandWrapper for Custody {
    fn wrap_child(
        &mut self,
        mut child: Box<dyn ChildWrapper>,
        _core: &CommandWrap,
    ) -> io::Result<Box<dyn ChildWrapper>> {
        let assigned = child
            .process_handle()
            .ok_or_else(|| io::Error::other("owned Windows process handle is unavailable"))
            .and_then(|handle| {
                self.0
                    .assign_process(handle.as_raw_handle() as isize)
                    .map_err(io::Error::from)
            });
        if let Err(error) = assigned {
            let _ = child.start_kill();
            return Err(error);
        }
        Ok(child)
    }
}
