use super::cache::{Cache, CachedScope, restored_scope};
use super::state::{INITIAL_WINDOW, now_seconds};

#[test]
fn learned_capacity_decays_to_the_cold_window() {
    let state = restored_scope(CachedScope {
        window: 12,
        updated_at_unix_seconds: 0,
        healthy_since_penalty: 0,
        penalty_level: 0,
        growth_blocked_until_unix_seconds: None,
        rate_limit_ceiling: None,
        budgets: std::collections::HashMap::new(),
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
        rate_limit_ceiling: Some(2),
        budgets: std::collections::HashMap::new(),
    });

    assert_eq!(state.window, 1);
    assert_eq!(state.healthy_since_penalty, 3);
    assert_eq!(state.penalty_level, 2);
    assert_eq!(state.growth_blocked_until_unix_seconds, Some(deadline));
    assert_eq!(state.rate_limit_ceiling, Some(2));
}

#[test]
fn expired_capacity_penalty_is_not_restored_from_cache() {
    let state = restored_scope(CachedScope {
        window: 1,
        updated_at_unix_seconds: now_seconds(),
        healthy_since_penalty: 3,
        penalty_level: 2,
        growth_blocked_until_unix_seconds: Some(now_seconds().saturating_sub(1)),
        rate_limit_ceiling: Some(2),
        budgets: std::collections::HashMap::new(),
    });

    assert_eq!(state.window, INITIAL_WINDOW);
    assert_eq!(state.healthy_since_penalty, 0);
    assert_eq!(state.penalty_level, 0);
    assert!(state.growth_blocked_until_unix_seconds.is_none());
    assert!(state.rate_limit_ceiling.is_none());
}

#[test]
fn version_two_cache_without_penalty_fields_remains_compatible() {
    let cache: Cache = serde_json::from_str(
        r#"{"schema_version":2,"scopes":{"credential":{"window":4,"updated_at_unix_seconds":0}}}"#,
    )
    .expect("legacy cache should deserialize");
    let scope = &cache.scopes["credential"];
    assert_eq!(scope.penalty_level, 0);
    assert!(scope.growth_blocked_until_unix_seconds.is_none());
}

#[test]
fn cache_round_trip_preserves_penalties_without_restoring_live_leases() {
    use super::cache::{load_cache, save_cache};
    use super::state::ScopeState;
    use std::collections::HashMap;

    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("cache.json");
    let deadline = now_seconds() + 600;
    let scopes = HashMap::from([(
        "synthetic-scope".to_owned(),
        ScopeState {
            window: 1,
            active: 1,
            active_foreground_inference: 1,
            healthy_since_penalty: 3,
            penalty_level: 2,
            growth_blocked_until_unix_seconds: Some(deadline),
            rate_limit_ceiling: Some(2),
            ..ScopeState::default()
        },
    )]);
    save_cache(&path, &scopes).expect("save private cache");
    let restored = load_cache(&path);
    let state = &restored["synthetic-scope"];
    assert_eq!(state.window, 1);
    assert_eq!(state.healthy_since_penalty, 3);
    assert_eq!(state.penalty_level, 2);
    assert_eq!(state.growth_blocked_until_unix_seconds, Some(deadline));
    assert_eq!(state.rate_limit_ceiling, Some(2));
    assert_eq!(state.active, 0);
    assert_eq!(state.active_foreground_inference, 0);
    assert!(state.pending.is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&path)
                .expect("cache metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
