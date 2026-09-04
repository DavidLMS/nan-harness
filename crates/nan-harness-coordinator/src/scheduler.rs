use crate::protocol::{AttemptOutcome, RequestLane, RequestPriority};
use nan_harness_private_fs::{open_private_read, open_private_truncate};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot};

const INITIAL_WINDOW: usize = 2;
const INVALID_RESPONSE_WINDOW_FLOOR: usize = 2;
const MAX_WINDOW: usize = 10;
const TICK: Duration = Duration::from_millis(25);
const CACHE_TTL: Duration = Duration::from_hours(1);
const CACHE_WRITE_INTERVAL: Duration = Duration::from_secs(1);
const BACKGROUND_AGING: Duration = Duration::from_secs(10);
const MIN_GROWTH_INTERVAL: Duration = Duration::from_mins(2);
const HEALTHY_HEADERS: Duration = Duration::from_secs(30);
const BASE_GROWTH_HOLD: Duration = Duration::from_mins(10);
const MAX_GROWTH_HOLD: Duration = Duration::from_hours(1);

#[derive(Debug)]
pub(crate) struct AcquireRequest {
    pub(crate) scope: String,
    pub(crate) launch_id: String,
    pub(crate) lane: RequestLane,
    pub(crate) priority: RequestPriority,
    pub(crate) enqueued_at: Instant,
}

#[derive(Debug)]
pub(crate) struct Grant {
    pub(crate) lease_id: u64,
    pub(crate) queued: Duration,
    pub(crate) growth_eligible: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Observation {
    pub(crate) delay: Duration,
    pub(crate) previous_window: usize,
    pub(crate) window: usize,
    pub(crate) growth_blocked_seconds: u64,
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
        growth_eligible: bool,
        foreground_inference: bool,
        headers_elapsed: Option<Duration>,
        reply: oneshot::Sender<Observation>,
    },
    Release {
        scope: String,
        foreground_inference: bool,
    },
}

struct Pending {
    request: AcquireRequest,
    reply: oneshot::Sender<Grant>,
}

struct ScopeState {
    active: usize,
    active_foreground_inference: usize,
    window: usize,
    successful_round: usize,
    healthy_since_penalty: usize,
    penalty_level: u8,
    growth_blocked_until_unix_seconds: Option<u64>,
    transient_failures: u8,
    invalid_response_streak: u8,
    rate_limit_streak: u8,
    cooldown_until: Option<Instant>,
    last_launch: Option<String>,
    pending: VecDeque<Pending>,
    updated_at_unix_seconds: u64,
    last_growth: Option<Instant>,
}

impl Default for ScopeState {
    fn default() -> Self {
        Self {
            active: 0,
            active_foreground_inference: 0,
            window: INITIAL_WINDOW,
            successful_round: 0,
            healthy_since_penalty: 0,
            penalty_level: 0,
            growth_blocked_until_unix_seconds: None,
            transient_failures: 0,
            invalid_response_streak: 0,
            rate_limit_streak: 0,
            cooldown_until: None,
            last_launch: None,
            pending: VecDeque::new(),
            updated_at_unix_seconds: now_seconds(),
            last_growth: None,
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
    #[serde(default)]
    healthy_since_penalty: usize,
    #[serde(default)]
    penalty_level: u8,
    #[serde(default)]
    growth_blocked_until_unix_seconds: Option<u64>,
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
        growth_eligible: bool,
        foreground_inference: bool,
        headers_elapsed: Option<Duration>,
    ) -> Option<Observation> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::Observe {
                scope,
                outcome,
                retry_after,
                growth_eligible,
                foreground_inference,
                headers_elapsed,
                reply,
            })
            .ok()?;
        response.await.ok()
    }

    pub(crate) fn release(&self, scope: String, foreground_inference: bool) {
        let _ = self.commands.send(Command::Release {
            scope,
            foreground_inference,
        });
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
            growth_eligible,
            foreground_inference,
            headers_elapsed,
            reply,
        } => {
            let state = scopes.entry(scope).or_default();
            let previous_window = state.window;
            let delay = observe(
                state,
                outcome,
                retry_after,
                growth_eligible,
                foreground_inference,
                headers_elapsed,
            );
            let growth_blocked_seconds = state
                .growth_blocked_until_unix_seconds
                .map_or(0, |deadline| deadline.saturating_sub(now_seconds()));
            let _ = reply.send(Observation {
                delay,
                previous_window,
                window: state.window,
                growth_blocked_seconds,
            });
            true
        }
        Command::Release {
            scope,
            foreground_inference,
        } => {
            let state = scopes.entry(scope).or_default();
            state.active = state.active.saturating_sub(1);
            if foreground_inference {
                state.active_foreground_inference =
                    state.active_foreground_inference.saturating_sub(1);
            }
            false
        }
    }
}

fn schedule(scopes: &mut HashMap<String, ScopeState>, next_lease_id: &mut u64) {
    let now = Instant::now();
    for state in scopes.values_mut() {
        state.pending.retain(|pending| !pending.reply.is_closed());
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
            let foreground_inference = pending.request.lane == RequestLane::Inference
                && pending.request.priority == RequestPriority::Foreground;
            let queued_foreground_inference = state
                .pending
                .iter()
                .filter(|item| {
                    !item.reply.is_closed()
                        && item.request.lane == RequestLane::Inference
                        && item.request.priority == RequestPriority::Foreground
                })
                .count();
            let grant = Grant {
                lease_id: *next_lease_id,
                queued: pending.request.enqueued_at.elapsed(),
                growth_eligible: foreground_inference
                    && state.active_foreground_inference + 1 + queued_foreground_inference
                        >= state.window,
            };
            *next_lease_id = next_lease_id.wrapping_add(1).max(1);
            let launch_id = pending.request.launch_id;
            if pending.reply.send(grant).is_ok() {
                state.active += 1;
                state.active_foreground_inference += usize::from(foreground_inference);
                state.last_launch = Some(launch_id);
            }
        }
    }
}

fn take_fair(state: &mut ScopeState) -> Option<Pending> {
    let now = Instant::now();
    let has_foreground = state
        .pending
        .iter()
        .any(|pending| pending.request.priority == RequestPriority::Foreground);
    let eligible = |pending: &&Pending| {
        !has_foreground
            || pending.request.priority == RequestPriority::Foreground
            || now.duration_since(pending.request.enqueued_at) >= BACKGROUND_AGING
    };
    let preferred = state.last_launch.as_ref().and_then(|last| {
        state
            .pending
            .iter()
            .position(|pending| eligible(&pending) && pending.request.launch_id != *last)
    });
    let foreground = state.pending.iter().position(|pending| eligible(&pending));
    state.pending.remove(preferred.or(foreground).unwrap_or(0))
}

fn observe(
    state: &mut ScopeState,
    outcome: AttemptOutcome,
    retry_after: Option<Duration>,
    growth_eligible: bool,
    foreground_inference: bool,
    headers_elapsed: Option<Duration>,
) -> Duration {
    match outcome {
        AttemptOutcome::Success => observe_success(
            state,
            growth_eligible,
            foreground_inference,
            headers_elapsed,
        ),
        AttemptOutcome::RateLimited => observe_rate_limit(state, retry_after, foreground_inference),
        AttemptOutcome::Transport => {
            observe_transient_failure(state, false, foreground_inference, false)
        }
        AttemptOutcome::Timeout | AttemptOutcome::ServerError => {
            observe_transient_failure(state, true, foreground_inference, true)
        }
        AttemptOutcome::InvalidResponse => observe_invalid_response(state, foreground_inference),
        AttemptOutcome::Cancelled | AttemptOutcome::Terminal => Duration::ZERO,
    }
}

fn observe_success(
    state: &mut ScopeState,
    growth_eligible: bool,
    foreground_inference: bool,
    headers_elapsed: Option<Duration>,
) -> Duration {
    let now = now_seconds();
    state.updated_at_unix_seconds = now;
    state.transient_failures = 0;
    state.rate_limit_streak = 0;
    let healthy =
        foreground_inference && headers_elapsed.is_some_and(|elapsed| elapsed <= HEALTHY_HEADERS);
    if foreground_inference {
        state.invalid_response_streak = 0;
    }
    if healthy && state.penalty_level > 0 {
        state.healthy_since_penalty = state.healthy_since_penalty.saturating_add(1);
    }
    if growth_eligible && healthy {
        state.successful_round = state.successful_round.saturating_add(1);
    } else {
        state.successful_round = 0;
    }
    let growth_ready = state
        .last_growth
        .is_none_or(|last| last.elapsed() >= MIN_GROWTH_INTERVAL);
    let required = growth_successes(state.window);
    let hold_expired = state
        .growth_blocked_until_unix_seconds
        .is_none_or(|deadline| deadline <= now);
    let recovered = state.penalty_level == 0 || state.healthy_since_penalty >= required;
    if growth_ready
        && (hold_expired || recovered)
        && state.successful_round >= required
        && state.window < MAX_WINDOW
    {
        state.window = state.window.saturating_add(1).min(MAX_WINDOW);
        state.successful_round = 0;
        state.healthy_since_penalty = 0;
        state.penalty_level = 0;
        state.growth_blocked_until_unix_seconds = None;
        state.last_growth = Some(Instant::now());
    }
    Duration::ZERO
}

fn observe_invalid_response(state: &mut ScopeState, foreground_inference: bool) -> Duration {
    if !foreground_inference {
        return transient_backoff(1);
    }
    state.updated_at_unix_seconds = now_seconds();
    state.successful_round = 0;
    state.invalid_response_streak = state.invalid_response_streak.saturating_add(1);
    let previous_window = state.window;
    state.window = state
        .window
        .saturating_sub(1)
        .max(INVALID_RESPONSE_WINDOW_FLOOR.min(previous_window));
    if state.window < previous_window {
        apply_growth_penalty(state);
    }
    let delay = invalid_response_backoff(state.invalid_response_streak);
    state.cooldown_until = Some(Instant::now() + delay);
    delay
}

fn observe_rate_limit(
    state: &mut ScopeState,
    retry_after: Option<Duration>,
    foreground_inference: bool,
) -> Duration {
    if !foreground_inference {
        return retry_after.unwrap_or_else(|| rate_limit_backoff(1));
    }
    state.updated_at_unix_seconds = now_seconds();
    state.window = (state.window / 2).max(1);
    state.successful_round = 0;
    state.rate_limit_streak = state.rate_limit_streak.saturating_add(1);
    apply_growth_penalty(state);
    let delay = retry_after.unwrap_or_else(|| rate_limit_backoff(state.rate_limit_streak));
    state.cooldown_until = Some(Instant::now() + delay);
    delay
}

fn observe_transient_failure(
    state: &mut ScopeState,
    halve_window: bool,
    foreground_inference: bool,
    block_growth: bool,
) -> Duration {
    if !foreground_inference {
        return transient_backoff(1);
    }
    state.updated_at_unix_seconds = now_seconds();
    state.transient_failures = state.transient_failures.saturating_add(1);
    state.successful_round = 0;
    state.window = if halve_window {
        (state.window / 2).max(1)
    } else {
        state.window.saturating_sub(1).max(1)
    };
    if block_growth {
        apply_growth_penalty(state);
    }
    let delay = transient_backoff(state.transient_failures);
    if state.transient_failures >= 3 {
        let breaker = 5_u64
            .saturating_mul(1_u64 << u32::from(state.transient_failures.saturating_sub(3).min(3)))
            .min(30);
        state.cooldown_until = Some(Instant::now() + Duration::from_secs(breaker));
    }
    delay
}

fn growth_successes(window: usize) -> usize {
    window.saturating_mul(2).max(4)
}

fn apply_growth_penalty(state: &mut ScopeState) {
    state.healthy_since_penalty = 0;
    state.penalty_level = state.penalty_level.saturating_add(1).min(4);
    let multiplier = 1_u32 << u32::from(state.penalty_level.saturating_sub(1));
    let hold = BASE_GROWTH_HOLD
        .saturating_mul(multiplier)
        .min(MAX_GROWTH_HOLD);
    let deadline = now_seconds().saturating_add(hold.as_secs());
    state.growth_blocked_until_unix_seconds = Some(
        state
            .growth_blocked_until_unix_seconds
            .map_or(deadline, |existing| existing.max(deadline)),
    );
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

fn invalid_response_backoff(streak: u8) -> Duration {
    let cap = if streak <= 1 {
        Duration::from_secs(2)
    } else {
        Duration::from_secs(3)
    };
    equal_jitter(cap)
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
    if cache.schema_version != 2 {
        return HashMap::new();
    }
    cache
        .scopes
        .into_iter()
        .map(|(scope, cached)| (scope, restored_scope(cached)))
        .collect()
}

fn restored_scope(cached: CachedScope) -> ScopeState {
    let now = now_seconds();
    let age = Duration::from_secs(now.saturating_sub(cached.updated_at_unix_seconds));
    let hold_active = cached
        .growth_blocked_until_unix_seconds
        .is_some_and(|deadline| deadline > now);
    let learned = cached.window.clamp(INITIAL_WINDOW, MAX_WINDOW);
    let window = if hold_active {
        cached.window.clamp(1, MAX_WINDOW)
    } else if age >= CACHE_TTL {
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
        healthy_since_penalty: if hold_active {
            cached.healthy_since_penalty
        } else {
            0
        },
        penalty_level: if hold_active { cached.penalty_level } else { 0 },
        growth_blocked_until_unix_seconds: if hold_active {
            cached.growth_blocked_until_unix_seconds
        } else {
            None
        },
        ..ScopeState::default()
    }
}

fn save_cache(path: &Path, scopes: &HashMap<String, ScopeState>) -> std::io::Result<()> {
    let cache = Cache {
        schema_version: 2,
        scopes: scopes
            .iter()
            .map(|(scope, state)| {
                (
                    scope.clone(),
                    CachedScope {
                        window: state.window,
                        updated_at_unix_seconds: state.updated_at_unix_seconds,
                        healthy_since_penalty: state.healthy_since_penalty,
                        penalty_level: state.penalty_level,
                        growth_blocked_until_unix_seconds: state.growth_blocked_until_unix_seconds,
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
        AcquireRequest, Cache, CachedScope, INITIAL_WINDOW, MAX_WINDOW, Pending, Scheduler,
        ScopeState, growth_successes, now_seconds, observe_invalid_response, observe_rate_limit,
        observe_success, restored_scope, schedule,
    };
    use crate::{RequestLane, RequestPriority};
    use std::collections::{HashMap, VecDeque};
    use std::time::{Duration, Instant};

    fn request(launch_id: &str) -> AcquireRequest {
        AcquireRequest {
            scope: "credential".to_owned(),
            launch_id: launch_id.to_owned(),
            lane: RequestLane::Inference,
            priority: RequestPriority::Foreground,
            enqueued_at: Instant::now(),
        }
    }

    fn background_request(launch_id: &str) -> AcquireRequest {
        AcquireRequest {
            priority: RequestPriority::Background,
            ..request(launch_id)
        }
    }

    fn control_request(launch_id: &str) -> AcquireRequest {
        AcquireRequest {
            lane: RequestLane::Control,
            priority: RequestPriority::Background,
            ..request(launch_id)
        }
    }

    #[test]
    fn success_grows_and_rate_limit_reduces_the_window() {
        let mut state = ScopeState::default();
        assert_eq!(state.window, 2);
        for _ in 0..growth_successes(state.window) {
            observe_success(&mut state, true, true, Some(Duration::from_secs(1)));
        }
        assert_eq!(state.window, 3);
        let delay = observe_rate_limit(&mut state, Some(Duration::from_secs(2)), true);
        assert_eq!(state.window, 1);
        assert_eq!(delay, Duration::from_secs(2));
        assert_eq!(state.penalty_level, 1);
        assert!(
            state
                .growth_blocked_until_unix_seconds
                .is_some_and(|deadline| deadline >= now_seconds() + 599)
        );
    }

    #[test]
    fn unsaturated_and_control_successes_do_not_grow_the_window() {
        let mut state = ScopeState::default();
        observe_success(&mut state, false, true, Some(Duration::from_secs(1)));
        observe_success(&mut state, false, false, Some(Duration::from_secs(1)));
        assert_eq!(state.window, INITIAL_WINDOW);

        observe_success(&mut state, true, true, Some(Duration::from_secs(31)));
        observe_success(&mut state, true, true, Some(Duration::from_secs(31)));
        assert_eq!(state.window, INITIAL_WINDOW);
    }

    #[test]
    fn invalid_foreground_inference_reduces_capacity_and_sets_a_shared_cooldown() {
        let mut state = ScopeState {
            window: 4,
            ..ScopeState::default()
        };

        let first = observe_invalid_response(&mut state, true);
        assert_eq!(state.window, 3);
        assert_eq!(state.invalid_response_streak, 1);
        assert!((Duration::from_secs(1)..=Duration::from_secs(2)).contains(&first));
        assert!(
            state
                .cooldown_until
                .is_some_and(|deadline| deadline > Instant::now())
        );

        let second = observe_invalid_response(&mut state, true);
        assert_eq!(state.window, 2);
        assert_eq!(state.invalid_response_streak, 2);
        assert_eq!(state.penalty_level, 2);
        assert!((Duration::from_millis(1_500)..=Duration::from_secs(3)).contains(&second));

        let _ = observe_invalid_response(&mut state, true);
        assert_eq!(state.window, 2);
        assert_eq!(state.invalid_response_streak, 3);
        assert_eq!(state.penalty_level, 2);

        observe_success(&mut state, false, true, Some(Duration::from_secs(1)));
        assert_eq!(state.invalid_response_streak, 0);
    }

    #[test]
    fn invalid_control_response_does_not_penalize_inference_capacity() {
        let mut state = ScopeState {
            window: 4,
            ..ScopeState::default()
        };

        let _ = observe_invalid_response(&mut state, false);

        assert_eq!(state.window, 4);
        assert_eq!(state.invalid_response_streak, 0);
        assert!(state.cooldown_until.is_none());
        assert!(state.growth_blocked_until_unix_seconds.is_none());
    }

    #[test]
    fn learned_capacity_decays_to_the_cold_window() {
        let state = restored_scope(CachedScope {
            window: 12,
            updated_at_unix_seconds: 0,
            healthy_since_penalty: 0,
            penalty_level: 0,
            growth_blocked_until_unix_seconds: None,
        });
        assert_eq!(state.window, 2);
    }

    #[test]
    fn active_capacity_penalty_survives_restart_at_window_one() {
        let deadline = now_seconds() + 600;
        let state = restored_scope(CachedScope {
            window: 1,
            updated_at_unix_seconds: now_seconds(),
            healthy_since_penalty: 3,
            penalty_level: 2,
            growth_blocked_until_unix_seconds: Some(deadline),
        });

        assert_eq!(state.window, 1);
        assert_eq!(state.healthy_since_penalty, 3);
        assert_eq!(state.penalty_level, 2);
        assert_eq!(state.growth_blocked_until_unix_seconds, Some(deadline));
    }

    #[test]
    fn healthy_evidence_can_restore_capacity_before_the_hold_expires() {
        let mut state = ScopeState {
            window: 1,
            penalty_level: 1,
            growth_blocked_until_unix_seconds: Some(now_seconds() + 600),
            ..ScopeState::default()
        };
        for _ in 0..growth_successes(state.window) {
            observe_success(&mut state, true, true, Some(Duration::from_secs(1)));
        }
        assert_eq!(state.window, 2);
        assert_eq!(state.penalty_level, 0);
        assert!(state.growth_blocked_until_unix_seconds.is_none());
    }

    #[test]
    fn an_expired_hold_still_requires_a_full_healthy_round() {
        let mut state = ScopeState {
            window: 1,
            penalty_level: 1,
            growth_blocked_until_unix_seconds: Some(now_seconds().saturating_sub(1)),
            ..ScopeState::default()
        };
        let required = growth_successes(state.window);

        for _ in 1..required {
            observe_success(&mut state, true, true, Some(Duration::from_secs(1)));
        }
        assert_eq!(state.window, 1);
        observe_success(&mut state, true, true, Some(Duration::from_secs(1)));
        assert_eq!(state.window, 2);
    }

    #[test]
    fn capacity_never_grows_above_ten() {
        let mut state = ScopeState {
            window: MAX_WINDOW,
            ..ScopeState::default()
        };
        for _ in 0..growth_successes(state.window) {
            observe_success(&mut state, true, true, Some(Duration::from_secs(1)));
        }
        assert_eq!(state.window, MAX_WINDOW);
    }

    #[test]
    fn version_two_cache_without_penalty_fields_remains_compatible() {
        let cache: Cache = serde_json::from_str(
            r#"{"schema_version":2,"scopes":{"credential":{"window":4,"updated_at_unix_seconds":0}}}"#,
        )
        .expect("legacy cache should deserialize");
        let scope = cache.scopes["credential"];
        assert_eq!(scope.penalty_level, 0);
        assert!(scope.growth_blocked_until_unix_seconds.is_none());
    }

    #[test]
    fn disconnected_waiters_are_removed_even_while_capacity_is_full() {
        let (reply, response) = tokio::sync::oneshot::channel();
        drop(response);
        let mut scopes = HashMap::from([(
            "credential".to_owned(),
            ScopeState {
                active: 1,
                window: 1,
                pending: VecDeque::from([Pending {
                    request: request("disconnected"),
                    reply,
                }]),
                ..ScopeState::default()
            },
        )]);

        let mut next_lease_id = 1;
        schedule(&mut scopes, &mut next_lease_id);

        assert!(scopes["credential"].pending.is_empty());
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

        scheduler.release("credential".to_owned(), true);
        let grant = tokio::time::timeout(Duration::from_millis(200), pending)
            .await
            .expect("queued request should resume")
            .expect("queued task should finish")
            .expect("queued request should receive a grant");
        assert!(grant.queued >= Duration::from_millis(50));
    }

    #[tokio::test]
    async fn ten_launches_eventually_receive_capacity_without_starvation() {
        let temporary = tempfile::tempdir().expect("temporary directory should exist");
        let scheduler = Scheduler::start(temporary.path().join("capacity.json"));
        let mut launches = tokio::task::JoinSet::new();
        for index in 0..10 {
            let scheduler = scheduler.clone();
            launches.spawn(async move {
                let launch = format!("launch-{index}");
                scheduler.acquire(request(&launch)).await.map(|_| launch)
            });
        }

        let mut completed = Vec::new();
        while let Some(result) = tokio::time::timeout(Duration::from_secs(1), launches.join_next())
            .await
            .expect("a queued launch should receive capacity")
        {
            let launch = result
                .expect("launch task should finish")
                .expect("launch should receive a grant");
            completed.push(launch);
            scheduler.release("credential".to_owned(), true);
        }
        completed.sort();
        assert_eq!(
            completed,
            (0..10)
                .map(|index| format!("launch-{index}"))
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn control_pressure_does_not_qualify_inference_capacity_growth() {
        let temporary = tempfile::tempdir().expect("temporary directory should exist");
        let scheduler = Scheduler::start(temporary.path().join("capacity.json"));
        scheduler
            .acquire(control_request("discovery"))
            .await
            .expect("control request should receive a grant");
        let grant = scheduler
            .acquire(request("interactive"))
            .await
            .expect("inference request should receive a grant");

        assert!(!grant.growth_eligible);
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
        scheduler.release("credential".to_owned(), true);

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
        scheduler.release("credential".to_owned(), true);
        tokio::time::timeout(Duration::from_millis(200), same)
            .await
            .expect("same launch should eventually resume")
            .expect("same launch task should finish")
            .expect("same launch should receive a grant");
    }

    #[tokio::test]
    async fn foreground_requests_pass_queued_background_work() {
        let temporary = tempfile::tempdir().expect("temporary directory should exist");
        let scheduler = Scheduler::start(temporary.path().join("capacity.json"));
        scheduler
            .acquire(request("first"))
            .await
            .expect("first grant");
        scheduler
            .acquire(request("second"))
            .await
            .expect("second grant");

        let background_scheduler = scheduler.clone();
        let mut background = tokio::spawn(async move {
            background_scheduler
                .acquire(background_request("system"))
                .await
        });
        let foreground_scheduler = scheduler.clone();
        let foreground =
            tokio::spawn(async move { foreground_scheduler.acquire(request("interactive")).await });
        tokio::time::sleep(Duration::from_millis(30)).await;
        scheduler.release("credential".to_owned(), true);

        tokio::time::timeout(Duration::from_millis(200), foreground)
            .await
            .expect("foreground should be selected")
            .expect("foreground task should finish")
            .expect("foreground should receive a grant");
        assert!(
            tokio::time::timeout(Duration::from_millis(40), &mut background)
                .await
                .is_err()
        );
    }
}
