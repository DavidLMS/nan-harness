use crate::CoordinatorError;
use crate::paths::private_directory;
use crate::protocol::{
    AttemptOutcome, AttemptPhase, ClientMessage, PROTOCOL_VERSION, Receipt, ServerMessage,
    read_frame, write_frame,
};
use crate::scheduler::{AcquireRequest, Scheduler};
use crate::{CaptureLeg, CaptureSink};
use nan_harness_private_fs::{open_private_new, open_private_read, open_private_truncate};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq as _;
use tokio::io::AsyncReadExt as _;
use tokio::net::{TcpListener, TcpStream};

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

async fn serve(
    listener: TcpListener,
    token: String,
    cache_path: PathBuf,
) -> Result<(), CoordinatorError> {
    let scheduler = Scheduler::start(cache_path);
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

async fn handle_connection(
    stream: TcpStream,
    token: String,
    scheduler: Scheduler,
    connections: Arc<AtomicUsize>,
    last_activity: Arc<AtomicU64>,
    capture: CaptureSink,
    started: Instant,
) {
    let _guard = ConnectionGuard(connections);
    let (mut reader, mut writer) = stream.into_split();
    let Ok(message) = read_frame::<ClientMessage>(&mut reader).await else {
        return;
    };
    let ClientMessage::Acquire {
        protocol_version,
        token: supplied_token,
        scope,
        launch_id,
        endpoint,
        model,
        lane,
        priority,
    } = message
    else {
        return;
    };
    if protocol_version != PROTOCOL_VERSION || !tokens_match(&token, &supplied_token) {
        let _ = write_frame(
            &mut writer,
            &ServerMessage::Rejected {
                reason: "incompatible or unauthorized coordinator client".to_owned(),
            },
        )
        .await;
        return;
    }
    let event_launch_id = launch_id.clone();
    let acquire = scheduler.acquire(AcquireRequest {
        scope: scope.clone(),
        launch_id,
        lane,
        priority,
        enqueued_at: Instant::now(),
    });
    tokio::pin!(acquire);
    let grant = tokio::select! {
        grant = &mut acquire => grant,
        _disconnected = reader.read_u8() => {
            return;
        }
    };
    let Some(grant) = grant else {
        return;
    };
    if write_frame(
        &mut writer,
        &ServerMessage::Granted {
            lease_id: grant.lease_id,
            queued_ms: millis(grant.queued),
        },
    )
    .await
    .is_err()
    {
        scheduler.release(
            scope,
            lane == crate::RequestLane::Inference && priority == crate::RequestPriority::Foreground,
        );
        return;
    }
    record_activity(&last_activity, started);
    let capture = capture.begin_request(format!("lease_{}", grant.lease_id));
    if let Some(capture) = &capture {
        let event = serde_json::json!({
            "event": "permit_granted",
            "launch_id": event_launch_id,
            "queued_ms": millis(grant.queued),
            "endpoint": endpoint,
            "model": model,
            "lane": lane,
            "priority": priority,
        });
        if let Ok(payload) = serde_json::to_vec(&event) {
            capture.record(CaptureLeg::Coordinator, &payload);
        }
    }
    let context = LeaseContext {
        scope,
        lease_id: grant.lease_id,
        growth_eligible: grant.growth_eligible,
        foreground_inference: lane == crate::RequestLane::Inference
            && priority == crate::RequestPriority::Foreground,
        capture,
    };
    observe_until_release(&mut reader, &mut writer, &scheduler, context).await;
    record_activity(&last_activity, started);
}

struct LeaseContext {
    scope: String,
    lease_id: u64,
    growth_eligible: bool,
    foreground_inference: bool,
    capture: Option<crate::CaptureRequest>,
}

async fn observe_until_release(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    scheduler: &Scheduler,
    context: LeaseContext,
) {
    let LeaseContext {
        scope,
        lease_id,
        growth_eligible,
        foreground_inference,
        capture,
    } = context;
    let mut headers_ms = None;
    let observed = loop {
        match read_frame::<ClientMessage>(reader).await {
            Ok(ClientMessage::Progress {
                lease_id: observed_lease,
                phase: AttemptPhase::HeadersReceived,
                elapsed_ms,
            }) if observed_lease == lease_id => {
                headers_ms = Some(elapsed_ms);
            }
            Ok(ClientMessage::Observe {
                lease_id: observed_lease,
                outcome,
                retry_after_ms,
            }) if observed_lease == lease_id => break Some((outcome, retry_after_ms)),
            Ok(_) | Err(_) => break None,
        }
    };
    let Some((outcome, retry_after_ms)) = observed else {
        if let Some(capture) = &capture {
            let event = serde_json::json!({
                "event": "attempt_abandoned",
                "outcome": AttemptOutcome::Cancelled,
            });
            if let Ok(payload) = serde_json::to_vec(&event) {
                capture.record(CaptureLeg::Coordinator, &payload);
            }
        }
        let _ = scheduler
            .observe(
                scope.clone(),
                AttemptOutcome::Cancelled,
                None,
                false,
                foreground_inference,
                None,
            )
            .await;
        scheduler.release(scope, foreground_inference);
        return;
    };
    let retry_after = retry_after_ms.map(Duration::from_millis);
    if let Some(capture) = &capture {
        let event = serde_json::json!({
            "event": "attempt_observed",
            "outcome": outcome,
            "retry_after_ms": retry_after_ms,
            "headers_ms": headers_ms,
        });
        if let Ok(payload) = serde_json::to_vec(&event) {
            capture.record(CaptureLeg::Coordinator, &payload);
        }
    }
    let delay = scheduler
        .observe(
            scope.clone(),
            outcome,
            retry_after,
            growth_eligible,
            foreground_inference,
            headers_ms.map(Duration::from_millis),
        )
        .await
        .unwrap_or_default();
    if is_retryable(outcome) {
        scheduler.release(scope, foreground_inference);
        let _ = write_frame(
            writer,
            &ServerMessage::Retry {
                delay_ms: millis(delay),
            },
        )
        .await;
        return;
    }
    let _ = write_frame(writer, &ServerMessage::Complete).await;
    scheduler.release(scope, foreground_inference);
}

const fn is_retryable(outcome: AttemptOutcome) -> bool {
    matches!(
        outcome,
        AttemptOutcome::Transport
            | AttemptOutcome::Timeout
            | AttemptOutcome::RateLimited
            | AttemptOutcome::ServerError
            | AttemptOutcome::InvalidResponse
    )
}

fn acquire_process_lock(directory: &Path) -> Result<File, CoordinatorError> {
    let path = directory.join("process.lock");
    let file = open_private_truncate(&path).map_err(|source| state_error(&path, source))?;
    file.try_lock()
        .map_err(|_| CoordinatorError::Protocol("another coordinator is already running"))?;
    Ok(file)
}

fn write_receipt(directory: &Path, receipt: &Receipt) -> Result<(), CoordinatorError> {
    let temporary = directory.join(format!("receipt-{}.tmp", receipt.generation));
    let target = directory.join("receipt.json");
    let payload = serde_json::to_vec(receipt)?;
    let mut file =
        open_private_new(&temporary).map_err(|source| state_error(&temporary, source))?;
    file.write_all(&payload)
        .and_then(|()| file.sync_all())
        .map_err(|source| state_error(&temporary, source))?;
    if target.exists() {
        fs::remove_file(&target).map_err(|source| state_error(&target, source))?;
    }
    fs::rename(&temporary, &target).map_err(|source| state_error(&target, source))
}

fn remove_own_receipt(directory: &Path, generation: &str) {
    let path = directory.join("receipt.json");
    let matches = open_private_read(&path)
        .ok()
        .and_then(|(file, _)| serde_json::from_reader::<_, Receipt>(file).ok())
        .is_some_and(|receipt| receipt.generation == generation);
    if matches {
        let _ = fs::remove_file(path);
    }
}

fn random_hex() -> Result<String, CoordinatorError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)?;
    let mut value = String::with_capacity(64);
    for byte in bytes {
        let _ = write!(&mut value, "{byte:02x}");
    }
    Ok(value)
}

fn tokens_match(expected: &str, supplied: &str) -> bool {
    expected.len() == supplied.len() && bool::from(expected.as_bytes().ct_eq(supplied.as_bytes()))
}

fn record_activity(activity: &AtomicU64, started: Instant) {
    activity.store(millis(started.elapsed()), Ordering::Relaxed);
}

fn elapsed_since_activity(activity: &AtomicU64, started: Instant) -> Duration {
    started
        .elapsed()
        .saturating_sub(Duration::from_millis(activity.load(Ordering::Relaxed)))
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn state_error(path: &Path, source: std::io::Error) -> CoordinatorError {
    CoordinatorError::State {
        path: path.to_path_buf(),
        source,
    }
}

struct ConnectionGuard(Arc<AtomicUsize>);

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::{acquire_process_lock, serve, tokens_match, write_receipt};
    use crate::paths::private_directory;
    use crate::protocol::{
        AttemptOutcome, ClientMessage, EndpointKind, PROTOCOL_VERSION, Receipt, RequestLane,
        RequestPriority, ServerMessage, read_frame, write_frame,
    };
    use nan_harness_private_fs::open_private_read;
    use tokio::net::{TcpListener, TcpStream};

    #[test]
    fn process_election_and_receipt_replacement_are_deterministic() {
        let temporary = tempfile::tempdir().expect("temporary directory should exist");
        let directory = temporary.path().join("coordinator/v1");
        private_directory(&directory).expect("coordinator directory should be private");
        let lock = acquire_process_lock(&directory).expect("first process should win election");
        assert!(acquire_process_lock(&directory).is_err());

        for generation in ["first", "second"] {
            write_receipt(
                &directory,
                &Receipt {
                    protocol_version: PROTOCOL_VERSION,
                    port: 42,
                    token: "private-token".to_owned(),
                    generation: generation.to_owned(),
                    pid: 42,
                },
            )
            .expect("receipt should replace safely");
        }
        let (file, _) = open_private_read(&directory.join("receipt.json"))
            .expect("receipt should be private and readable");
        let receipt: Receipt = serde_json::from_reader(file).expect("receipt should contain JSON");
        assert_eq!(receipt.generation, "second");
        assert!(tokens_match("private-token", &receipt.token));
        assert!(!tokens_match("private-token", "wrong-token"));
        drop(lock);
        acquire_process_lock(&directory).expect("election lock should be released");
    }

    #[tokio::test]
    async fn authenticated_ipc_grants_and_completes_a_lease() {
        let temporary = tempfile::tempdir().expect("temporary directory should exist");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test coordinator should bind");
        let address = listener
            .local_addr()
            .expect("listener address should exist");
        let cache = temporary.path().join("capacity.json");
        let daemon = tokio::spawn(serve(listener, "token".to_owned(), cache));

        let mut stream = TcpStream::connect(address)
            .await
            .expect("client should connect");
        write_frame(
            &mut stream,
            &ClientMessage::Acquire {
                protocol_version: PROTOCOL_VERSION,
                token: "token".to_owned(),
                scope: "credential".to_owned(),
                launch_id: "codex".to_owned(),
                endpoint: EndpointKind::Inference,
                model: Some("model".to_owned()),
                lane: RequestLane::Inference,
                priority: RequestPriority::Foreground,
            },
        )
        .await
        .expect("acquire should write");
        let grant = read_frame::<ServerMessage>(&mut stream)
            .await
            .expect("grant should read");
        assert!(matches!(grant, ServerMessage::Granted { .. }));

        write_frame(
            &mut stream,
            &ClientMessage::Observe {
                lease_id: 1,
                outcome: AttemptOutcome::Success,
                retry_after_ms: None,
            },
        )
        .await
        .expect("outcome should write");
        let completion = read_frame::<ServerMessage>(&mut stream)
            .await
            .expect("completion should read");
        assert!(matches!(completion, ServerMessage::Complete));
        drop(stream);
        daemon.abort();
    }
}
