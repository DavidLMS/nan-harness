use crate::CoordinatorError;
use crate::paths::private_directory;
use crate::protocol::{
    AttemptOutcome, AttemptPhase, ClientMessage, EndpointKind, PROTOCOL_VERSION, Receipt,
    RequestLane, RequestPriority, ServerMessage, read_frame, write_frame,
};
use nan_harness_core::SecretValue;
use nan_harness_private_fs::{open_private_new, open_private_read};
use sha2::{Digest as _, Sha256};
use std::fmt::Write as _;
use std::io::Write as IoWrite;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use url::Url;

const STARTUP_BUDGET: Duration = Duration::from_secs(2);
const CONNECT_BUDGET: Duration = Duration::from_millis(25);
const FAILED_PROBE_COOLDOWN: Duration = Duration::from_secs(5);
const DISABLE_ENVIRONMENT: &str = "NAN_HARNESS_INTERNAL_DISABLE_COORDINATOR";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDirective {
    Complete,
    RetryAfter(Duration),
}

#[derive(Clone)]
pub struct CoordinatorClient {
    directory: PathBuf,
    scope: String,
    launch_id: Arc<str>,
    retry_probe_at: Arc<Mutex<Instant>>,
}

pub struct RequestLease {
    stream: Option<TcpStream>,
    lease_id: u64,
    pub queued: Duration,
}

impl CoordinatorClient {
    #[must_use]
    pub fn new(
        provider_base_url: &str,
        api_key: &SecretValue,
        launch_id: impl Into<String>,
    ) -> Option<Self> {
        Self::try_new(provider_base_url, api_key, launch_id)
            .ok()
            .flatten()
    }

    /// Creates a coordinator client for a managed process without hiding setup failures.
    ///
    /// # Errors
    ///
    /// Returns an error when private coordinator state cannot be prepared.
    pub fn try_new(
        provider_base_url: &str,
        api_key: &SecretValue,
        launch_id: impl Into<String>,
    ) -> Result<Option<Self>, CoordinatorError> {
        if std::env::var_os(DISABLE_ENVIRONMENT).is_some() || !crate::paths::is_managed_process() {
            return Ok(None);
        }
        let directory = crate::config_directory()?.join("coordinator/v1");
        private_directory(&directory)?;
        let salt = load_or_create_salt(&directory).map_err(|source| CoordinatorError::State {
            path: directory.join("scope.salt"),
            source,
        })?;
        let origin = canonical_origin(provider_base_url).ok_or(CoordinatorError::Protocol(
            "provider URL has no valid origin",
        ))?;
        let scope = api_key.with_secret(|secret| fingerprint(&salt, &origin, secret));
        Ok(Some(Self {
            directory,
            scope,
            launch_id: launch_id.into().into(),
            retry_probe_at: Arc::new(Mutex::new(Instant::now())),
        }))
    }

    /// Waits for capacity in the default lane for an endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error when coordination fails or the capacity wait expires.
    pub async fn acquire(
        &self,
        endpoint: EndpointKind,
        model: Option<&str>,
        budget: Duration,
    ) -> Result<Option<RequestLease>, CoordinatorError> {
        let (lane, priority) = match endpoint {
            EndpointKind::Inference => (RequestLane::Inference, RequestPriority::Foreground),
            EndpointKind::Models => (RequestLane::Control, RequestPriority::Background),
            EndpointKind::Search => (RequestLane::Control, RequestPriority::Foreground),
        };
        self.acquire_classified(endpoint, model, lane, priority, budget)
            .await
    }

    /// Waits for capacity with an explicit scheduling lane and priority.
    ///
    /// # Errors
    ///
    /// Returns an error when coordination fails or the capacity wait expires.
    pub async fn acquire_classified(
        &self,
        endpoint: EndpointKind,
        model: Option<&str>,
        lane: RequestLane,
        priority: RequestPriority,
        budget: Duration,
    ) -> Result<Option<RequestLease>, CoordinatorError> {
        let started = Instant::now();
        let (mut stream, receipt) = self.connect_or_start().await?;
        let request = ClientMessage::Acquire {
            protocol_version: PROTOCOL_VERSION,
            token: receipt.token,
            scope: self.scope.clone(),
            launch_id: self.launch_id.to_string(),
            endpoint,
            model: model.map(ToOwned::to_owned),
            lane,
            priority,
        };
        write_frame(&mut stream, &request)
            .await
            .map_err(|_| CoordinatorError::Protocol("could not request provider capacity"))?;
        let remaining = budget.saturating_sub(started.elapsed());
        let response = tokio::time::timeout(remaining, read_frame::<ServerMessage>(&mut stream))
            .await
            .map_err(|_| CoordinatorError::QueueTimeout)?
            .map_err(|_| CoordinatorError::Protocol("coordinator disconnected while queued"))?;
        match response {
            ServerMessage::Granted {
                lease_id,
                queued_ms,
            } => Ok(Some(RequestLease {
                stream: Some(stream),
                lease_id,
                queued: Duration::from_millis(queued_ms),
            })),
            ServerMessage::Rejected { .. } => Err(CoordinatorError::Protocol(
                "coordinator rejected the capacity request",
            )),
            ServerMessage::Retry { .. } | ServerMessage::Complete => Err(
                CoordinatorError::Protocol("coordinator returned an invalid capacity response"),
            ),
        }
    }

    async fn connect_or_start(&self) -> Result<(TcpStream, Receipt), CoordinatorError> {
        if self
            .retry_probe_at
            .lock()
            .is_ok_and(|deadline| *deadline > Instant::now())
        {
            return Err(CoordinatorError::Protocol(
                "coordinator is temporarily unavailable",
            ));
        }
        if let Some(stream) = connect_from_receipt(&self.directory).await? {
            return Ok(stream);
        }
        spawn_daemon();
        let deadline = Instant::now() + STARTUP_BUDGET;
        loop {
            if let Some(stream) = connect_from_receipt(&self.directory).await? {
                return Ok(stream);
            }
            if Instant::now() >= deadline {
                if let Ok(receipt) = read_receipt(&self.directory)
                    && receipt.protocol_version != PROTOCOL_VERSION
                {
                    return Err(CoordinatorError::IncompatibleDaemon {
                        detected: receipt.protocol_version,
                    });
                }
                if let Ok(mut retry_probe_at) = self.retry_probe_at.lock() {
                    *retry_probe_at = Instant::now() + FAILED_PROBE_COOLDOWN;
                }
                return Err(CoordinatorError::Protocol("coordinator did not start"));
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

impl RequestLease {
    pub async fn headers_received(&mut self, elapsed: Duration) {
        let Some(stream) = self.stream.as_mut() else {
            return;
        };
        let message = ClientMessage::Progress {
            lease_id: self.lease_id,
            phase: AttemptPhase::HeadersReceived,
            elapsed_ms: duration_millis(elapsed),
        };
        if write_frame(stream, &message).await.is_err() {
            self.stream = None;
        }
    }

    pub async fn observe(
        &mut self,
        outcome: AttemptOutcome,
        retry_after: Option<Duration>,
    ) -> RetryDirective {
        let Some(stream) = self.stream.as_mut() else {
            return RetryDirective::Complete;
        };
        let message = ClientMessage::Observe {
            lease_id: self.lease_id,
            outcome,
            retry_after_ms: retry_after.map(duration_millis),
        };
        if write_frame(stream, &message).await.is_err() {
            self.stream = None;
            return RetryDirective::Complete;
        }
        let response = read_frame::<ServerMessage>(stream).await;
        match response {
            Ok(ServerMessage::Retry { delay_ms }) => {
                self.stream = None;
                RetryDirective::RetryAfter(Duration::from_millis(delay_ms))
            }
            Ok(
                ServerMessage::Complete
                | ServerMessage::Granted { .. }
                | ServerMessage::Rejected { .. },
            )
            | Err(_) => {
                self.stream = None;
                RetryDirective::Complete
            }
        }
    }
}

async fn connect_from_receipt(
    directory: &Path,
) -> Result<Option<(TcpStream, Receipt)>, CoordinatorError> {
    let Ok(receipt) = read_receipt(directory) else {
        return Ok(None);
    };
    let address = SocketAddr::from(([127, 0, 0, 1], receipt.port));
    if receipt.protocol_version != PROTOCOL_VERSION {
        let live = tokio::time::timeout(CONNECT_BUDGET, TcpStream::connect(address))
            .await
            .is_ok_and(|result| result.is_ok());
        return if live {
            Err(CoordinatorError::IncompatibleDaemon {
                detected: receipt.protocol_version,
            })
        } else {
            Ok(None)
        };
    }
    let stream = tokio::time::timeout(CONNECT_BUDGET, TcpStream::connect(address))
        .await
        .ok()
        .and_then(Result::ok);
    Ok(stream.map(|stream| (stream, receipt)))
}

fn read_receipt(directory: &Path) -> Result<Receipt, std::io::Error> {
    let (file, _) = open_private_read(&directory.join("receipt.json"))?;
    serde_json::from_reader(file).map_err(std::io::Error::other)
}

fn load_or_create_salt(directory: &Path) -> Result<Vec<u8>, std::io::Error> {
    let path = directory.join("scope.salt");
    if let Ok((mut file, _)) = open_private_read(&path) {
        let mut salt = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut salt)?;
        if salt.len() == 32 {
            return Ok(salt);
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "coordinator scope salt has an invalid length",
        ));
    }
    let mut salt = vec![0_u8; 32];
    getrandom::fill(&mut salt).map_err(std::io::Error::other)?;
    match open_private_new(&path) {
        Ok(mut file) => {
            IoWrite::write_all(&mut file, &salt)?;
            file.sync_all()?;
            Ok(salt)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let (mut file, _) = open_private_read(&path)?;
            let mut existing = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut existing)?;
            (existing.len() == 32).then_some(existing).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "coordinator scope salt has an invalid length",
                )
            })
        }
        Err(error) => Err(error),
    }
}

fn canonical_origin(value: &str) -> Option<String> {
    Url::parse(value)
        .ok()?
        .origin()
        .ascii_serialization()
        .into()
}

fn fingerprint(salt: &[u8], origin: &str, secret: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"nan-harness-coordinator-scope-v1\0");
    digest.update(salt);
    digest.update(b"\0");
    digest.update(origin.as_bytes());
    digest.update(b"\0");
    digest.update(secret.as_bytes());
    let bytes = digest.finalize();
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut value, "{byte:02x}");
    }
    value
}

fn spawn_daemon() {
    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    let mut command = Command::new(executable);
    command
        .arg("__coordinator")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_detached(&mut command);
    if let Ok(mut child) = command.spawn() {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

#[cfg(unix)]
fn configure_detached(command: &mut Command) {
    use std::os::unix::process::CommandExt as _;
    command.process_group(0);
}

#[cfg(windows)]
fn configure_detached(command: &mut Command) {
    use std::os::windows::process::CommandExt as _;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    command.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
}

#[cfg(not(any(unix, windows)))]
fn configure_detached(_command: &mut Command) {}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::{CoordinatorClient, canonical_origin, fingerprint};
    use crate::protocol::{ClientMessage, PROTOCOL_VERSION, Receipt, read_frame};
    use crate::{CoordinatorError, EndpointKind};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use tokio::net::TcpListener;

    #[test]
    fn scope_fingerprint_is_stable_and_origin_scoped() {
        let salt = [7_u8; 32];
        let first = fingerprint(&salt, "https://api.example.com", "secret");
        assert_eq!(
            first,
            fingerprint(&salt, "https://api.example.com", "secret")
        );
        assert_ne!(
            first,
            fingerprint(&salt, "https://other.example.com", "secret")
        );
        assert_eq!(
            canonical_origin("https://api.example.com/v1"),
            Some("https://api.example.com".to_owned())
        );
    }

    #[tokio::test]
    async fn capacity_wait_timeout_is_an_error_instead_of_uncoordinated_fallback() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let receipt = Receipt {
            protocol_version: PROTOCOL_VERSION,
            port: listener.local_addr().expect("listener address").port(),
            token: "test-token".to_owned(),
            generation: "test-generation".to_owned(),
            pid: std::process::id(),
        };
        std::fs::write(
            temporary.path().join("receipt.json"),
            serde_json::to_vec(&receipt).expect("receipt should encode"),
        )
        .expect("receipt should be written");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("client should connect");
            let _: ClientMessage = read_frame(&mut stream)
                .await
                .expect("capacity request should arrive");
            tokio::time::sleep(Duration::from_secs(1)).await;
        });
        let client = CoordinatorClient {
            directory: temporary.path().to_owned(),
            scope: "scope".to_owned(),
            launch_id: Arc::from("launch"),
            retry_probe_at: Arc::new(Mutex::new(Instant::now())),
        };

        let result = client
            .acquire(
                EndpointKind::Inference,
                Some("model"),
                Duration::from_millis(20),
            )
            .await;
        assert!(matches!(result, Err(CoordinatorError::QueueTimeout)));
        server.abort();
    }
}
