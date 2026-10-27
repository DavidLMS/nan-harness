//! Retained kernel ownership for Windows terminal observations.

use process_wrap::tokio::{ChildWrapper, CommandWrap, CommandWrapper, JobObject, KillOnDrop};
use std::io;
use std::os::windows::io::AsRawHandle;
use std::process::ExitStatus;
use std::sync::Arc;
use tokio::process::{ChildStderr, ChildStdout, Command};
use win32job::{ExtendedLimitInfo, Job};

pub(super) struct OwnedChild {
    child: Box<dyn ChildWrapper>,
    custody: Arc<Job>,
}

impl OwnedChild {
    pub(super) fn spawn(command: Command) -> io::Result<Self> {
        let mut limits = ExtendedLimitInfo::new();
        limits.limit_kill_on_job_close();
        let custody = Arc::new(Job::create_with_limit_info(&limits).map_err(io::Error::from)?);
        // JobObject suspends before the child hooks. Assign outer custody first;
        // its nested JobObject is assigned before that wrapper resumes the root.
        let child = CommandWrap::from(command)
            .wrap(KillOnDrop)
            .wrap(Custody(Arc::clone(&custody)))
            .wrap(JobObject)
            .spawn()?;
        Ok(Self { child, custody })
    }

    pub(super) fn id(&self) -> Option<u32> {
        self.child.id()
    }

    pub(super) fn stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout().take()
    }

    pub(super) fn stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr().take()
    }

    pub(super) fn start_kill(&mut self) -> io::Result<()> {
        self.child.start_kill()
    }

    pub(super) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    pub(super) fn active_process_count(&self) -> io::Result<u32> {
        // Count every active member, including nested Jobs and a root whose
        // cleanup is unconfirmed. Never exclude a rediscovered numeric root PID.
        let members = self
            .custody
            .query_process_id_list()
            .map_err(io::Error::other)?;
        u32::try_from(members.len()).map_err(io::Error::other)
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
