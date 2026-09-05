use crate::protocol::{AttemptOutcome, RequestLane, RequestPriority};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

mod cache;
mod state;

use cache::{load_cache, save_cache};
use state::{ScopeState, now_seconds, observe};

const TICK: Duration = Duration::from_millis(25);
const CACHE_WRITE_INTERVAL: Duration = Duration::from_secs(1);
const BACKGROUND_AGING: Duration = Duration::from_secs(10);

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

#[cfg(test)]
mod cache_tests;
#[cfg(test)]
mod policy_tests;
#[cfg(test)]
mod scheduling_tests;
