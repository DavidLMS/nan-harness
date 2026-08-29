use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

pub(crate) type SharedUsage = Arc<UsageLedger>;

#[derive(Debug, Default)]
pub(crate) struct UsageLedger {
    snapshot: Mutex<ProviderUsageSnapshot>,
    active_requests: AtomicUsize,
    idle: Notify,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderUsageSnapshot {
    pub models: BTreeMap<String, ModelUsageSnapshot>,
}

impl ProviderUsageSnapshot {
    #[must_use]
    pub fn completed_requests(&self) -> u64 {
        self.models.values().fold(0_u64, |total, usage| {
            total.saturating_add(usage.completed_requests())
        })
    }

    #[must_use]
    pub fn responses_with_usage(&self) -> u64 {
        self.sum(|usage| usage.responses_with_usage)
    }

    #[must_use]
    pub fn responses_without_usage(&self) -> u64 {
        self.sum(|usage| usage.responses_without_usage)
    }

    #[must_use]
    pub fn incomplete_responses(&self) -> u64 {
        self.sum(|usage| usage.incomplete_responses)
    }

    #[must_use]
    pub fn input_tokens(&self) -> u64 {
        self.sum(|usage| usage.input_tokens)
    }

    #[must_use]
    pub fn output_tokens(&self) -> u64 {
        self.sum(|usage| usage.output_tokens)
    }

    #[must_use]
    pub fn reasoning_tokens(&self) -> u64 {
        self.sum(|usage| usage.reasoning_tokens)
    }

    #[must_use]
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens().saturating_add(self.output_tokens())
    }

    #[must_use]
    pub fn inference_requests(&self) -> u64 {
        self.models.values().fold(0_u64, |total, usage| {
            total.saturating_add(usage.inference_requests())
        })
    }

    fn sum(&self, value: impl Fn(&ModelUsageSnapshot) -> u64) -> u64 {
        self.models
            .values()
            .fold(0_u64, |total, usage| total.saturating_add(value(usage)))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelUsageSnapshot {
    pub responses_with_usage: u64,
    pub responses_without_usage: u64,
    pub incomplete_responses: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
}

impl ModelUsageSnapshot {
    #[must_use]
    pub const fn completed_requests(&self) -> u64 {
        self.responses_with_usage
            .saturating_add(self.responses_without_usage)
    }

    #[must_use]
    pub const fn inference_requests(&self) -> u64 {
        self.completed_requests()
            .saturating_add(self.incomplete_responses)
    }

    #[must_use]
    pub const fn total_tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UsageValues {
    pub(crate) input: u64,
    pub(crate) output: u64,
    pub(crate) reasoning: u64,
}

#[derive(Debug)]
pub(crate) struct RequestUsageGuard {
    usage: SharedUsage,
    model: String,
    finished: bool,
}

impl RequestUsageGuard {
    pub(crate) fn new(usage: &SharedUsage, model: impl Into<String>) -> Self {
        usage.active_requests.fetch_add(1, Ordering::AcqRel);
        Self {
            usage: Arc::clone(usage),
            model: model.into(),
            finished: false,
        }
    }

    pub(crate) fn complete(&mut self, values: Option<UsageValues>) {
        if self.finished {
            return;
        }
        {
            let mut snapshot = self
                .usage
                .snapshot
                .lock()
                .expect("provider usage mutex should not be poisoned");
            let model = snapshot.models.entry(self.model.clone()).or_default();
            if let Some(values) = values {
                model.responses_with_usage = model.responses_with_usage.saturating_add(1);
                model.input_tokens = model.input_tokens.saturating_add(values.input);
                model.output_tokens = model.output_tokens.saturating_add(values.output);
                model.reasoning_tokens = model.reasoning_tokens.saturating_add(values.reasoning);
            } else {
                model.responses_without_usage = model.responses_without_usage.saturating_add(1);
            }
        }
        self.finished = true;
        request_finished(&self.usage);
    }
}

impl Drop for RequestUsageGuard {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        {
            let mut snapshot = self
                .usage
                .snapshot
                .lock()
                .expect("provider usage mutex should not be poisoned");
            let model = snapshot.models.entry(self.model.clone()).or_default();
            model.incomplete_responses = model.incomplete_responses.saturating_add(1);
        }
        self.finished = true;
        request_finished(&self.usage);
    }
}

pub(crate) fn new_usage() -> SharedUsage {
    Arc::new(UsageLedger::default())
}

pub(crate) fn snapshot(usage: &SharedUsage) -> ProviderUsageSnapshot {
    usage
        .snapshot
        .lock()
        .expect("provider usage mutex should not be poisoned")
        .clone()
}

pub(crate) async fn wait_until_idle(usage: &SharedUsage) {
    loop {
        let notified = usage.idle.notified();
        if usage.active_requests.load(Ordering::Acquire) == 0 {
            return;
        }
        notified.await;
    }
}

fn request_finished(usage: &UsageLedger) {
    let previous = usage.active_requests.fetch_sub(1, Ordering::AcqRel);
    debug_assert!(previous > 0, "usage request count should not underflow");
    if previous == 1 {
        usage.idle.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ModelUsageSnapshot, RequestUsageGuard, UsageValues, new_usage, snapshot, wait_until_idle,
    };

    #[test]
    fn aggregates_models_concurrently_and_saturates_every_sum() {
        let usage = new_usage();
        {
            let mut snapshot = usage.snapshot.lock().expect("usage lock");
            snapshot.models.insert(
                "saturated".to_owned(),
                ModelUsageSnapshot {
                    responses_with_usage: u64::MAX,
                    responses_without_usage: u64::MAX,
                    incomplete_responses: u64::MAX,
                    input_tokens: u64::MAX,
                    output_tokens: u64::MAX,
                    reasoning_tokens: u64::MAX,
                },
            );
        }

        std::thread::scope(|scope| {
            for (model, input, output) in [("qwen3.6", 12, 4), ("glm5.2", 8, 3)] {
                let usage = usage.clone();
                scope.spawn(move || {
                    let mut guard = RequestUsageGuard::new(&usage, model);
                    guard.complete(Some(UsageValues {
                        input,
                        output,
                        reasoning: 2,
                    }));
                });
            }
        });
        let mut saturated = RequestUsageGuard::new(&usage, "saturated");
        saturated.complete(Some(UsageValues {
            input: 1,
            output: 1,
            reasoning: 1,
        }));

        let snapshot = snapshot(&usage);
        assert_eq!(snapshot.models["qwen3.6"].input_tokens, 12);
        assert_eq!(snapshot.models["glm5.2"].output_tokens, 3);
        assert_eq!(snapshot.models["saturated"].responses_with_usage, u64::MAX);
        assert_eq!(snapshot.input_tokens(), u64::MAX);
        assert_eq!(snapshot.completed_requests(), u64::MAX);
        assert_eq!(snapshot.inference_requests(), u64::MAX);
    }

    #[test]
    fn dropped_and_usage_free_requests_are_distinct() {
        let usage = new_usage();
        drop(RequestUsageGuard::new(&usage, "qwen3.6"));
        let mut complete = RequestUsageGuard::new(&usage, "qwen3.6");
        complete.complete(None);

        let snapshot = snapshot(&usage);
        assert_eq!(snapshot.completed_requests(), 1);
        assert_eq!(snapshot.responses_without_usage(), 1);
        assert_eq!(snapshot.incomplete_responses(), 1);
    }

    #[tokio::test]
    async fn idle_waits_until_every_request_guard_records_its_outcome() {
        let usage = new_usage();
        let incomplete = RequestUsageGuard::new(&usage, "qwen3.6");
        let mut complete = RequestUsageGuard::new(&usage, "glm5.2");
        let waiter_usage = usage.clone();
        let waiter = tokio::spawn(async move {
            wait_until_idle(&waiter_usage).await;
        });

        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());
        complete.complete(None);
        assert!(!waiter.is_finished());
        drop(incomplete);
        waiter.await.expect("idle waiter should finish");

        let snapshot = snapshot(&usage);
        assert_eq!(snapshot.responses_without_usage(), 1);
        assert_eq!(snapshot.incomplete_responses(), 1);
    }
}
