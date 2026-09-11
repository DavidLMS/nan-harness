//! Shared lifecycle supervision for a configured local `SearXNG` instance.
//!
//! This module owns process lifetime only. It does not inspect harness
//! configuration, choose a search policy, or change a bridge configuration.
//! Callers provide an already validated local endpoint and an already planned
//! direct command, then hold a [`SearchLease`] for the duration of their
//! session. Search startup errors are typed and advisory to the caller; the
//! harness launch can continue without search.

mod host;

pub use host::run_searxng_host;

use crate::process::{ManagedChild, spawn_searxng};
use crate::searxng::{
    SearxngCommand, SearxngInstallPaths, SearxngPlatform, read_searxng_install_metadata,
};
use futures_util::future::BoxFuture;
use nan_harness_private_fs::{
    PrivatePathKind, create_private_dir_all, open_private_new, open_private_read,
    open_private_read_write, restrict_path,
};
use nan_harness_search::{SearxngConfig, SearxngMode};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::fs::{self, File, TryLockError};
use std::io::{self, ErrorKind, Read as _, Seek as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::mpsc;

const RECORD_SCHEMA_VERSION: u8 = 1;
const RECORD_OWNER: &str = "nan-harness-searxng-supervisor-v1";
const RECORD_FILE_NAME: &str = ".nan-harness-searxng.json";
const LOCK_FILE_NAME: &str = ".nan-harness-searxng.lock";
const LEASE_FILE_PREFIX: &str = ".nan-harness-searxng-interest-";
const LEASE_MARKER: &[u8] = b"nan-harness searxng interest v1\n";
const MAX_RECORD_BYTES: u64 = 16 * 1024;
const MAX_LEASE_ID_ATTEMPTS: usize = 8;
const MAX_INTEREST_MARKERS: usize = 1024;

static ACTIVE_LEASES: std::sync::LazyLock<Mutex<std::collections::BTreeSet<PathBuf>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::BTreeSet::new()));

/// A local endpoint and direct process command supplied by the launch layer.
///
/// The supervisor intentionally receives a command rather than inferring one
/// from a harness or provider plan. The command is executed without a shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSearxngSpec {
    /// Private directory used for coordination records and interest leases.
    pub configuration_directory: PathBuf,
    /// Validated loopback endpoint served by the local process.
    pub endpoint: SearxngConfig,
    /// Direct `SearXNG` process invocation.
    pub command: SearxngCommand,
}

impl LocalSearxngSpec {
    /// Builds a local specification without reading or changing the filesystem.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint is not a local loopback endpoint or the
    /// coordination directory is empty.
    pub fn new(
        configuration_directory: impl Into<PathBuf>,
        endpoint: SearxngConfig,
        command: SearxngCommand,
    ) -> Result<Self, SearchSupervisorError> {
        let configuration_directory = configuration_directory.into();
        if endpoint.mode() != SearxngMode::Local {
            return Err(SearchSupervisorError::InvalidConfiguration(
                "the supervised SearXNG endpoint must be local",
            ));
        }
        if configuration_directory.as_os_str().is_empty() {
            return Err(SearchSupervisorError::InvalidConfiguration(
                "the SearXNG coordination directory is empty",
            ));
        }
        Ok(Self {
            configuration_directory,
            endpoint,
            command,
        })
    }

    fn record_path(&self) -> PathBuf {
        self.configuration_directory.join(RECORD_FILE_NAME)
    }

    fn lock_path(&self) -> PathBuf {
        self.configuration_directory.join(LOCK_FILE_NAME)
    }
}

/// Tunable lifecycle bounds. The defaults are the public local-search
/// contract: a 60-second readiness wait and a 30-second idle grace period.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchSupervisorTimings {
    /// Maximum time spent waiting for one process generation to become ready.
    pub readiness_timeout: Duration,
    /// Delay between bounded readiness probes.
    pub readiness_retry: Duration,
    /// Delay before the single crash recovery attempt.
    pub recovery_backoff: Duration,
    /// Time from the last interest release before stopping an idle process.
    pub shutdown_grace: Duration,
    /// Poll period used while another process still owns an interest lease.
    pub grace_recheck: Duration,
    /// Bound for discovering a process that is currently starting elsewhere.
    pub coordination_timeout: Duration,
}

impl Default for SearchSupervisorTimings {
    fn default() -> Self {
        Self {
            readiness_timeout: Duration::from_mins(1),
            readiness_retry: Duration::from_millis(100),
            recovery_backoff: Duration::from_secs(1),
            shutdown_grace: Duration::from_secs(30),
            grace_recheck: Duration::from_millis(100),
            coordination_timeout: Duration::from_mins(1),
        }
    }
}

/// A cloneable shared supervisor. Clones coordinate through one actor and all
/// leases count toward the same process lifetime.
#[derive(Clone)]
pub struct SearchSupervisor {
    inner: Arc<SupervisorInner>,
    host_executable: Option<PathBuf>,
}

impl std::fmt::Debug for SearchSupervisor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SearchSupervisor")
            .field("configured", &self.inner.setup.is_some())
            .finish_non_exhaustive()
    }
}

impl SearchSupervisor {
    /// Builds a supervisor only for the owned, validated standalone installation.
    ///
    /// Remote and Docker endpoints retain their explicit lifecycle semantics and return `None`.
    /// A missing active installation is also a no-op: launch-time supervision must never create
    /// or install search state on its own.
    ///
    /// # Errors
    ///
    /// Returns an advisory error when an active standalone installation exists but fails its
    /// ownership or metadata validation.
    pub fn from_standalone_install(
        endpoint: &SearxngConfig,
        home: Option<&Path>,
    ) -> Result<Option<Self>, SearchSupervisorError> {
        if endpoint.mode() != SearxngMode::Local {
            return Ok(None);
        }
        let Some(platform) = SearxngPlatform::current() else {
            return Ok(None);
        };
        let Some(home) = home else {
            return Ok(None);
        };
        let paths = SearxngInstallPaths::for_user_home(home, platform);
        let Some(metadata) =
            read_searxng_install_metadata(&paths).map_err(SearchSupervisorError::Installation)?
        else {
            return Ok(None);
        };
        if metadata.platform != platform.as_str() {
            return Err(SearchSupervisorError::InstallationPlatformMismatch);
        }
        let spec = LocalSearxngSpec::new(
            paths.root().to_path_buf(),
            endpoint.clone(),
            paths.runtime_command(platform, metadata.python_bootstrapped),
        )?;
        let executable = std::env::current_exe().map_err(SearchSupervisorError::Spawn)?;
        Self::new(Some(spec)).map(|supervisor| Some(supervisor.with_host_executable(executable)))
    }

    /// Creates a supervisor using the production managed-child and HTTP probe
    /// implementations. `None` is an intentional no-op and touches no state.
    ///
    /// # Errors
    ///
    /// Returns an error when the local specification is invalid or the bounded
    /// readiness client cannot be built.
    pub fn new(spec: Option<LocalSearxngSpec>) -> Result<Self, SearchSupervisorError> {
        Self::with_timings(spec, SearchSupervisorTimings::default())
    }

    /// Creates a supervisor with explicit lifecycle bounds.
    ///
    /// This is useful to callers that need a shorter test or embedding budget;
    /// production callers should normally use [`Self::new`].
    ///
    /// # Errors
    ///
    /// Returns an error when the local specification is invalid or the bounded
    /// readiness client cannot be built.
    pub fn with_timings(
        spec: Option<LocalSearxngSpec>,
        timings: SearchSupervisorTimings,
    ) -> Result<Self, SearchSupervisorError> {
        let setup = spec.map(|spec| {
            Ok(ActorSetup {
                spec: Arc::new(spec),
                timings,
                factory: Arc::new(ManagedChildFactory),
                probe: Arc::new(HttpReadinessProbe::new()?),
            })
        });
        let setup = setup.transpose()?;
        Ok(Self {
            inner: Arc::new(SupervisorInner::new(setup)),
            host_executable: None,
        })
    }

    /// Uses a separate executable to host the shared backend beyond this runtime's lifetime.
    ///
    /// The executable must dispatch `__searxng-host <request-file>` to [`run_searxng_host`].
    /// Managed standalone installations use the CLI executable automatically. Without this
    /// setting, the supervisor owns the backend in the current runtime, which is useful for
    /// embedding and in-process tests but cannot outlive that runtime.
    #[must_use]
    pub fn with_host_executable(mut self, executable: impl Into<PathBuf>) -> Self {
        self.host_executable = Some(executable.into());
        self
    }

    /// Acquires a lease for the configured local search service.
    ///
    /// The first lease starts the service asynchronously and waits at most the
    /// configured readiness bound. Later leases reuse the ready generation.
    /// `Ok(None)` means search was not configured; all other errors are
    /// advisory and must not force a harness launch to fail.
    ///
    /// # Errors
    ///
    /// Returns a typed advisory error when coordination, process startup, or
    /// readiness fails. The caller may continue the harness launch without the
    /// optional search service.
    pub async fn acquire(&self) -> Result<Option<SearchLease>, SearchSupervisorError> {
        let Some(setup) = self.inner.setup.as_ref() else {
            return Ok(None);
        };
        if let Some(executable) = &self.host_executable {
            let marker = host::acquire(setup, executable).await?;
            return Ok(Some(SearchLease {
                endpoint: setup.spec.endpoint.clone(),
                marker,
                supervisor: Arc::clone(&self.inner),
            }));
        }
        self.inner.start_actor(setup.clone());
        let (response, receiver) = tokio::sync::oneshot::channel();
        self.inner
            .messages
            .send(Message::Acquire { response })
            .map_err(|_| SearchSupervisorError::SupervisorClosed)?;
        receiver
            .await
            .map_err(|_| SearchSupervisorError::SupervisorClosed)?
    }
}

/// An interest lease that keeps a local `SearXNG` process alive.
///
/// The lease's private marker remains locked for its entire lifetime, allowing
/// a separate nan-harness process to discover active interest and preventing an
/// owner from shutting down while another session is still using the service.
pub struct SearchLease {
    endpoint: SearxngConfig,
    marker: LeaseMarker,
    supervisor: Arc<SupervisorInner>,
}

impl std::fmt::Debug for SearchLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SearchLease")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

impl SearchLease {
    /// Returns the endpoint that should be used by the later search bridge.
    #[must_use]
    pub const fn endpoint(&self) -> &SearxngConfig {
        &self.endpoint
    }

    /// Returns the canonical local endpoint string.
    #[must_use]
    pub fn base_url(&self) -> String {
        self.endpoint.base_url_string()
    }
}

impl Drop for SearchLease {
    fn drop(&mut self) {
        // Removing the marker before notifying the owner closes the small race
        // where the grace check could observe a lease that is already gone.
        self.marker.release();
        let _ = self.supervisor.messages.send(Message::Release);
    }
}

/// An interest marker for a managed search backend whose process is owned by another lifecycle
/// manager, such as Docker.
///
/// The marker uses the same locked-file protocol as [`SearchLease`], so status, update, and
/// uninstall operations in another nan-harness process can observe that a search session is
/// active. The directory must already exist; acquiring an interest never creates backend state.
pub struct SearchInterest {
    marker: LeaseMarker,
}

impl std::fmt::Debug for SearchInterest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SearchInterest")
            .field("marker", &self.marker.path)
            .finish()
    }
}

impl SearchInterest {
    /// Acquires a private interest marker in an existing managed-backend directory.
    ///
    /// # Errors
    ///
    /// Returns a filesystem or coordination error when the directory cannot be validated or the
    /// marker cannot be created and locked.
    pub fn acquire(directory: impl AsRef<Path>) -> Result<Self, SearchSupervisorError> {
        let directory = directory.as_ref();
        let metadata = fs::symlink_metadata(directory).map_err(|source| {
            filesystem_error(
                "inspect search interest directory",
                directory.to_path_buf(),
                source,
            )
        })?;
        if !metadata.is_dir() {
            return Err(SearchSupervisorError::InvalidConfiguration(
                "the search interest directory is not a directory",
            ));
        }
        Ok(Self {
            marker: create_lease_marker(directory)?,
        })
    }
}

/// Advisory failures from local search lifecycle supervision.
#[derive(Debug, Error)]
pub enum SearchSupervisorError {
    #[error("local SearXNG configuration is invalid: {0}")]
    InvalidConfiguration(&'static str),
    #[error("could not build the SearXNG readiness client")]
    ReadinessClient,
    #[error("could not access SearXNG coordination state at '{}': {operation}: {source}", path.display())]
    Filesystem {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("SearXNG coordination state is owned by another format")]
    ForeignRecord,
    #[error("SearXNG coordination state is invalid")]
    InvalidRecord,
    #[error("SearXNG coordination state does not match this endpoint")]
    RecordConflict,
    #[error("another SearXNG process is starting and did not publish state in time")]
    CoordinationTimeout,
    #[error("could not start the local SearXNG process")]
    Spawn(#[source] io::Error),
    #[error("local SearXNG did not become ready within the startup bound")]
    ReadinessTimeout,
    #[error("local SearXNG process exited before search became available")]
    ProcessUnavailable,
    #[error("SearXNG supervisor stopped unexpectedly")]
    SupervisorClosed,
    #[error("the owned standalone SearXNG installation could not be validated")]
    Installation(#[source] crate::searxng::SearxngInstallError),
    #[error("the owned standalone SearXNG installation targets a different platform")]
    InstallationPlatformMismatch,
}

struct SupervisorInner {
    messages: mpsc::UnboundedSender<Message>,
    receiver: Mutex<Option<mpsc::UnboundedReceiver<Message>>>,
    setup: Option<ActorSetup>,
    actor_started: std::sync::atomic::AtomicBool,
}

impl SupervisorInner {
    fn new(setup: Option<ActorSetup>) -> Self {
        let (messages, receiver) = mpsc::unbounded_channel();
        Self {
            messages,
            receiver: Mutex::new(Some(receiver)),
            setup,
            actor_started: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn start_actor(self: &Arc<Self>, setup: ActorSetup) {
        use std::sync::atomic::Ordering;

        if self
            .actor_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let receiver = self.receiver.lock().ok().and_then(|mut slot| slot.take());
            let Some(receiver) = receiver else {
                return;
            };
            let actor = Actor::new(setup, self.messages.clone(), Arc::downgrade(self), receiver);
            tokio::spawn(actor.run());
        }
    }
}

impl Drop for SupervisorInner {
    fn drop(&mut self) {
        let _ = self.messages.send(Message::OwnerDropped);
    }
}

#[derive(Clone)]
struct ActorSetup {
    spec: Arc<LocalSearxngSpec>,
    timings: SearchSupervisorTimings,
    factory: Arc<dyn ProcessFactory>,
    probe: Arc<dyn ReadinessProbe>,
}

trait ProcessFactory: Send + Sync {
    fn spawn(&self, command: &SearxngCommand) -> io::Result<Box<dyn SearchChild>>;
}

trait SearchChild: Send {
    fn id(&self) -> Option<u32>;
    fn kill(&mut self) -> BoxFuture<'_, io::Result<()>>;
    fn wait(&mut self) -> BoxFuture<'_, io::Result<std::process::ExitStatus>>;
}

struct ManagedChildFactory;

impl ProcessFactory for ManagedChildFactory {
    fn spawn(&self, command: &SearxngCommand) -> io::Result<Box<dyn SearchChild>> {
        Ok(Box::new(ManagedSearchChild(spawn_searxng(command)?)))
    }
}

struct ManagedSearchChild(ManagedChild);

impl SearchChild for ManagedSearchChild {
    fn id(&self) -> Option<u32> {
        self.0.id()
    }

    fn kill(&mut self) -> BoxFuture<'_, io::Result<()>> {
        Box::pin(self.0.kill())
    }

    fn wait(&mut self) -> BoxFuture<'_, io::Result<std::process::ExitStatus>> {
        Box::pin(self.0.wait())
    }
}

trait ReadinessProbe: Send + Sync {
    fn check<'a>(&'a self, endpoint: &'a SearxngConfig) -> BoxFuture<'a, bool>;
}

struct HttpReadinessProbe {
    client: reqwest::Client,
}

impl HttpReadinessProbe {
    fn new() -> Result<Self, SearchSupervisorError> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|_| SearchSupervisorError::ReadinessClient)?;
        Ok(Self { client })
    }
}

impl ReadinessProbe for HttpReadinessProbe {
    fn check<'a>(&'a self, endpoint: &'a SearxngConfig) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            self.client
                .get(endpoint.base_url().clone())
                .send()
                .await
                .is_ok_and(|response| response.status().is_success())
        })
    }
}

enum Message {
    Acquire {
        response: tokio::sync::oneshot::Sender<Result<Option<SearchLease>, SearchSupervisorError>>,
    },
    Release,
    ChildExited {
        generation: u64,
        stopped: bool,
        failed: bool,
    },
    Ready {
        generation: u64,
        available: bool,
    },
    StartRecovery,
    OwnerDropped,
    GraceExpired {
        generation: u64,
    },
}

struct PendingAcquire {
    response: tokio::sync::oneshot::Sender<Result<Option<SearchLease>, SearchSupervisorError>>,
}

enum Ownership {
    Owner(CoordinationLock),
    Discovered,
}

impl Ownership {
    fn is_owner(&self) -> bool {
        match self {
            Self::Owner(lock) => {
                let _ = &lock.file;
                true
            }
            Self::Discovered => false,
        }
    }
}

struct ProcessControl {
    generation: u64,
    commands: mpsc::UnboundedSender<ChildCommand>,
}

enum ChildCommand {
    Stop,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LifecycleState {
    Idle,
    Starting,
    Ready,
    Recovering,
    Stopping,
    Failed,
}

struct Actor {
    setup: ActorSetup,
    messages: mpsc::UnboundedSender<Message>,
    inner: Weak<SupervisorInner>,
    receiver: mpsc::UnboundedReceiver<Message>,
    state: LifecycleState,
    ownership: Option<Ownership>,
    process: Option<ProcessControl>,
    pending: Vec<PendingAcquire>,
    interests: usize,
    generation: u64,
    recovery_used: bool,
    grace_generation: Option<u64>,
    grace_saw_active_lease: bool,
    channel_open: bool,
}

impl Actor {
    fn new(
        setup: ActorSetup,
        messages: mpsc::UnboundedSender<Message>,
        inner: Weak<SupervisorInner>,
        receiver: mpsc::UnboundedReceiver<Message>,
    ) -> Self {
        Self {
            setup,
            messages,
            inner,
            receiver,
            state: LifecycleState::Idle,
            ownership: None,
            process: None,
            pending: Vec::new(),
            interests: 0,
            generation: 0,
            recovery_used: false,
            grace_generation: None,
            grace_saw_active_lease: false,
            channel_open: true,
        }
    }

    async fn run(mut self) {
        while self.channel_open || self.process.is_some() || !self.pending.is_empty() {
            let Some(message) = self.receiver.recv().await else {
                self.channel_open = false;
                if self.interests == 0 {
                    self.schedule_grace();
                }
                // Internal timers and process events use the same channel, but
                // with no external sender there is no useful work to await.
                if self.process.is_none() {
                    break;
                }
                continue;
            };
            self.handle(message).await;
        }
        self.finish_without_leases();
    }

    async fn handle(&mut self, message: Message) {
        match message {
            Message::Acquire { response } => self.acquire(response).await,
            Message::Release => self.release(),
            Message::ChildExited {
                generation,
                stopped,
                failed,
            } => self.child_exited(generation, stopped, failed),
            Message::Ready {
                generation,
                available,
            } => self.ready(generation, available),
            Message::StartRecovery => self.start_recovery().await,
            Message::OwnerDropped => self.owner_dropped(),
            Message::GraceExpired { generation } => self.grace_expired(generation),
        }
    }

    async fn acquire(
        &mut self,
        response: tokio::sync::oneshot::Sender<Result<Option<SearchLease>, SearchSupervisorError>>,
    ) {
        self.interests = self.interests.saturating_add(1);
        self.grace_generation = None;
        self.grace_saw_active_lease = false;
        match self.state {
            LifecycleState::Ready => self.respond_ready(response),
            LifecycleState::Failed if self.process.is_none() && self.interests == 1 => {
                self.state = LifecycleState::Idle;
                self.recovery_used = false;
                self.pending.push(PendingAcquire { response });
                self.start_initial().await;
            }
            LifecycleState::Failed => {
                self.interests = self.interests.saturating_sub(1);
                let _ = response.send(Err(SearchSupervisorError::ProcessUnavailable));
            }
            LifecycleState::Idle => {
                self.pending.push(PendingAcquire { response });
                self.start_initial().await;
            }
            LifecycleState::Starting | LifecycleState::Recovering | LifecycleState::Stopping => {
                self.pending.push(PendingAcquire { response });
            }
        }
    }

    async fn start_initial(&mut self) {
        let coordination = coordinate(&self.setup.spec, self.setup.timings).await;
        match coordination {
            Ok(Coordinated::Owner(lock)) => {
                self.ownership = Some(Ownership::Owner(lock));
                self.start_generation().await;
            }
            Ok(Coordinated::Discovered(record)) => {
                if record.endpoint != self.setup.spec.endpoint.base_url_string() {
                    self.fail_pending(&SearchSupervisorError::RecordConflict);
                    self.state = LifecycleState::Failed;
                    return;
                }
                self.ownership = Some(Ownership::Discovered);
                self.generation = self.generation.saturating_add(1);
                self.state = LifecycleState::Starting;
                self.start_readiness(self.generation);
            }
            Err(error) => {
                self.fail_pending(&error);
                self.state = LifecycleState::Failed;
                self.cleanup_ownership();
            }
        }
    }

    async fn start_generation(&mut self) {
        self.generation = self.generation.saturating_add(1);
        let generation = self.generation;
        let mut child = match self.setup.factory.spawn(&self.setup.spec.command) {
            Ok(child) => child,
            Err(error) => {
                let failure = SearchSupervisorError::Spawn(error);
                self.fail_pending(&failure);
                self.state = LifecycleState::Failed;
                self.cleanup_ownership();
                return;
            }
        };
        let pid = child.id();
        if let Err(error) = write_record(&self.setup.spec, pid) {
            let _ = child.kill().await;
            self.fail_pending(&error);
            self.state = LifecycleState::Failed;
            self.cleanup_ownership();
            return;
        }
        let (commands, command_receiver) = mpsc::unbounded_channel();
        self.process = Some(ProcessControl {
            generation,
            commands: commands.clone(),
        });
        let messages = self.messages.clone();
        tokio::spawn(monitor_child(child, generation, command_receiver, messages));
        self.state = if self.recovery_used {
            LifecycleState::Recovering
        } else {
            LifecycleState::Starting
        };
        self.start_readiness(generation);
    }

    fn start_readiness(&self, generation: u64) {
        let endpoint = self.setup.spec.endpoint.clone();
        let probe = Arc::clone(&self.setup.probe);
        let timings = self.setup.timings;
        let messages = self.messages.clone();
        tokio::spawn(async move {
            let available = readiness_loop(&*probe, &endpoint, timings).await;
            let _ = messages.send(Message::Ready {
                generation,
                available,
            });
        });
    }

    fn ready(&mut self, generation: u64, available: bool) {
        if generation != self.generation
            || !matches!(
                self.state,
                LifecycleState::Starting | LifecycleState::Recovering
            )
        {
            return;
        }
        if !available {
            self.fail_pending(&SearchSupervisorError::ReadinessTimeout);
            self.state = LifecycleState::Failed;
            self.stop_or_cleanup_after_failure();
            return;
        }
        self.state = LifecycleState::Ready;
        self.respond_all_ready();
    }

    fn child_exited(&mut self, generation: u64, stopped: bool, failed: bool) {
        if self
            .process
            .as_ref()
            .is_none_or(|process| process.generation != generation)
        {
            return;
        }
        self.process = None;
        if matches!(self.state, LifecycleState::Stopping) || stopped {
            remove_owned_record(&self.setup.spec);
            self.cleanup_ownership();
            self.state = LifecycleState::Idle;
            self.recovery_used = false;
            return;
        }
        if self.interests > 0 && !self.recovery_used {
            self.recovery_used = true;
            self.state = LifecycleState::Recovering;
            self.schedule_recovery();
        } else {
            let _ = failed;
            self.fail_pending(&SearchSupervisorError::ProcessUnavailable);
            self.state = LifecycleState::Failed;
            self.cleanup_ownership();
        }
    }

    fn schedule_recovery(&self) {
        let delay = self.setup.timings.recovery_backoff;
        let messages = self.messages.clone();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = messages.send(Message::StartRecovery);
        });
    }

    async fn start_recovery(&mut self) {
        if !matches!(self.state, LifecycleState::Recovering) {
            return;
        }
        if self.interests == 0 {
            self.cleanup_ownership();
            self.state = LifecycleState::Idle;
            self.recovery_used = false;
            return;
        }
        remove_owned_record(&self.setup.spec);
        self.start_generation().await;
    }

    fn release(&mut self) {
        self.interests = self.interests.saturating_sub(1);
        if self.interests != 0 {
            return;
        }
        if self.process.is_none() {
            self.cleanup_ownership();
            self.state = LifecycleState::Idle;
            self.recovery_used = false;
        } else if self.process.is_some() && self.ownership.is_some() {
            self.schedule_grace();
        }
    }

    fn schedule_grace(&mut self) {
        let generation = self.generation;
        self.grace_generation = Some(generation);
        let delay = self.setup.timings.shutdown_grace;
        let messages = self.messages.clone();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = messages.send(Message::GraceExpired { generation });
        });
    }

    fn grace_expired(&mut self, generation: u64) {
        if self.grace_generation != Some(generation) || self.interests != 0 {
            return;
        }
        self.grace_generation = None;
        if self.process.is_none() {
            self.cleanup_ownership();
            self.state = LifecycleState::Idle;
            self.recovery_used = false;
            return;
        }
        match active_interest_markers(&self.setup.spec.configuration_directory) {
            Ok(true) | Err(_) => {
                self.grace_saw_active_lease = true;
                let delay = self.setup.timings.grace_recheck;
                let messages = self.messages.clone();
                self.grace_generation = Some(generation);
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    let _ = messages.send(Message::GraceExpired { generation });
                });
            }
            Ok(false) => {
                if self.grace_saw_active_lease {
                    self.grace_saw_active_lease = false;
                    self.schedule_grace();
                    return;
                }
                if let Some(process) = &self.process {
                    let _ = process.commands.send(ChildCommand::Stop);
                    self.state = LifecycleState::Stopping;
                }
            }
        }
    }

    fn respond_ready(
        &mut self,
        response: tokio::sync::oneshot::Sender<Result<Option<SearchLease>, SearchSupervisorError>>,
    ) {
        match create_lease_marker(&self.setup.spec.configuration_directory) {
            Ok(marker) => {
                let Some(supervisor) = self.inner.upgrade() else {
                    self.interests = self.interests.saturating_sub(1);
                    let _ = response.send(Err(SearchSupervisorError::SupervisorClosed));
                    return;
                };
                let lease = SearchLease {
                    endpoint: self.setup.spec.endpoint.clone(),
                    marker,
                    supervisor,
                };
                let _ = response.send(Ok(Some(lease)));
            }
            Err(error) => {
                self.interests = self.interests.saturating_sub(1);
                let _ = response.send(Err(error));
            }
        }
    }

    fn respond_all_ready(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        for pending in pending {
            self.respond_ready(pending.response);
        }
    }

    fn fail_pending(&mut self, error: &SearchSupervisorError) {
        let pending = std::mem::take(&mut self.pending);
        self.interests = self.interests.saturating_sub(pending.len());
        for pending in pending {
            let _ = pending.response.send(Err(advisory_error(error)));
        }
    }

    fn stop_or_cleanup_after_failure(&mut self) {
        if self.process.is_some() && self.ownership.is_some() && self.interests == 0 {
            self.schedule_grace();
        } else if self.process.is_none() {
            self.cleanup_ownership();
        }
    }

    fn cleanup_ownership(&mut self) {
        if self.ownership.as_ref().is_some_and(Ownership::is_owner) {
            remove_owned_record(&self.setup.spec);
        }
        self.ownership = None;
    }

    fn finish_without_leases(&mut self) {
        if self.interests == 0 {
            if let Some(process) = &self.process {
                let _ = process.commands.send(ChildCommand::Stop);
            }
            self.cleanup_ownership();
        }
    }

    fn owner_dropped(&mut self) {
        self.channel_open = false;
        self.fail_pending(&SearchSupervisorError::SupervisorClosed);
        self.interests = 0;
        self.grace_generation = None;
        self.grace_saw_active_lease = false;
        if let Some(process) = &self.process {
            if self.ownership.is_some() {
                self.schedule_grace();
            } else {
                let _ = process.commands.send(ChildCommand::Stop);
                self.state = LifecycleState::Stopping;
            }
        } else {
            self.cleanup_ownership();
        }
    }
}

fn advisory_error(error: &SearchSupervisorError) -> SearchSupervisorError {
    match error {
        SearchSupervisorError::InvalidConfiguration(message) => {
            SearchSupervisorError::InvalidConfiguration(message)
        }
        SearchSupervisorError::ReadinessClient => SearchSupervisorError::ReadinessClient,
        SearchSupervisorError::Filesystem {
            operation, path, ..
        } => SearchSupervisorError::Filesystem {
            operation,
            path: path.clone(),
            source: io::Error::other("filesystem operation failed"),
        },
        SearchSupervisorError::ForeignRecord => SearchSupervisorError::ForeignRecord,
        SearchSupervisorError::InvalidRecord => SearchSupervisorError::InvalidRecord,
        SearchSupervisorError::RecordConflict => SearchSupervisorError::RecordConflict,
        SearchSupervisorError::CoordinationTimeout => SearchSupervisorError::CoordinationTimeout,
        SearchSupervisorError::Spawn(_) => {
            SearchSupervisorError::Spawn(io::Error::other("SearXNG process could not start"))
        }
        SearchSupervisorError::ReadinessTimeout => SearchSupervisorError::ReadinessTimeout,
        SearchSupervisorError::ProcessUnavailable => SearchSupervisorError::ProcessUnavailable,
        SearchSupervisorError::SupervisorClosed => SearchSupervisorError::SupervisorClosed,
        SearchSupervisorError::Installation(_) => SearchSupervisorError::Installation(
            crate::searxng::SearxngInstallError::InvalidMetadata("installation"),
        ),
        SearchSupervisorError::InstallationPlatformMismatch => {
            SearchSupervisorError::InstallationPlatformMismatch
        }
    }
}

async fn monitor_child(
    mut child: Box<dyn SearchChild>,
    generation: u64,
    mut commands: mpsc::UnboundedReceiver<ChildCommand>,
    messages: mpsc::UnboundedSender<Message>,
) {
    tokio::select! {
        result = child.wait() => {
            let _ = messages.send(Message::ChildExited {
                generation,
                stopped: false,
                failed: result.is_err(),
            });
        }
        command = commands.recv() => {
            if matches!(command, Some(ChildCommand::Stop)) {
                let failed = child.kill().await.is_err();
                let _ = child.wait().await;
                let _ = messages.send(Message::ChildExited {
                    generation,
                    stopped: true,
                    failed,
                });
            }
        }
    }
}

async fn readiness_loop(
    probe: &dyn ReadinessProbe,
    endpoint: &SearxngConfig,
    timings: SearchSupervisorTimings,
) -> bool {
    let deadline = Instant::now() + timings.readiness_timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        if tokio::time::timeout(remaining, probe.check(endpoint))
            .await
            .is_ok_and(|available| available)
        {
            return true;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        tokio::time::sleep(timings.readiness_retry.min(remaining)).await;
    }
}

enum Coordinated {
    Owner(CoordinationLock),
    Discovered(ProcessRecord),
}

async fn coordinate(
    spec: &LocalSearxngSpec,
    timings: SearchSupervisorTimings,
) -> Result<Coordinated, SearchSupervisorError> {
    create_private_dir_all(&spec.configuration_directory).map_err(|source| {
        filesystem_error(
            "create private coordination directory",
            spec.configuration_directory.clone(),
            source,
        )
    })?;
    restrict_path(&spec.configuration_directory, PrivatePathKind::Directory).map_err(|source| {
        filesystem_error(
            "harden coordination directory",
            spec.configuration_directory.clone(),
            source,
        )
    })?;
    let deadline = Instant::now() + timings.coordination_timeout;
    loop {
        match try_acquire_lock(&spec.lock_path())? {
            LockAttempt::Acquired(lock) => {
                if let Some(record) = read_record(spec)? {
                    remove_record_file(spec)?;
                    let _ = record;
                }
                return Ok(Coordinated::Owner(lock));
            }
            LockAttempt::Busy => {
                if let Some(record) = read_record(spec)? {
                    return Ok(Coordinated::Discovered(record));
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(SearchSupervisorError::CoordinationTimeout);
                }
                tokio::time::sleep(Duration::from_millis(10).min(remaining)).await;
            }
        }
    }
}

enum LockAttempt {
    Acquired(CoordinationLock),
    Busy,
}

struct CoordinationLock {
    file: File,
}

impl Drop for CoordinationLock {
    fn drop(&mut self) {
        let _ = File::unlock(&self.file);
    }
}

fn try_acquire_lock(path: &Path) -> Result<LockAttempt, SearchSupervisorError> {
    let file = match open_private_new(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(path).map_err(|source| {
                filesystem_error("inspect coordination lock", path.to_path_buf(), source)
            })?;
            if !metadata.file_type().is_file() {
                return Err(SearchSupervisorError::ForeignRecord);
            }
            open_private_read_write(path).map_err(|source| {
                filesystem_error("open coordination lock", path.to_path_buf(), source)
            })?
        }
        Err(source) => {
            return Err(filesystem_error(
                "create coordination lock",
                path.to_path_buf(),
                source,
            ));
        }
    };
    match file.try_lock() {
        Ok(()) => Ok(LockAttempt::Acquired(CoordinationLock { file })),
        Err(TryLockError::WouldBlock) => Ok(LockAttempt::Busy),
        Err(TryLockError::Error(source)) => Err(filesystem_error(
            "lock coordination state",
            path.to_path_buf(),
            source,
        )),
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProcessRecord {
    schema_version: u8,
    owner: String,
    endpoint: String,
    pid: Option<u32>,
}

fn read_record(spec: &LocalSearxngSpec) -> Result<Option<ProcessRecord>, SearchSupervisorError> {
    let path = spec.record_path();
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(filesystem_error(
                "inspect coordination record",
                path,
                source,
            ));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(SearchSupervisorError::ForeignRecord);
    }
    let (mut file, _) = match open_private_read(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(filesystem_error("read coordination record", path, source));
        }
    };
    if file
        .metadata()
        .map_err(|source| filesystem_error("inspect coordination record", path.clone(), source))?
        .len()
        > MAX_RECORD_BYTES
    {
        return Err(SearchSupervisorError::InvalidRecord);
    }
    let mut payload = Vec::new();
    file.read_to_end(&mut payload)
        .map_err(|source| filesystem_error("read coordination record", path, source))?;
    let record: ProcessRecord =
        serde_json::from_slice(&payload).map_err(|_| SearchSupervisorError::InvalidRecord)?;
    if record.schema_version != RECORD_SCHEMA_VERSION {
        return Err(SearchSupervisorError::InvalidRecord);
    }
    if record.owner != RECORD_OWNER {
        return Err(SearchSupervisorError::ForeignRecord);
    }
    let endpoint =
        SearxngConfig::local(&record.endpoint).map_err(|_| SearchSupervisorError::InvalidRecord)?;
    if endpoint.base_url_string() != record.endpoint {
        return Err(SearchSupervisorError::InvalidRecord);
    }
    Ok(Some(record))
}

fn write_record(spec: &LocalSearxngSpec, pid: Option<u32>) -> Result<(), SearchSupervisorError> {
    let path = spec.record_path();
    let parent = path
        .parent()
        .ok_or(SearchSupervisorError::InvalidConfiguration(
            "coordination record has no parent",
        ))?;
    let payload = serde_json::to_vec_pretty(&ProcessRecord {
        schema_version: RECORD_SCHEMA_VERSION,
        owner: RECORD_OWNER.to_owned(),
        endpoint: spec.endpoint.base_url_string(),
        pid,
    })
    .map_err(|_| SearchSupervisorError::InvalidRecord)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".nan-harness-searxng-record-")
        .tempfile_in(parent)
        .map_err(|source| {
            filesystem_error("create coordination record", parent.to_path_buf(), source)
        })?;
    let temporary_path = temporary.path().to_path_buf();
    // `tempfile` owns the handle, while `restrict_path` can harden its named file on Windows
    // without requiring the default temporary handle to request `WRITE_DAC`.
    restrict_path(&temporary_path, PrivatePathKind::File).map_err(|source| {
        filesystem_error("harden coordination record", temporary_path.clone(), source)
    })?;
    temporary
        .write_all(&payload)
        .and_then(|()| temporary.write_all(b"\n"))
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| filesystem_error("write coordination record", path.clone(), source))?;
    temporary.persist(&path).map_err(|error| {
        filesystem_error("publish coordination record", path.clone(), error.error)
    })?;
    restrict_path(&path, PrivatePathKind::File)
        .map_err(|source| filesystem_error("harden coordination record", path, source))
}

fn remove_record_file(spec: &LocalSearxngSpec) -> Result<(), SearchSupervisorError> {
    match fs::remove_file(spec.record_path()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(source) => Err(filesystem_error(
            "remove stale coordination record",
            spec.record_path(),
            source,
        )),
    }
}

fn remove_owned_record(spec: &LocalSearxngSpec) {
    let _ = remove_record_file(spec);
}

struct LeaseMarker {
    path: PathBuf,
    file: Option<File>,
}

impl LeaseMarker {
    fn release(&mut self) {
        if let Some(file) = self.file.take() {
            ACTIVE_LEASES
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&self.path);
            let _ = File::unlock(&file);
            drop(file);
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl Drop for LeaseMarker {
    fn drop(&mut self) {
        self.release();
    }
}

fn create_lease_marker(directory: &Path) -> Result<LeaseMarker, SearchSupervisorError> {
    for _ in 0..MAX_LEASE_ID_ATTEMPTS {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| SearchSupervisorError::InvalidRecord)?;
        let mut id = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            let _ = write!(&mut id, "{byte:02x}");
        }
        let path = directory.join(format!("{LEASE_FILE_PREFIX}{id}"));
        let mut file = match open_private_new(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(filesystem_error("create interest lease", path, source));
            }
        };
        if let Err(source) = file
            .write_all(LEASE_MARKER)
            .and_then(|()| file.sync_data())
            .and_then(|()| file.lock())
        {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(filesystem_error("publish interest lease", path, source));
        }
        ACTIVE_LEASES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(path.clone());
        return Ok(LeaseMarker {
            path,
            file: Some(file),
        });
    }
    Err(SearchSupervisorError::InvalidRecord)
}

/// Counts currently locked, valid interest markers without starting a local backend.
///
/// The directory scan is deliberately bounded so status and uninstall cannot be held hostage by
/// an attacker-controlled number of marker files. Invalid unlocked markers are ignored; a locked
/// marker is counted only when it carries the supervisor's exact marker bytes.
///
/// # Errors
///
/// Returns a filesystem error when the bounded marker scan cannot be completed, or an invalid
/// record error when the directory exceeds the marker scan bound.
pub fn active_search_interests(directory: &Path) -> Result<usize, SearchSupervisorError> {
    let owned_markers: std::collections::BTreeSet<PathBuf> = ACTIVE_LEASES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .filter(|path| path.parent() == Some(directory))
        .cloned()
        .collect();
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(owned_markers.len()),
        Err(source) => {
            return Err(filesystem_error(
                "inspect interest leases",
                directory.to_path_buf(),
                source,
            ));
        }
    };
    let mut inspected = 0;
    let mut active = owned_markers.len();
    for entry in entries {
        inspected += 1;
        if inspected > MAX_INTEREST_MARKERS {
            return Err(SearchSupervisorError::InvalidRecord);
        }
        let entry = entry.map_err(|source| {
            filesystem_error("inspect interest leases", directory.to_path_buf(), source)
        })?;
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with(LEASE_FILE_PREFIX) {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|source| filesystem_error("inspect interest lease", path.clone(), source))?;
        if !metadata.file_type().is_file() {
            continue;
        }
        let file = open_private_read_write(&path)
            .map_err(|source| filesystem_error("open interest lease", path.clone(), source))?;
        match file.try_lock() {
            Err(TryLockError::WouldBlock) => {
                let mut file = file;
                let mut marker = Vec::new();
                let valid = file
                    .seek(io::SeekFrom::Start(0))
                    .and_then(|_| file.read_to_end(&mut marker))
                    .is_ok_and(|_| marker == LEASE_MARKER);
                if valid && !owned_markers.contains(&path) {
                    active += 1;
                }
            }
            Err(TryLockError::Error(source)) => {
                return Err(filesystem_error("lock interest lease", path, source));
            }
            Ok(()) => {
                let mut file = file;
                let mut marker = Vec::new();
                let valid = file
                    .seek(io::SeekFrom::Start(0))
                    .and_then(|_| file.read_to_end(&mut marker))
                    .is_ok_and(|_| marker == LEASE_MARKER);
                let _ = File::unlock(&file);
                drop(file);
                if valid {
                    let _ = fs::remove_file(path);
                }
            }
        }
    }
    Ok(active)
}

fn active_interest_markers(directory: &Path) -> Result<bool, SearchSupervisorError> {
    Ok(active_search_interests(directory)? != 0)
}

fn filesystem_error(
    operation: &'static str,
    path: PathBuf,
    source: io::Error,
) -> SearchSupervisorError {
    SearchSupervisorError::Filesystem {
        operation,
        path,
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::process::{ExitStatus, Stdio};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tokio::sync::Notify;

    fn command() -> SearxngCommand {
        SearxngCommand {
            program: PathBuf::from("unused-test-command"),
            arguments: Vec::new(),
            current_directory: PathBuf::from("."),
            settings_path: None,
        }
    }

    fn timings() -> SearchSupervisorTimings {
        SearchSupervisorTimings {
            readiness_timeout: Duration::from_millis(100),
            readiness_retry: Duration::from_millis(5),
            recovery_backoff: Duration::from_millis(10),
            shutdown_grace: Duration::from_millis(35),
            grace_recheck: Duration::from_millis(5),
            coordination_timeout: Duration::from_millis(100),
        }
    }

    struct FakeFactory {
        children: Mutex<Vec<Arc<FakeChildControl>>>,
        spawns: AtomicUsize,
    }

    impl FakeFactory {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                children: Mutex::new(Vec::new()),
                spawns: AtomicUsize::new(0),
            })
        }

        fn child(&self, index: usize) -> Arc<FakeChildControl> {
            self.children.lock().expect("children lock")[index].clone()
        }
    }

    impl ProcessFactory for FakeFactory {
        fn spawn(&self, _command: &SearxngCommand) -> io::Result<Box<dyn SearchChild>> {
            let control = Arc::new(FakeChildControl::new());
            self.children
                .lock()
                .expect("children lock")
                .push(control.clone());
            self.spawns.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(FakeChild { control }))
        }
    }

    struct FakeChildControl {
        exited: Notify,
        killed: Notify,
        has_exited: AtomicBool,
        kill_count: AtomicUsize,
    }

    impl FakeChildControl {
        fn new() -> Self {
            Self {
                exited: Notify::new(),
                killed: Notify::new(),
                has_exited: AtomicBool::new(false),
                kill_count: AtomicUsize::new(0),
            }
        }

        fn exit(&self) {
            self.has_exited.store(true, Ordering::SeqCst);
            self.exited.notify_waiters();
        }
    }

    struct FakeChild {
        control: Arc<FakeChildControl>,
    }

    impl SearchChild for FakeChild {
        fn id(&self) -> Option<u32> {
            Some(321)
        }

        fn kill(&mut self) -> BoxFuture<'_, io::Result<()>> {
            let control = Arc::clone(&self.control);
            Box::pin(async move {
                control.kill_count.fetch_add(1, Ordering::SeqCst);
                control.killed.notify_waiters();
                control.has_exited.store(true, Ordering::SeqCst);
                control.exited.notify_waiters();
                Ok(())
            })
        }

        fn wait(&mut self) -> BoxFuture<'_, io::Result<ExitStatus>> {
            let control = Arc::clone(&self.control);
            Box::pin(async move {
                while !control.has_exited.load(Ordering::SeqCst) {
                    control.exited.notified().await;
                }
                Ok(exit_status())
            })
        }
    }

    fn exit_status() -> ExitStatus {
        std::process::Command::new(if cfg!(windows) { "cmd" } else { "true" })
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("test status should be available")
    }

    struct FakeProbe {
        outcomes: Mutex<VecDeque<bool>>,
    }

    impl FakeProbe {
        fn always(available: bool) -> Arc<Self> {
            Arc::new(Self {
                outcomes: Mutex::new(VecDeque::from([available])),
            })
        }

        fn sequence(outcomes: impl IntoIterator<Item = bool>) -> Arc<Self> {
            Arc::new(Self {
                outcomes: Mutex::new(outcomes.into_iter().collect()),
            })
        }
    }

    impl ReadinessProbe for FakeProbe {
        fn check<'a>(&'a self, _endpoint: &'a SearxngConfig) -> BoxFuture<'a, bool> {
            Box::pin(async move {
                self.outcomes
                    .lock()
                    .expect("probe lock")
                    .pop_front()
                    .unwrap_or(false)
            })
        }
    }

    fn supervisor(
        directory: &Path,
        factory: Arc<FakeFactory>,
        probe: Arc<FakeProbe>,
    ) -> SearchSupervisor {
        let endpoint = SearxngConfig::local("http://127.0.0.1:8888").expect("endpoint");
        let spec = LocalSearxngSpec::new(directory, endpoint, command()).expect("spec");
        let setup = ActorSetup {
            spec: Arc::new(spec),
            timings: timings(),
            factory,
            probe,
        };
        SearchSupervisor {
            inner: Arc::new(SupervisorInner::new(Some(setup))),
            host_executable: None,
        }
    }

    #[tokio::test]
    async fn concurrent_first_interests_start_one_instance_and_reuse_it() {
        let root = tempfile::tempdir().expect("temporary directory");
        let factory = FakeFactory::new();
        let supervisor = supervisor(root.path(), factory.clone(), FakeProbe::always(true));
        let (left, right) = tokio::join!(supervisor.acquire(), supervisor.acquire());
        let left = left.expect("left acquire").expect("left lease");
        let right = right.expect("right acquire").expect("right lease");
        assert_eq!(left.endpoint(), right.endpoint());
        assert_eq!(factory.spawns.load(Ordering::SeqCst), 1);
        drop(left);
        drop(right);
    }

    #[test]
    fn standalone_derivation_is_a_no_op_without_an_owned_installation() {
        let home = tempfile::tempdir().expect("temporary home");
        let local = SearxngConfig::local("http://127.0.0.1:8888").expect("endpoint");
        assert!(
            SearchSupervisor::from_standalone_install(&local, Some(home.path()))
                .expect("missing installation should be advisory")
                .is_none()
        );
    }

    #[test]
    fn standalone_derivation_ignores_remote_and_docker_endpoints() {
        let home = tempfile::tempdir().expect("temporary home");
        for endpoint in [
            SearxngConfig::remote("https://search.example.test").expect("remote endpoint"),
            SearxngConfig::docker("http://searxng:8080").expect("docker endpoint"),
        ] {
            assert!(
                SearchSupervisor::from_standalone_install(&endpoint, Some(home.path()))
                    .expect("non-local endpoint should be a no-op")
                    .is_none()
            );
        }
    }

    #[tokio::test]
    async fn startup_waits_for_readiness_within_the_bound() {
        let root = tempfile::tempdir().expect("temporary directory");
        let supervisor = supervisor(
            root.path(),
            FakeFactory::new(),
            FakeProbe::sequence([false, false, true]),
        );
        let started = Instant::now();
        let lease = supervisor
            .acquire()
            .await
            .expect("startup should succeed")
            .expect("lease should exist");
        assert!(started.elapsed() >= Duration::from_millis(8));
        drop(lease);
    }

    #[tokio::test]
    async fn crash_gets_one_backed_off_recovery_attempt() {
        let root = tempfile::tempdir().expect("temporary directory");
        let factory = FakeFactory::new();
        let supervisor = supervisor(root.path(), factory.clone(), FakeProbe::always(true));
        let lease = supervisor.acquire().await.expect("startup").expect("lease");
        factory.child(0).exit();
        tokio::time::timeout(Duration::from_millis(150), async {
            while factory.spawns.load(Ordering::SeqCst) < 2 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("recovery should be attempted");
        assert_eq!(factory.spawns.load(Ordering::SeqCst), 2);
        drop(lease);
    }

    #[tokio::test]
    async fn stale_owned_record_is_replaced_after_lock_recovery() {
        let root = tempfile::tempdir().expect("temporary directory");
        let endpoint = SearxngConfig::local("http://127.0.0.1:8888").expect("endpoint");
        let spec = LocalSearxngSpec::new(root.path(), endpoint, command()).expect("spec");
        create_private_dir_all(root.path()).expect("coordination directory");
        write_record(&spec, Some(99)).expect("stale record should write");

        let factory = FakeFactory::new();
        let supervisor = supervisor(root.path(), factory.clone(), FakeProbe::always(true));
        let lease = supervisor
            .acquire()
            .await
            .expect("stale state should recover")
            .expect("lease should exist");
        assert_eq!(factory.spawns.load(Ordering::SeqCst), 1);
        let record = read_record(&spec)
            .expect("record should read")
            .expect("record should exist");
        assert_eq!(record.pid, Some(321));
        drop(lease);
    }

    #[tokio::test]
    async fn discovered_interest_prevents_owner_shutdown_until_its_grace() {
        let root = tempfile::tempdir().expect("temporary directory");
        let owner_factory = FakeFactory::new();
        let owner = supervisor(root.path(), owner_factory.clone(), FakeProbe::always(true));
        let owner_lease = owner
            .acquire()
            .await
            .expect("owner startup")
            .expect("lease");

        let discovered_factory = FakeFactory::new();
        let discovered = supervisor(
            root.path(),
            discovered_factory.clone(),
            FakeProbe::always(true),
        );
        let discovered_lease = discovered
            .acquire()
            .await
            .expect("discovery startup")
            .expect("discovered lease");
        assert_eq!(discovered_factory.spawns.load(Ordering::SeqCst), 0);
        let child = owner_factory.child(0);
        drop(owner_lease);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(child.kill_count.load(Ordering::SeqCst), 0);
        drop(discovered_lease);
        tokio::time::sleep(Duration::from_millis(15)).await;
        assert_eq!(child.kill_count.load(Ordering::SeqCst), 0);
        tokio::time::sleep(Duration::from_millis(45)).await;
        assert_eq!(child.kill_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn grace_period_does_not_stop_an_interested_process() {
        let root = tempfile::tempdir().expect("temporary directory");
        let factory = FakeFactory::new();
        let supervisor = supervisor(root.path(), factory.clone(), FakeProbe::always(true));
        let first = supervisor.acquire().await.expect("startup").expect("lease");
        let second = supervisor.acquire().await.expect("reuse").expect("lease");
        let child = factory.child(0);
        drop(first);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(child.kill_count.load(Ordering::SeqCst), 0);
        drop(second);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(child.kill_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn abrupt_lease_closure_releases_marker_and_allows_recovery() {
        let root = tempfile::tempdir().expect("temporary directory");
        let supervisor = supervisor(root.path(), FakeFactory::new(), FakeProbe::always(true));
        let lease = supervisor.acquire().await.expect("startup").expect("lease");
        let marker_count = fs::read_dir(root.path())
            .expect("directory")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(LEASE_FILE_PREFIX)
            })
            .count();
        assert_eq!(marker_count, 1);
        drop(lease);
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert!(!active_interest_markers(root.path()).expect("marker scan"));
    }

    #[test]
    fn active_interest_count_ignores_invalid_markers() {
        let root = tempfile::tempdir().expect("temporary directory");
        let marker = create_lease_marker(root.path()).expect("lease marker");
        fs::write(
            root.path().join(format!("{LEASE_FILE_PREFIX}invalid")),
            b"foreign",
        )
        .expect("invalid marker");

        assert_eq!(
            active_search_interests(root.path()).expect("interest count"),
            1
        );
        drop(marker);
        assert_eq!(
            active_search_interests(root.path()).expect("interest count"),
            0
        );
    }

    #[test]
    fn independent_search_interest_is_counted_until_drop() {
        let root = tempfile::tempdir().expect("temporary directory");
        let interest = SearchInterest::acquire(root.path()).expect("interest should be created");

        assert_eq!(
            active_search_interests(root.path()).expect("interest count"),
            1
        );
        drop(interest);
        assert_eq!(
            active_search_interests(root.path()).expect("interest count"),
            0
        );
    }

    #[tokio::test]
    async fn supervisor_drop_keeps_a_lease_alive_and_then_honors_grace() {
        let root = tempfile::tempdir().expect("temporary directory");
        let factory = FakeFactory::new();
        let supervisor = supervisor(root.path(), factory.clone(), FakeProbe::always(true));
        let lease = supervisor.acquire().await.expect("startup").expect("lease");
        let child = factory.child(0);
        drop(supervisor);
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(child.kill_count.load(Ordering::SeqCst), 0);
        drop(lease);
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(child.kill_count.load(Ordering::SeqCst), 0);
        tokio::time::sleep(Duration::from_millis(25)).await;
        assert_eq!(child.kill_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unconfigured_supervision_is_a_no_op() {
        let root = tempfile::tempdir().expect("temporary directory");
        let supervisor = SearchSupervisor::new(None).expect("no-op supervisor");
        assert!(supervisor.acquire().await.expect("no-op acquire").is_none());
        assert_eq!(fs::read_dir(root.path()).expect("directory").count(), 0);
    }
}
