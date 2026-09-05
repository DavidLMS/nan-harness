use super::connection::handle_connection;
use super::state::{
    acquire_process_lock, random_hex, remove_own_receipt, state_error, write_receipt,
};
use crate::CaptureSink;
use crate::CoordinatorError;
use crate::paths::private_directory;
use crate::protocol::{PROTOCOL_VERSION, Receipt};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;

const IDLE_TIMEOUT: Duration = Duration::from_mins(15);

/// Runs the per-user coordinator until it has been idle for fifteen minutes.
///
/// # Errors
///
/// Returns an error when private state cannot be created, another coordinator
/// owns the process lock, or the loopback listener cannot be served.
pub async fn run_daemon() -> Result<(), CoordinatorError> {
    let directory = crate::config_directory()?.join("coordinator/v1");
    private_directory(&directory)?;
    let lock = acquire_process_lock(&directory)?;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|source| state_error(&directory, source))?;
    let token = random_hex()?;
    let generation = random_hex()?;
    let receipt = Receipt {
        protocol_version: PROTOCOL_VERSION,
        port: listener
            .local_addr()
            .map_err(|source| state_error(&directory, source))?
            .port(),
        token: token.clone(),
        generation: generation.clone(),
        pid: std::process::id(),
    };
    write_receipt(&directory, &receipt)?;
    let result = serve(listener, token, directory.join("capacity.json")).await;
    remove_own_receipt(&directory, &generation);
    drop(lock);
    result
}

pub(super) async fn serve(
    listener: TcpListener,
    token: String,
    cache_path: PathBuf,
) -> Result<(), CoordinatorError> {
    let scheduler = crate::scheduler::Scheduler::start(cache_path);
    let capture = CaptureSink::new(format!("coordinator_{}", std::process::id()));
    let started = Instant::now();
    let last_activity = Arc::new(AtomicU64::new(0));
    let connections = Arc::new(AtomicUsize::new(0));
    let mut idle_check = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted.map_err(|source| CoordinatorError::State {
                    path: PathBuf::from("coordinator listener"),
                    source,
                })?;
                record_activity(&last_activity, started);
                connections.fetch_add(1, Ordering::Relaxed);
                tokio::spawn(handle_connection(
                    stream,
                    token.clone(),
                    scheduler.clone(),
                    Arc::clone(&connections),
                    Arc::clone(&last_activity),
                    capture.clone(),
                    started,
                ));
            }
            _ = idle_check.tick() => {
                if connections.load(Ordering::Relaxed) == 0
                    && elapsed_since_activity(&last_activity, started) >= IDLE_TIMEOUT
                {
                    return Ok(());
                }
            }
        }
    }
}

pub(super) fn record_activity(activity: &AtomicU64, started: Instant) {
    activity.store(millis(started.elapsed()), Ordering::Relaxed);
}

fn elapsed_since_activity(activity: &AtomicU64, started: Instant) -> Duration {
    started
        .elapsed()
        .saturating_sub(Duration::from_millis(activity.load(Ordering::Relaxed)))
}

pub(super) fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
