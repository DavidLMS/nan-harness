use super::{
    ActorSetup, CoordinationLock, LeaseMarker, LocalSearxngSpec, LockAttempt,
    SearchSupervisorError, SearchSupervisorTimings, active_search_interests, create_lease_marker,
    filesystem_error, read_record, readiness_loop, remove_owned_record, try_acquire_lock,
    write_record,
};
use crate::process::spawn_hosted_searxng;
use crate::searxng::SearxngCommand;
use nan_harness_private_fs::{
    PrivatePathKind, create_private_dir_all, open_private_read, restrict_path,
};
use nan_harness_search::SearxngConfig;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};

const START_LOCK: &str = ".nan-harness-searxng-start.lock";
const MAX_HOST_REQUEST_BYTES: usize = 64 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HostRequest {
    directory: PathBuf,
    endpoint: String,
    command: SearxngCommand,
    shutdown_grace: Duration,
    grace_recheck: Duration,
    recovery_backoff: Duration,
}

pub(super) async fn acquire(
    setup: &ActorSetup,
    executable: &Path,
) -> Result<LeaseMarker, SearchSupervisorError> {
    let spec = &setup.spec;
    create_private_dir_all(&spec.configuration_directory).map_err(|source| {
        filesystem_error(
            "create host directory",
            spec.configuration_directory.clone(),
            source,
        )
    })?;
    // Lease creation and idle shutdown share this gate. The host cannot stop after observing
    // zero interests while a client is in the middle of acquiring a new one.
    let _start = wait_for_lock(&start_lock(spec), setup.timings.coordination_timeout).await?;
    let marker = create_lease_marker(&spec.configuration_directory)?;
    let mut child = match try_acquire_lock(&spec.lock_path())? {
        LockAttempt::Busy => {
            let record = read_record(spec)?.ok_or(SearchSupervisorError::InvalidRecord)?;
            if record.endpoint != spec.endpoint.base_url_string() {
                return Err(SearchSupervisorError::RecordConflict);
            }
            None
        }
        LockAttempt::Acquired(lock) => {
            // A stale PID is diagnostic data, never authority to signal a process. The old
            // backend's lifetime pipe closes with its host; an unrelated listener is preserved.
            if endpoint_is_listening(spec).await {
                return Err(SearchSupervisorError::RecordConflict);
            }
            let _ = read_record(spec)?;
            remove_owned_record(spec);
            let request_path = write_request(spec, setup.timings)?;
            drop(lock);
            Some(start_host(executable, &request_path)?)
        }
    };
    let readiness = readiness_loop(&*setup.probe, &spec.endpoint, setup.timings);
    let ready = if let Some(host) = child.as_mut() {
        tokio::select! {
            ready = readiness => ready,
            _ = host.wait() => return Err(SearchSupervisorError::ProcessUnavailable),
        }
    } else {
        readiness.await
    };
    if let Some(mut child) = child.take() {
        let exited = child
            .try_wait()
            .map_err(SearchSupervisorError::Spawn)?
            .is_some();
        tokio::spawn(async move {
            let _ = child.wait().await;
        });
        if exited {
            return Err(SearchSupervisorError::ProcessUnavailable);
        }
    }
    if !ready {
        return Err(SearchSupervisorError::ReadinessTimeout);
    }
    Ok(marker)
}

fn start_lock(spec: &LocalSearxngSpec) -> PathBuf {
    spec.configuration_directory.join(START_LOCK)
}

async fn endpoint_is_listening(spec: &LocalSearxngSpec) -> bool {
    let url = spec.endpoint.base_url();
    let host = url.host_str().unwrap_or_default().trim_matches(['[', ']']);
    let port = url.port_or_known_default().unwrap_or(80);
    tokio::time::timeout(
        Duration::from_secs(2),
        tokio::net::TcpStream::connect((host, port)),
    )
    .await
    .is_ok_and(|result| result.is_ok())
}

async fn wait_for_lock(
    path: &Path,
    timeout: Duration,
) -> Result<CoordinationLock, SearchSupervisorError> {
    let deadline = Instant::now() + timeout;
    loop {
        if let LockAttempt::Acquired(lock) = try_acquire_lock(path)? {
            return Ok(lock);
        }
        if Instant::now() >= deadline {
            return Err(SearchSupervisorError::CoordinationTimeout);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn write_request(
    spec: &LocalSearxngSpec,
    timings: SearchSupervisorTimings,
) -> Result<PathBuf, SearchSupervisorError> {
    let write = || -> io::Result<PathBuf> {
        let mut file = tempfile::Builder::new()
            .prefix(".nan-harness-searxng-host-")
            .tempfile_in(&spec.configuration_directory)?;
        let temporary_path = file.path().to_path_buf();
        // `tempfile` owns the handle, while `restrict_path` can harden its named file on Windows
        // without requiring the default temporary handle to request `WRITE_DAC`.
        restrict_path(&temporary_path, PrivatePathKind::File)?;
        serde_json::to_writer(
            &mut file,
            &HostRequest {
                directory: spec.configuration_directory.clone(),
                endpoint: spec.endpoint.base_url_string(),
                command: spec.command.clone(),
                shutdown_grace: timings.shutdown_grace,
                grace_recheck: timings.grace_recheck,
                recovery_backoff: timings.recovery_backoff,
            },
        )?;
        file.as_file().sync_all()?;
        file.keep()
            .map(|(_, path)| path)
            .map_err(|error| error.error)
    };
    write().map_err(|source| {
        filesystem_error(
            "write host request",
            spec.configuration_directory.clone(),
            source,
        )
    })
}

fn start_host(executable: &Path, request: &Path) -> Result<Child, SearchSupervisorError> {
    let child = spawn_host_process(executable, request);
    if let Err(source) = child {
        let _ = fs::remove_file(request);
        return Err(SearchSupervisorError::Spawn(source));
    }
    child.map_err(SearchSupervisorError::Spawn)
}

fn host_command(executable: &Path, request: &Path) -> Command {
    let mut command = Command::new(executable);
    command
        .arg("__searxng-host")
        .arg(request)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env_remove("NAN_API_KEY")
        .env_remove("NAN_HARNESS_SEARCH_API_KEY")
        .kill_on_drop(false);
    command
}

#[cfg(unix)]
fn spawn_host_process(executable: &Path, request: &Path) -> io::Result<Child> {
    let mut command = host_command(executable, request);
    command.process_group(0);
    command.spawn()
}

#[cfg(windows)]
fn spawn_host_process(executable: &Path, request: &Path) -> io::Result<Child> {
    use std::io::ErrorKind;
    use std::os::windows::process::CommandExt as _;

    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;

    let mut command = host_command(executable, request);
    command.creation_flags(CREATE_BREAKAWAY_FROM_JOB | DETACHED_PROCESS);
    match command.spawn() {
        Ok(child) => Ok(child),
        Err(error) if error.kind() == ErrorKind::PermissionDenied => {
            // Hosted CI jobs may forbid breakaway. The plain-child fallback keeps the host alive
            // after its launcher exits; normal launches still break away from the harness job
            // when the OS permits it.
            let mut fallback = host_command(executable, request);
            fallback.spawn()
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(any(unix, windows)))]
fn spawn_host_process(executable: &Path, request: &Path) -> io::Result<Child> {
    host_command(executable, request).spawn()
}

/// Runs the private host entry point dispatched by the CLI.
///
/// The host owns both the service lock and a handle to its backend. It never adopts or signals
/// a process identified only by a persisted PID. A private pipe makes even abrupt host death
/// terminate its Python backend.
///
/// # Errors
///
/// Returns an error for an invalid private request, foreign coordination state, or process failure.
pub async fn run_searxng_host(request_path: impl AsRef<Path>) -> Result<(), SearchSupervisorError> {
    let request = read_request(request_path.as_ref())?;
    let endpoint = SearxngConfig::local(&request.endpoint)
        .map_err(|_| SearchSupervisorError::InvalidRecord)?;
    let spec = LocalSearxngSpec::new(&request.directory, endpoint, request.command.clone())?;
    let LockAttempt::Acquired(service) = try_acquire_lock(&spec.lock_path())? else {
        return Err(SearchSupervisorError::RecordConflict);
    };
    let _ = read_record(&spec)?;
    let result = host_backend(&spec, &request).await;
    remove_owned_record(&spec);
    drop(service);
    result.map(drop)
}

fn read_request(path: &Path) -> Result<HostRequest, SearchSupervisorError> {
    let (file, _) = open_private_read(path)
        .map_err(|source| filesystem_error("read host request", path.to_path_buf(), source))?;
    let mut bytes = Vec::new();
    file.take((MAX_HOST_REQUEST_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| filesystem_error("read host request", path.to_path_buf(), source))?;
    if bytes.len() > MAX_HOST_REQUEST_BYTES {
        return Err(SearchSupervisorError::InvalidRecord);
    }
    let request: HostRequest =
        serde_json::from_slice(&bytes).map_err(|_| SearchSupervisorError::InvalidRecord)?;
    if path.parent() != Some(request.directory.as_path())
        || request.shutdown_grace > Duration::from_mins(5)
        || request.grace_recheck.is_zero()
        || request.grace_recheck > Duration::from_secs(5)
        || request.recovery_backoff > Duration::from_secs(30)
    {
        return Err(SearchSupervisorError::InvalidRecord);
    }
    fs::remove_file(path)
        .map_err(|source| filesystem_error("consume host request", path.to_path_buf(), source))?;
    Ok(request)
}

async fn host_backend(
    spec: &LocalSearxngSpec,
    request: &HostRequest,
) -> Result<Option<CoordinationLock>, SearchSupervisorError> {
    let mut recovered = false;
    loop {
        let (mut backend, _lifetime_pipe) =
            spawn_hosted_searxng(&request.command).map_err(SearchSupervisorError::Spawn)?;
        write_record(spec, backend.id())?;
        let mut idle_since = None;
        let mut poll = tokio::time::interval(request.grace_recheck.max(Duration::from_millis(5)));
        loop {
            tokio::select! {
                _ = backend.wait() => break,
                _ = poll.tick() => {
                match begin_idle_shutdown(spec, request.shutdown_grace, &mut idle_since) {
                    Ok(Some(shutdown)) => {
                        backend.kill().await.map_err(SearchSupervisorError::Spawn)?;
                        return Ok(Some(shutdown));
                    }
                    Ok(None) => {}
                    Err(_) => idle_since = None,
                }
                }
            }
        }
        if recovered || active_search_interests(&spec.configuration_directory)? == 0 {
            return Ok(None);
        }
        recovered = true;
        tokio::time::sleep(request.recovery_backoff).await;
    }
}

fn begin_idle_shutdown(
    spec: &LocalSearxngSpec,
    grace: Duration,
    idle_since: &mut Option<Instant>,
) -> Result<Option<CoordinationLock>, SearchSupervisorError> {
    let LockAttempt::Acquired(start) = try_acquire_lock(&start_lock(spec))? else {
        *idle_since = None;
        return Ok(None);
    };
    if active_search_interests(&spec.configuration_directory)? > 0 {
        *idle_since = None;
        return Ok(None);
    }
    Ok((idle_since.get_or_insert_with(Instant::now).elapsed() >= grace).then_some(start))
}
