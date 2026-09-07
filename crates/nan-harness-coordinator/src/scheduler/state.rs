use super::Pending;
use crate::protocol::AttemptOutcome;
use std::collections::VecDeque;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(super) const INITIAL_WINDOW: usize = 2;
pub(super) const SOFT_FAILURE_WINDOW_FLOOR: usize = 2;
pub(super) const MAX_WINDOW: usize = 10;
const MIN_GROWTH_INTERVAL: Duration = Duration::from_mins(2);
const HEALTHY_HEADERS: Duration = Duration::from_secs(30);
const BASE_GROWTH_HOLD: Duration = Duration::from_mins(10);
const MAX_GROWTH_HOLD: Duration = Duration::from_hours(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum CooldownDeadline {
    Until(Instant),
    // A hint outside the monotonic clock range never expires in this process.
    // Clamping it to a shorter representable deadline could send too early.
    Unrepresentable,
}

impl CooldownDeadline {
    pub(super) fn is_active(self, now: Instant) -> bool {
        match self {
            Self::Until(deadline) => deadline > now,
            Self::Unrepresentable => true,
        }
    }
}

pub(super) struct ScopeState {
    pub(super) active: usize,
    pub(super) active_foreground_inference: usize,
    pub(super) window: usize,
    pub(super) successful_round: usize,
    pub(super) healthy_since_penalty: usize,
    pub(super) penalty_level: u8,
    pub(super) growth_blocked_until_unix_seconds: Option<u64>,
    pub(super) rate_limit_ceiling: Option<usize>,
    pub(super) transient_failures: u8,
    pub(super) invalid_response_streak: u8,
    pub(super) rate_limit_streak: u8,
    pub(super) cooldown_until: Option<CooldownDeadline>,
    pub(super) last_launch: Option<String>,
    pub(super) pending: VecDeque<Pending>,
    pub(super) updated_at_unix_seconds: u64,
    pub(super) last_growth: Option<Instant>,
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
            rate_limit_ceiling: None,
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

impl ScopeState {
    fn extend_cooldown(&mut self, delay: Duration) {
        // The wire format saturates milliseconds at u64::MAX. Such a hint may
        // originally have been much longer, so it cannot safely expire here.
        let deadline = (delay < Duration::from_millis(u64::MAX))
            .then(|| Instant::now().checked_add(delay))
            .flatten()
            .map_or(CooldownDeadline::Unrepresentable, CooldownDeadline::Until);
        self.cooldown_until = Some(
            self.cooldown_until
                .map_or(deadline, |old| old.max(deadline)),
        );
    }
}

pub(super) fn observe(
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
        AttemptOutcome::Timeout => {
            observe_transient_failure(state, true, foreground_inference, true)
        }
        AttemptOutcome::ServerError => {
            let delay = observe_transient_failure(state, true, foreground_inference, true);
            if let Some(hint) = retry_after {
                if foreground_inference {
                    state.extend_cooldown(hint);
                }
                delay.max(hint)
            } else {
                delay
            }
        }
        AttemptOutcome::InvalidResponse => observe_invalid_response(state, foreground_inference),
        AttemptOutcome::Cancelled | AttemptOutcome::Terminal => Duration::ZERO,
    }
}

pub(super) fn observe_success(
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
    if !hold_expired
        && state
            .rate_limit_ceiling
            .is_some_and(|ceiling| state.window >= ceiling)
    {
        state.successful_round = 0;
    }
    let recovered = state.penalty_level == 0 || state.healthy_since_penalty >= required;
    let ceiling_allows_growth = hold_expired
        || state
            .rate_limit_ceiling
            .is_none_or(|ceiling| state.window < ceiling);
    if growth_ready
        && (hold_expired || recovered)
        && ceiling_allows_growth
        && state.successful_round >= required
        && state.window < MAX_WINDOW
    {
        state.window = state.window.saturating_add(1).min(MAX_WINDOW);
        state.successful_round = 0;
        state.healthy_since_penalty = 0;
        if hold_expired || state.rate_limit_ceiling.is_none() {
            state.penalty_level = 0;
            state.growth_blocked_until_unix_seconds = None;
            state.rate_limit_ceiling = None;
        }
        state.last_growth = Some(Instant::now());
    }
    Duration::ZERO
}

pub(super) fn observe_invalid_response(
    state: &mut ScopeState,
    foreground_inference: bool,
) -> Duration {
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
        .max(SOFT_FAILURE_WINDOW_FLOOR.min(previous_window));
    if state.window < previous_window {
        apply_growth_penalty(state);
    }
    let delay = invalid_response_backoff(state.invalid_response_streak);
    state.extend_cooldown(delay);
    delay
}

pub(super) fn observe_rate_limit(
    state: &mut ScopeState,
    retry_after: Option<Duration>,
    foreground_inference: bool,
) -> Duration {
    if !foreground_inference {
        return retry_after.unwrap_or_else(|| rate_limit_backoff(1));
    }
    state.updated_at_unix_seconds = now_seconds();
    let previous_window = state.window;
    let estimated_ceiling = previous_window
        .saturating_sub(1)
        .max(INITIAL_WINDOW.min(previous_window));
    state.rate_limit_ceiling = Some(
        state
            .rate_limit_ceiling
            .map_or(estimated_ceiling, |ceiling| ceiling.min(estimated_ceiling)),
    );
    state.window = (state.window / 2).max(1);
    state.successful_round = 0;
    state.rate_limit_streak = state.rate_limit_streak.saturating_add(1);
    apply_growth_penalty(state);
    let delay = retry_after.unwrap_or_else(|| rate_limit_backoff(state.rate_limit_streak));
    state.extend_cooldown(delay);
    delay
}

pub(super) fn observe_transient_failure(
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
        (state.window / 2).max(SOFT_FAILURE_WINDOW_FLOOR.min(state.window))
    } else {
        let previous_window = state.window;
        state
            .window
            .saturating_sub(1)
            .max(SOFT_FAILURE_WINDOW_FLOOR.min(previous_window))
    };
    if block_growth {
        apply_growth_penalty(state);
    }
    let delay = transient_backoff(state.transient_failures);
    if state.transient_failures >= 3 {
        let breaker = 5_u64
            .saturating_mul(1_u64 << u32::from(state.transient_failures.saturating_sub(3).min(3)))
            .min(30);
        state.extend_cooldown(Duration::from_secs(breaker));
    }
    delay
}

pub(super) fn growth_successes(window: usize) -> usize {
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

pub(super) fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
