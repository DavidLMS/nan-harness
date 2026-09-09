use super::state::{
    INITIAL_WINDOW, MAX_WINDOW, ScopeState, growth_successes, now_seconds,
    observe_invalid_response, observe_rate_limit, observe_success, observe_transient_failure,
};
use std::time::{Duration, Instant};

#[test]
fn retry_cooldown_extreme_hint_never_expires_or_gets_shortened() {
    use super::state::CooldownDeadline;
    let mut state = ScopeState::default();
    let delay = observe_rate_limit(&mut state, Some(Duration::MAX), true);
    assert_eq!(delay, Duration::MAX);
    assert_eq!(
        state.cooldown_until,
        Some(CooldownDeadline::Unrepresentable)
    );
    observe_rate_limit(&mut state, Some(Duration::ZERO), true);
    observe_invalid_response(&mut state, true);
    for _ in 0..3 {
        observe_transient_failure(&mut state, true, true, true);
    }
    observe_success(&mut state, true, true, Some(Duration::from_secs(1)));
    assert_eq!(
        state.cooldown_until,
        Some(CooldownDeadline::Unrepresentable)
    );
    assert!(
        state
            .cooldown_until
            .expect("cooldown")
            .is_active(Instant::now())
    );
}

#[test]
fn retry_cooldown_saturated_wire_hint_has_no_early_expiration() {
    let mut state = ScopeState::default();
    observe_rate_limit(&mut state, Some(Duration::from_millis(u64::MAX)), true);
    assert_eq!(
        state.cooldown_until,
        Some(super::state::CooldownDeadline::Unrepresentable)
    );
}

#[test]
fn retry_cooldown_preserves_longer_hints_and_honors_server_errors() {
    let mut state = ScopeState::default();
    let delay = super::state::observe(
        &mut state,
        crate::AttemptOutcome::ServerError,
        Some(Duration::from_mins(1)),
        false,
        true,
        None,
    );
    assert_eq!(delay, Duration::from_mins(1));
    let deadline = state.cooldown_until;
    observe_rate_limit(&mut state, Some(Duration::from_secs(1)), true);
    assert_eq!(state.cooldown_until, deadline);
    observe_rate_limit(&mut state, Some(Duration::from_mins(2)), true);
    assert!(state.cooldown_until > deadline);
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
            .is_some_and(|deadline| deadline.is_active(Instant::now()))
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
fn transient_failures_preserve_two_parallel_slots() {
    let mut state = ScopeState::default();

    let _ = observe_transient_failure(&mut state, false, true, false);

    assert_eq!(state.window, INITIAL_WINDOW);
    assert_eq!(state.transient_failures, 1);
    assert_eq!(state.penalty_level, 0);
    assert!(state.growth_blocked_until_unix_seconds.is_none());

    state.window = 4;
    let _ = observe_transient_failure(&mut state, true, true, true);
    assert_eq!(state.window, 2);
    assert_eq!(state.penalty_level, 1);

    let _ = observe_transient_failure(&mut state, true, true, true);
    assert_eq!(state.window, 2);
    assert_eq!(state.penalty_level, 2);
}

#[test]
fn another_rate_limit_lowers_the_estimate_and_extends_the_hold() {
    let mut state = ScopeState {
        window: 6,
        ..ScopeState::default()
    };
    observe_rate_limit(&mut state, Some(Duration::ZERO), true);
    let first_deadline = state.growth_blocked_until_unix_seconds.expect("hold");
    assert_eq!(state.rate_limit_ceiling, Some(5));

    observe_success(&mut state, true, true, Some(Duration::from_secs(1)));
    observe_rate_limit(&mut state, Some(Duration::ZERO), true);

    assert_eq!(state.window, 1);
    assert_eq!(state.rate_limit_ceiling, Some(2));
    assert_eq!(state.penalty_level, 2);
    assert_eq!(state.healthy_since_penalty, 0);
    assert_eq!(state.successful_round, 0);
    assert!(
        state
            .growth_blocked_until_unix_seconds
            .is_some_and(|deadline| deadline > first_deadline)
    );
}

#[test]
fn rate_limit_ceiling_prevents_repeated_probes_during_the_hold() {
    let mut state = ScopeState {
        window: 6,
        ..ScopeState::default()
    };

    let _ = observe_rate_limit(&mut state, None, true);
    assert_eq!(state.window, 3);
    assert_eq!(state.rate_limit_ceiling, Some(5));

    for expected_window in [4, 5] {
        state.last_growth = None;
        for _ in 0..growth_successes(state.window) {
            observe_success(&mut state, true, true, Some(Duration::from_secs(1)));
        }
        assert_eq!(state.window, expected_window);
        assert_eq!(state.rate_limit_ceiling, Some(5));
        assert_eq!(state.penalty_level, 1);
    }

    state.last_growth = None;
    for _ in 0..growth_successes(state.window) * 2 {
        observe_success(&mut state, true, true, Some(Duration::from_secs(1)));
    }
    assert_eq!(state.window, 5);
    assert_eq!(state.successful_round, 0);

    state.growth_blocked_until_unix_seconds = Some(now_seconds().saturating_sub(1));
    for _ in 0..growth_successes(state.window) {
        observe_success(&mut state, true, true, Some(Duration::from_secs(1)));
    }
    assert_eq!(state.window, 6);
    assert_eq!(state.penalty_level, 0);
    assert!(state.rate_limit_ceiling.is_none());
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
fn rate_limit_policy_escalates_caps_and_resets_on_success() {
    let mut state = ScopeState::default();
    for streak in 1..=u8::MAX {
        let delay = observe_rate_limit(&mut state, None, true);
        let step = u64::from(streak.min(3));
        assert!((Duration::from_secs(15 * step)..=Duration::from_secs(20 * step)).contains(&delay));
        assert_eq!(state.rate_limit_streak, streak);
        assert_eq!(state.window, 1);
    }
    let cooldown = state.cooldown_until;
    observe_success(&mut state, false, true, Some(Duration::from_secs(1)));
    let delay = observe_rate_limit(&mut state, None, true);
    assert!((Duration::from_secs(15)..=Duration::from_secs(20)).contains(&delay));
    assert_eq!(state.rate_limit_streak, 1);
    assert_eq!(state.cooldown_until, cooldown);
}

#[test]
fn control_rate_limits_keep_inference_state_unchanged() {
    let mut state = ScopeState::default();
    for _ in 0..4 {
        let delay = observe_rate_limit(&mut state, None, false);
        assert!((Duration::from_secs(15)..=Duration::from_secs(20)).contains(&delay));
    }
    assert_eq!(state.window, INITIAL_WINDOW);
    assert_eq!(state.rate_limit_streak, 0);
    assert!(state.cooldown_until.is_none());
    assert!(state.growth_blocked_until_unix_seconds.is_none());
}
