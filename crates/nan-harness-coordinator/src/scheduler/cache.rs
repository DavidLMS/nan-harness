use super::state::{BudgetState, INITIAL_WINDOW, MAX_WINDOW, ScopeState, now_seconds};
use nan_harness_private_fs::{open_private_read, open_private_truncate};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write as _;
use std::path::Path;
use std::time::Duration;

const CACHE_TTL: Duration = Duration::from_hours(1);

#[derive(Serialize, Deserialize)]
pub(super) struct Cache {
    pub(super) schema_version: u8,
    pub(super) scopes: HashMap<String, CachedScope>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct CachedScope {
    pub(super) window: usize,
    pub(super) updated_at_unix_seconds: u64,
    #[serde(default)]
    pub(super) healthy_since_penalty: usize,
    #[serde(default)]
    pub(super) penalty_level: u8,
    #[serde(default)]
    pub(super) growth_blocked_until_unix_seconds: Option<u64>,
    #[serde(default)]
    pub(super) rate_limit_ceiling: Option<usize>,
    #[serde(default)]
    pub(super) budgets: HashMap<String, CachedBudget>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub(super) struct CachedBudget {
    pub(super) limit: u64,
    pub(super) consumed: u64,
    #[serde(default)]
    pub(super) in_flight: u32,
    #[serde(default)]
    pub(super) accounting_blocked: bool,
}

pub(super) fn load_cache(path: &Path) -> HashMap<String, ScopeState> {
    let Ok((file, _)) = open_private_read(path) else {
        return HashMap::new();
    };
    let Ok(cache) = serde_json::from_reader::<_, Cache>(file) else {
        return HashMap::new();
    };
    if cache.schema_version != 3 {
        return HashMap::new();
    }
    cache
        .scopes
        .into_iter()
        .map(|(scope, cached)| (scope, restored_scope(cached)))
        .collect()
}

pub(super) fn restored_scope(cached: CachedScope) -> ScopeState {
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
    let budgets = cached
        .budgets
        .into_iter()
        .map(|(launch_id, budget)| {
            (
                launch_id,
                BudgetState {
                    limit: budget.limit,
                    consumed: budget.consumed,
                    in_flight: 0,
                    accounting_blocked: budget.accounting_blocked || budget.in_flight > 0,
                },
            )
        })
        .collect();
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
        rate_limit_ceiling: if hold_active {
            cached.rate_limit_ceiling
        } else {
            None
        },
        budgets,
        ..ScopeState::default()
    }
}

pub(super) fn save_cache(path: &Path, scopes: &HashMap<String, ScopeState>) -> std::io::Result<()> {
    let cache = Cache {
        schema_version: 3,
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
                        rate_limit_ceiling: state.rate_limit_ceiling,
                        budgets: state
                            .budgets
                            .iter()
                            .map(|(launch_id, budget)| {
                                (
                                    launch_id.clone(),
                                    CachedBudget {
                                        limit: budget.limit,
                                        consumed: budget.consumed,
                                        in_flight: budget.in_flight,
                                        accounting_blocked: budget.accounting_blocked,
                                    },
                                )
                            })
                            .collect(),
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
