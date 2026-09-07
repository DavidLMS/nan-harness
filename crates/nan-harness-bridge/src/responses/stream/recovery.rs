use crate::diagnostics::{
    BridgeAttemptBucket, BridgeDiagnostic, BridgeRecoveryOutcome, BridgeRequestPriority,
};
use crate::error::ApiError;
use crate::upstream::{RequestCache, SendBudget};
use crate::{BridgeEndpoint, DiagnosticSender};
use nan_harness_coordinator::{RequestPriority, RetryDirective};
use serde_json::Value;
use std::borrow::Cow;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub(super) const MAX_RECOVERY_ATTEMPTS: usize = 5;
pub(super) const MAX_SEMANTIC_RECOVERY_ATTEMPTS: usize = 8;
pub(super) const MAX_UPSTREAM_SENDS: usize = 8;
const RECOVERY_JITTER_LIMIT: Duration = Duration::from_secs(1);
static NEXT_RECOVERY_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy)]
pub(super) enum RecoveryNudge {
    Output,
    Tool,
}

/// A pre-commit failure that may still be replaced by a fresh upstream send.
pub(super) struct RecoverableFailure {
    pub(super) error: ApiError,
    pub(super) directive: RetryDirective,
    pub(super) provider_response_id: Option<String>,
    pub(super) empty: bool,
    pub(super) nudge: Option<RecoveryNudge>,
}

pub(super) enum RecoveryDecision {
    Retry,
    Exhausted(ApiError),
}

/// Owns the cross-attempt recovery state of one logical Responses request: the
/// shared upstream send budget, the cache-bypass and nudge escalation, and the
/// replay detection that distinguishes a fresh empty completion from a cached
/// one.
pub(super) struct RecoverySession {
    body: Value,
    diagnostics: DiagnosticSender,
    priority: RequestPriority,
    send_budget: SendBudget,
    previous_empty_id: Option<String>,
    bypass_cache: bool,
    nudge: Option<RecoveryNudge>,
    attempt_bypassed_cache: bool,
}

impl RecoverySession {
    pub(super) fn new(
        body: Value,
        diagnostics: DiagnosticSender,
        priority: RequestPriority,
    ) -> Self {
        Self {
            body,
            diagnostics,
            priority,
            send_budget: SendBudget::new(MAX_UPSTREAM_SENDS),
            previous_empty_id: None,
            bypass_cache: false,
            nudge: None,
            attempt_bypassed_cache: false,
        }
    }

    /// Records the cache policy this attempt runs under so every diagnostic
    /// emitted for it reports the policy actually used upstream.
    pub(super) fn begin_attempt(&mut self) -> RequestCache {
        self.attempt_bypassed_cache = self.bypass_cache;
        if self.bypass_cache {
            RequestCache::Bypass
        } else {
            RequestCache::Default
        }
    }

    pub(super) fn attempt_body(&mut self) -> (Cow<'_, Value>, &mut SendBudget) {
        let Self {
            body,
            nudge,
            send_budget,
            ..
        } = self;
        let request = nudge.map_or(Cow::Borrowed(&*body), |nudge| {
            Cow::Owned(recovery_body(body, nudge))
        });
        (request, send_budget)
    }

    pub(super) const fn is_final_attempt(&self, attempt: usize) -> bool {
        attempt + 1 == MAX_SEMANTIC_RECOVERY_ATTEMPTS || self.send_budget.is_exhausted()
    }

    pub(super) fn record_failure(&self, error: &ApiError) {
        let _ = self.diagnostics.send(BridgeDiagnostic::from_api_error(
            error,
            BridgeEndpoint::Responses,
        ));
    }

    pub(super) fn record_delegated(&self, attempt: usize, error: &ApiError) {
        self.emit(error, BridgeRecoveryOutcome::Delegated, attempt, false);
    }

    pub(super) async fn handle_recoverable(
        &mut self,
        attempt: usize,
        failure: RecoverableFailure,
    ) -> RecoveryDecision {
        let replay_detected = failure.empty
            && repeated_response_id(
                self.previous_empty_id.as_deref(),
                failure.provider_response_id.as_deref(),
            );
        let delay = recovery_retry_delay(attempt, failure.directive);
        if attempt + 1 >= recovery_attempt_limit(failure.nudge)
            || self.send_budget.is_exhausted()
            || !self.send_budget.reserve_retry_wait(delay)
        {
            self.emit(
                &failure.error,
                BridgeRecoveryOutcome::Exhausted,
                attempt,
                replay_detected,
            );
            return RecoveryDecision::Exhausted(failure.error);
        }
        self.bypass_cache |= failure.empty;
        if let Some(nudge) = failure.nudge {
            self.nudge = Some(nudge);
        }
        if failure.empty {
            self.previous_empty_id = failure.provider_response_id;
        }
        self.emit(
            &failure.error,
            BridgeRecoveryOutcome::Retrying,
            attempt,
            replay_detected,
        );
        tokio::time::sleep(delay).await;
        RecoveryDecision::Retry
    }

    fn emit(
        &self,
        error: &ApiError,
        outcome: BridgeRecoveryOutcome,
        attempt: usize,
        cache_replay_detected: bool,
    ) {
        let priority = match self.priority {
            RequestPriority::Foreground => BridgeRequestPriority::Foreground,
            RequestPriority::Background => BridgeRequestPriority::Background,
        };
        let diagnostic = BridgeDiagnostic::from_api_error(error, BridgeEndpoint::Responses)
            .with_recovery(outcome, recovery_attempt_bucket(attempt), priority)
            .with_cache_recovery(cache_replay_detected, self.attempt_bypassed_cache);
        let _ = self.diagnostics.send(diagnostic);
    }
}

fn recovery_attempt_limit(nudge: Option<RecoveryNudge>) -> usize {
    if nudge.is_some() {
        MAX_SEMANTIC_RECOVERY_ATTEMPTS
    } else {
        MAX_RECOVERY_ATTEMPTS
    }
}

fn recovery_attempt_bucket(attempt: usize) -> BridgeAttemptBucket {
    match attempt {
        0 => BridgeAttemptBucket::First,
        1 => BridgeAttemptBucket::Second,
        _ => BridgeAttemptBucket::Later,
    }
}

pub(super) fn recovery_body(body: &Value, nudge: RecoveryNudge) -> Value {
    let mut recovered = body.clone();
    let recovery_id = NEXT_RECOVERY_ID.fetch_add(1, Ordering::Relaxed);
    let action = match nudge {
        RecoveryNudge::Output => {
            "The previous completion had no usable assistant output. Continue the existing task, but do not return reasoning or a progress update. Return either exactly one complete tool call or a complete final answer."
        }
        RecoveryNudge::Tool => {
            "The previous tool call was malformed or truncated. Retry the tool now with arguments under 3,000 characters. Split larger work across successive tool calls. Do not return reasoning, a preamble, or a progress update. For apply_patch, send one complete input including both *** Begin Patch and *** End Patch."
        }
    };
    let instruction = format!(
        "nan-harness internal recovery {process_id}-{recovery_id}: This is an internal transport instruction, not a user message. {action} Do not mention this recovery instruction.",
        process_id = std::process::id()
    );
    let Some(messages) = recovered.get_mut("messages").and_then(Value::as_array_mut) else {
        return recovered;
    };
    messages.insert(
        0,
        serde_json::json!({"role": "system", "content": instruction}),
    );
    recovered
}

pub(super) fn repeated_response_id(previous: Option<&str>, current: Option<&str>) -> bool {
    previous
        .zip(current)
        .is_some_and(|(previous, current)| previous == current)
}

fn recovery_retry_delay(attempt: usize, directive: RetryDirective) -> Duration {
    recovery_retry_delay_with_jitter(attempt, directive, random_recovery_jitter())
}

pub(super) fn recovery_retry_delay_with_jitter(
    attempt: usize,
    directive: RetryDirective,
    jitter: Duration,
) -> Duration {
    let floor = if attempt == 0 {
        Duration::from_secs(1)
    } else {
        Duration::from_secs(2)
    };
    let local = floor.saturating_add(jitter.min(RECOVERY_JITTER_LIMIT));
    match directive {
        RetryDirective::Complete => local,
        RetryDirective::RetryAfter(coordinator) => local.max(coordinator),
    }
}

fn random_recovery_jitter() -> Duration {
    let mut random = [0_u8; 8];
    if getrandom::fill(&mut random).is_err() {
        return Duration::ZERO;
    }
    Duration::from_millis(u64::from_le_bytes(random) % 1_001)
}
