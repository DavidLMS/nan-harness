use super::*;
use std::collections::VecDeque;
use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc;

/// Zero makes the poll branch ready immediately; the long interval keeps it
/// pending so exactly one other branch of the select is ready per test.
const IMMEDIATE: Duration = Duration::ZERO;
const NEVER: Duration = Duration::from_hours(1);

struct FakeLifecycle {
    identities: Mutex<VecDeque<Result<bool, HermesDesktopError>>>,
    discoveries: Mutex<VecDeque<Result<Option<DesktopProcess>, HermesDesktopError>>>,
    checked: Mutex<Vec<DesktopProcess>>,
    terminations: AtomicUsize,
    termination_fails: bool,
}

impl FakeLifecycle {
    fn new() -> Self {
        Self {
            identities: Mutex::new(VecDeque::new()),
            discoveries: Mutex::new(VecDeque::new()),
            checked: Mutex::new(Vec::new()),
            terminations: AtomicUsize::new(0),
            termination_fails: false,
        }
    }

    fn identities(
        self,
        identities: impl IntoIterator<Item = Result<bool, HermesDesktopError>>,
    ) -> Self {
        *self.identities.lock().expect("identity script") = identities.into_iter().collect();
        self
    }

    fn discoveries(
        self,
        discoveries: impl IntoIterator<Item = Result<Option<DesktopProcess>, HermesDesktopError>>,
    ) -> Self {
        *self.discoveries.lock().expect("discovery script") = discoveries.into_iter().collect();
        self
    }

    fn failing_termination(mut self) -> Self {
        self.termination_fails = true;
        self
    }

    fn checked(&self) -> Vec<DesktopProcess> {
        self.checked.lock().expect("checked identities").clone()
    }

    fn terminations(&self) -> usize {
        self.terminations.load(Ordering::Relaxed)
    }
}

impl DesktopLifecycle for FakeLifecycle {
    fn running(&self) -> Result<Option<DesktopProcess>, HermesDesktopError> {
        self.discoveries
            .lock()
            .expect("discovery script")
            .pop_front()
            .unwrap_or(Ok(None))
    }

    fn is_same(&self, process: &DesktopProcess) -> Result<bool, HermesDesktopError> {
        self.checked
            .lock()
            .expect("checked identities")
            .push(process.clone());
        self.identities
            .lock()
            .expect("identity script")
            .pop_front()
            .unwrap_or(Ok(true))
    }

    async fn terminate(&self) -> Result<(), HermesDesktopError> {
        self.terminations.fetch_add(1, Ordering::Relaxed);
        if self.termination_fails {
            return Err(HermesDesktopError::DidNotTerminate);
        }
        Ok(())
    }
}

enum FakeGateway {
    Pending,
    Exited,
    Failed,
}

impl SupervisedGateway for FakeGateway {
    async fn wait(&mut self) -> Result<(), HermesDesktopError> {
        match self {
            Self::Pending => std::future::pending().await,
            Self::Exited => Ok(()),
            Self::Failed => Err(HermesDesktopError::BindGateway(std::io::Error::other(
                "gateway stopped",
            ))),
        }
    }
}

fn desktop(pid: u32) -> DesktopProcess {
    DesktopProcess {
        pid,
        started: format!("Mon Jan  1 00:00:0{pid} 2035"),
    }
}

async fn bounded<T>(operation: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(2), operation)
        .await
        .expect("supervision policy should complete within the test deadline")
}

mod relaunch;
mod running;
