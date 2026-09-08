mod child;
#[cfg(test)]
mod tests;

use std::io;
use std::path::Path;
use std::process::Output;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::io::AsyncReadExt;

const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const OUTPUT_LIMIT: usize = 1024 * 1024;
// Pipe futures are cancelled immediately; allow one additional second to reap after SIGKILL/job
// termination. An OS that cannot reap in this interval is reported as a cleanup failure.
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Error)]
pub(super) enum ProbeError {
    #[error("probe exceeded its 30-second deadline; check the executable installation")]
    Timeout,
    #[error("probe exceeded 1 MiB of combined output; check the executable installation")]
    OutputLimit,
    #[error("could not execute or clean up probe: {0}")]
    Io(#[from] io::Error),
}

pub(super) fn run_command(executable: &Path, arguments: &[&str]) -> Result<Output, ProbeError> {
    run_bounded(executable, arguments, PROBE_TIMEOUT, OUTPUT_LIMIT)
}

fn run_bounded(
    executable: &Path,
    arguments: &[&str],
    timeout: Duration,
    output_limit: usize,
) -> Result<Output, ProbeError> {
    let deadline = Instant::now() + timeout;
    // Discovery has a synchronous API, including callers already inside a Tokio runtime. A scoped
    // worker owns this small runtime; there are no blocking pipe readers to outlive the deadline.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("harness-probe".to_owned())
            .spawn_scoped(scope, || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?
                    .block_on(run(executable, arguments, deadline, output_limit))
            })?
            .join()
            .map_err(|_| io::Error::other("probe worker panicked"))?
    })
}

async fn run(
    executable: &Path,
    arguments: &[&str],
    deadline: Instant,
    output_limit: usize,
) -> Result<Output, ProbeError> {
    let mut child =
        spawn_with_retry(deadline, || child::ProbeChild::spawn(executable, arguments)).await?;
    let result =
        tokio::time::timeout_at(deadline.into(), collect(&mut child, output_limit, deadline))
            .await
            .unwrap_or(Err(ProbeError::Timeout));
    if result.is_err() {
        let terminated = child.terminate();
        let reaped = tokio::time::timeout(CLEANUP_TIMEOUT, child.reap()).await;
        reaped.map_err(|_| {
            io::Error::new(io::ErrorKind::TimedOut, "probe cleanup did not finish")
        })??;
        #[cfg(unix)]
        let terminated = child.confirm_termination(terminated);
        terminated.map_err(|error| {
            io::Error::new(error.kind(), format!("could not terminate probe: {error}"))
        })?;
    }
    result
}

async fn spawn_with_retry<T>(
    deadline: Instant,
    mut spawn: impl FnMut() -> io::Result<T>,
) -> Result<T, ProbeError> {
    for attempt in 1..=3 {
        if Instant::now() >= deadline {
            return Err(ProbeError::Timeout);
        }
        match spawn() {
            Err(error) if temporarily_busy(&error) && attempt < 3 => {
                tokio::time::sleep_until(
                    (Instant::now() + Duration::from_millis(10))
                        .min(deadline)
                        .into(),
                )
                .await;
            }
            result => return result.map_err(ProbeError::Io),
        }
    }
    unreachable!("all spawn attempts return")
}

fn temporarily_busy(error: &io::Error) -> bool {
    #[cfg(unix)]
    {
        error.raw_os_error() == Some(nix::libc::ETXTBSY)
    }
    #[cfg(not(unix))]
    {
        let _ = error;
        false
    }
}

async fn collect(
    child: &mut child::ProbeChild,
    limit: usize,
    deadline: Instant,
) -> Result<Output, ProbeError> {
    let (mut stdout, mut stderr) = child.take_pipes()?;
    let mut output = [Vec::new(), Vec::new()];
    let mut ended = [false; 2];
    let mut buffers = Box::new([[0_u8; 8192]; 2]);
    while !ended.iter().all(|done| *done) {
        // Tokio timeouts poll their inner future first. Check the shared wall deadline even when
        // both pipes stay immediately readable or process creation exhausted the budget.
        if Instant::now() >= deadline {
            return Err(ProbeError::Timeout);
        }
        let [out_buffer, err_buffer] = buffers.as_mut();
        let (stream, count) = tokio::select! {
            count = stdout.read(out_buffer), if !ended[0] => (0, count?),
            count = stderr.read(err_buffer), if !ended[1] => (1, count?),
        };
        if count == 0 {
            ended[stream] = true;
        } else {
            if output[0].len() + output[1].len() + count > limit {
                return Err(ProbeError::OutputLimit);
            }
            output[stream].extend_from_slice(&buffers[stream][..count]);
        }
    }
    let status = child.reap().await?;
    if Instant::now() >= deadline {
        return Err(ProbeError::Timeout);
    }
    let [stdout, stderr] = output;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}
