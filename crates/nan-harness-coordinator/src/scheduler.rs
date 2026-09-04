use crate::protocol::AttemptOutcome;
use nan_harness_private_fs::{open_private_read, open_private_truncate};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot};

const INITIAL_WINDOW: usize = 2;
const TICK: Duration = Duration::from_millis(25);
const CACHE_TTL: Duration = Duration::from_hours(1);
const CACHE_WRITE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub(crate) struct AcquireRequest {
    pub(crate) scope: String,
    pub(crate) launch_id: String,
    pub(crate) enqueued_at: Instant,
}

#[derive(Debug)]
pub(crate) struct Grant {
    pub(crate) lease_id: u64,
    pub(crate) queued: Duration,
}

#[derive(Clone)]
pub(crate) struct Scheduler {
    commands: mpsc::UnboundedSender<Command>,
}

enum Command {
    Acquire {
        request: AcquireRequest,
        reply: oneshot::Sender<Grant>,
    },
    Observe {
        scope: String,
        outcome: AttemptOutcome,
        retry_after: Option<Duration>,
        reply: oneshot::Sender<Duration>,
    },
    Release {
        scope: String,
    },
}

struct Pending {
    request: AcquireRequest,
    reply: oneshot::Sender<Grant>,
}

struct ScopeState {
    active: usize,
    window: usize,
    successful_round: usize,
    transient_failures: u8,
    rate_limit_streak: u8,
    cooldown_until: Option<Instant>,
    last_launch: Option<String>,
    pending: VecDeque<Pending>,
    updated_at_unix_seconds: u64,
}

impl Default for ScopeState {
    fn default() -> Self {
        Self {
            active: 0,
            window: INITIAL_WINDOW,
            successful_round: 0,
            transient_failures: 0,
            rate_limit_streak: 0,
            cooldown_until: None,
            last_launch: None,
            pending: VecDeque::new(),
            updated_at_unix_seconds: now_seconds(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Cache {
    schema_version: u8,
    scopes: HashMap<String, CachedScope>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct CachedScope {
    window: usize,
    updated_at_unix_seconds: u64,
}

impl Scheduler {
    pub(crate) fn start(cache_path: PathBuf) -> Self {
        let (commands, receiver) = mpsc::unbounded_channel();
        tokio::spawn(run(receiver, cache_path));
        Self { commands }
    }

    pub(crate) async fn acquire(&self, request: AcquireRequest) -> Option<Grant> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::Acquire { request, reply })
            .ok()?;
        response.await.ok()
    }

    pub(crate) async fn observe(
        &self,
        scope: String,
        outcome: AttemptOutcome,
        retry_after: Option<Duration>,
    ) -> Option<Duration> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::Observe {
                scope,
                outcome,
                retry_after,
                reply,
            })
            .ok()?;
        response.await.ok()
    }

    pub(crate) fn release(&self, scope: String) {
        let _ = self.commands.send(Command::Release { scope });
    }
}

async fn run(mut receiver: mpsc::UnboundedReceiver<Command>, cache_path: PathBuf) {
    let mut scopes = load_cache(&cache_path);
    let mut next_lease_id = 1_u64;
    let mut tick = tokio::time::interval(TICK);
    let mut dirty = false;
    let mut last_cache_write = Instant::now();
    loop {
        tokio::select! {
            command = receiver.recv() => {
                let Some(command) = command else {
                    if dirty { let _ = save_cache(&cache_path, &scopes); }
                    return;
                };
                dirty |= handle_command(command, &mut scopes);
            }
            _ = tick.tick() => {}
        }
        schedule(&mut scopes, &mut next_lease_id);
        if dirty && last_cache_write.elapsed() >= CACHE_WRITE_INTERVAL {
            if save_cache(&cache_path, &scopes).is_ok() {
                dirty = false;
            }
            last_cache_write = Instant::now();
        }
    }
}

fn handle_command(command: Command, scopes: &mut HashMap<String, ScopeState>) -> bool {
    match command {
        Command::Acquire { request, reply } => {
            let state = scopes.entry(request.scope.clone()).or_default();
            state.pending.push_back(Pending { request, reply });
            false
        }
        Command::Observe {
            scope,
            outcome,
            retry_after,
            reply,
        } => {
            let state = scopes.entry(scope).or_default();
            let delay = observe(state, outcome, retry_after);
            let _ = reply.send(delay);
            true
        }
        Command::Release { scope } => {
            let state = scopes.entry(scope).or_default();
            state.active = state.active.saturating_sub(1);
            false
        }
    }
}

fn schedule(scopes: &mut HashMap<String, ScopeState>, next_lease_id: &mut u64) {
    let now = Instant::now();
    for state in scopes.values_mut() {
        if state.cooldown_until.is_some_and(|deadline| deadline > now) {
            continue;
        }
        state.cooldown_until = None;
        while state.active < state.window {
            let Some(pending) = take_fair(state) else {
                break;
            };
            if pending.reply.is_closed() {
                continue;
            }
            let grant = Grant {
                lease_id: *next_lease_id,
                queued: pending.request.enqueued_at.elapsed(),
            };
            *next_lease_id = next_lease_id.wrapping_add(1).max(1);
            state.active += 1;
            state.last_launch = Some(pending.request.launch_id);
            let _ = pending.reply.send(grant);
        }
    }
}

fn take_fair(state: &mut ScopeState) -> Option<Pending> {
    let different_launch = state.last_launch.as_ref().and_then(|last| {
        state
            .pending
            .iter()
            .position(|pending| pending.request.launch_id != *last)
    });
    state.pending.remove(different_launch.unwrap_or(0))
}

fn observe(
    state: &mut ScopeState,
    outcome: AttemptOutcome,
    retry_after: Option<Duration>,
) -> Duration {
    match outcome {
        AttemptOutcome::Success => observe_success(state),
        AttemptOutcome::RateLimited => observe_rate_limit(state, retry_after),
        AttemptOutcome::Transport
        | AttemptOutcome::Timeout
        | AttemptOutcome::ServerError
        | AttemptOutcome::InvalidResponse => observe_transient_failure(state),
        AttemptOutcome::Cancelled | AttemptOutcome::Terminal => Duration::ZERO,
    }
}

fn observe_success(state: &mut ScopeState) -> Duration {
    state.updated_at_unix_seconds = now_seconds();
    state.transient_failures = 0;
    state.rate_limit_streak = 0;
    state.successful_round = state.successful_round.saturating_add(1);
    if state.successful_round >= state.window {
        state.window = state.window.saturating_add(1);
        state.successful_round = 0;
    }
    Duration::ZERO
}

fn observe_rate_limit(state: &mut ScopeState, retry_after: Option<Duration>) -> Duration {
    state.updated_at_unix_seconds = now_seconds();
    state.window = (state.window / 2).max(1);
    state.successful_round = 0;
    state.rate_limit_streak = state.rate_limit_streak.saturating_add(1);
    let delay = retry_after.unwrap_or_else(|| rate_limit_backoff(state.rate_limit_streak));
    state.cooldown_until = Some(Instant::now() + delay);
    delay
}

fn observe_transient_failure(state: &mut ScopeState) -> Duration {
    state.updated_at_unix_seconds = now_seconds();
    state.transient_failures = state.transient_failures.saturating_add(1);
    let delay = transient_backoff(state.transient_failures);
    if state.transient_failures >= 5 {
        let breaker = 5_u64
            .saturating_mul(1_u64 << u32::from(state.transient_failures.saturating_sub(5).min(3)))
            .min(30);
        state.cooldown_until = Some(Instant::now() + Duration::from_secs(breaker));
    }
    delay
}

fn rate_limit_backoff(streak: u8) -> Duration {
    let exponent = u32::from(streak.saturating_sub(1).min(5));
    equal_jitter(
        Duration::from_millis(500_u64.saturating_mul(1_u64 << exponent))
            .min(Duration::from_secs(8)),
    )
}

fn transient_backoff(streak: u8) -> Duration {
    let exponent = u32::from(streak.saturating_sub(1).min(3));
    equal_jitter(
        Duration::from_millis(250_u64.saturating_mul(1_u64 << exponent))
            .min(Duration::from_secs(2)),
    )
}

fn equal_jitter(cap: Duration) -> Duration {
    let cap_ms = u64::try_from(cap.as_millis()).unwrap_or(u64::MAX);
    let half = cap_ms / 2;
    let mut random = [0_u8; 8];
    let value = if getrandom::fill(&mut random).is_ok() {
        u64::from_le_bytes(random)
    } else {
        0
    };
    Duration::from_millis(half + value % (cap_ms.saturating_sub(half).max(1)))
}

fn load_cache(path: &Path) -> HashMap<String, ScopeState> {
    let Ok((file, _)) = open_private_read(path) else {
        return HashMap::new();
    };
    let Ok(cache) = serde_json::from_reader::<_, Cache>(file) else {
        return HashMap::new();
    };
    if cache.schema_version != 1 {
        return HashMap::new();
    }
    cache
        .scopes
        .into_iter()
        .map(|(scope, cached)| (scope, restored_scope(cached)))
        .collect()
}

fn restored_scope(cached: CachedScope) -> ScopeState {
    let age = Duration::from_secs(now_seconds().saturating_sub(cached.updated_at_unix_seconds));
    let learned = cached.window.max(INITIAL_WINDOW);
    let window = if age >= CACHE_TTL {
        INITIAL_WINDOW
    } else {
        let remaining = CACHE_TTL.saturating_sub(age).as_secs();
        INITIAL_WINDOW.saturating_add(
            (learned - INITIAL_WINDOW)
                .saturating_mul(usize::try_from(remaining).unwrap_or(usize::MAX))
                / usize::try_from(CACHE_TTL.as_secs()).unwrap_or(usize::MAX),
        )
    };
    ScopeState {
        window,
        updated_at_unix_seconds: cached.updated_at_unix_seconds,
        ..ScopeState::default()
    }
}

fn save_cache(path: &Path, scopes: &HashMap<String, ScopeState>) -> std::io::Result<()> {
    let cache = Cache {
        schema_version: 1,
        scopes: scopes
            .iter()
            .map(|(scope, state)| {
                (
                    scope.clone(),
                    CachedScope {
                        window: state.window,
                        updated_at_unix_seconds: state.updated_at_unix_seconds,
                    },
                )
            })
            .collect(),
    };
    let payload = serde_json::to_vec(&cache).map_err(std::io::Error::other)?;
    let mut file = open_private_truncate(path)?;
    file.write_all(&payload)?;
    file.write_all(b"\n")?;
    file.sync_all()
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::{
        AcquireRequest, CachedScope, Scheduler, ScopeState, observe_rate_limit, observe_success,
        restored_scope,
    };
    use std::time::{Duration, Instant};

    fn request(launch_id: &str) -> AcquireRequest {
        AcquireRequest {
            scope: "credential".to_owned(),
            launch_id: launch_id.to_owned(),
            enqueued_at: Instant::now(),
        }
    }

    #[test]
    fn success_grows_and_rate_limit_reduces_the_window() {
        let mut state = ScopeState::default();
        assert_eq!(state.window, 2);
        observe_success(&mut state);
        observe_success(&mut state);
        assert_eq!(state.window, 3);
        let delay = observe_rate_limit(&mut state, Some(Duration::from_secs(2)));
        assert_eq!(state.window, 1);
        assert_eq!(delay, Duration::from_secs(2));
    }

    #[test]
    fn learned_capacity_decays_to_the_cold_window() {
        let state = restored_scope(CachedScope {
            window: 12,
            updated_at_unix_seconds: 0,
        });
        assert_eq!(state.window, 2);
    }

    #[tokio::test]
    async fn requests_wait_for_capacity_and_resume_after_release() {
        let temporary = tempfile::tempdir().expect("temporary directory should exist");
        let scheduler = Scheduler::start(temporary.path().join("capacity.json"));
        scheduler
            .acquire(request("codex"))
            .await
            .expect("first grant");
        scheduler
            .acquire(request("pi"))
            .await
            .expect("second grant");

        let pending_scheduler = scheduler.clone();
        let mut pending =
            tokio::spawn(async move { pending_scheduler.acquire(request("claude")).await });
        assert!(
            tokio::time::timeout(Duration::from_millis(60), &mut pending)
                .await
                .is_err()
        );

        scheduler.release("credential".to_owned());
        let grant = tokio::time::timeout(Duration::from_millis(200), pending)
            .await
            .expect("queued request should resume")
            .expect("queued task should finish")
            .expect("queued request should receive a grant");
        assert!(grant.queued >= Duration::from_millis(50));
    }

    #[tokio::test]
    async fn a_different_launch_is_selected_before_the_previous_launch() {
        let temporary = tempfile::tempdir().expect("temporary directory should exist");
        let scheduler = Scheduler::start(temporary.path().join("capacity.json"));
        scheduler
            .acquire(request("codex"))
            .await
            .expect("first grant");
        scheduler
            .acquire(request("codex"))
            .await
            .expect("second grant");

        let same_scheduler = scheduler.clone();
        let mut same = tokio::spawn(async move { same_scheduler.acquire(request("codex")).await });
        let other_scheduler = scheduler.clone();
        let other = tokio::spawn(async move { other_scheduler.acquire(request("pi")).await });
        tokio::time::sleep(Duration::from_millis(30)).await;
        scheduler.release("credential".to_owned());

        tokio::time::timeout(Duration::from_millis(200), other)
            .await
            .expect("different launch should be selected")
            .expect("different launch task should finish")
            .expect("different launch should receive a grant");
        assert!(
            tokio::time::timeout(Duration::from_millis(40), &mut same)
                .await
                .is_err()
        );
        scheduler.release("credential".to_owned());
        tokio::time::timeout(Duration::from_millis(200), same)
            .await
            .expect("same launch should eventually resume")
            .expect("same launch task should finish")
            .expect("same launch should receive a grant");
    }
}
