use super::{RemoteScriptAttempt, RemoteScriptAttemptError, run_with_retries};
use std::collections::VecDeque;
use std::future::Future;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const REQUESTED: Duration = Duration::from_secs(30);
/// Production waits five seconds between transport retries; zero keeps the
/// scripted retries instant while `remaining` still applies its floor.
const NO_RETRY_DELAY: Duration = Duration::ZERO;
const MINIMUM: Duration = Duration::from_millis(1);

/// A scripted stand-in for a real SSH attempt: it records what the policy
/// supplied and never touches a process, a VM or the filesystem.
struct ScriptedAttempts {
    outcomes: Mutex<VecDeque<Result<(), RemoteScriptAttemptError>>>,
    calls: Mutex<Vec<(Duration, bool)>>,
}

impl ScriptedAttempts {
    fn new(outcomes: impl IntoIterator<Item = Result<(), RemoteScriptAttemptError>>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes.into_iter().collect()),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<(Duration, bool)> {
        self.calls.lock().expect("recorded attempts").clone()
    }

    fn appends(&self) -> Vec<bool> {
        self.calls().into_iter().map(|(_, append)| append).collect()
    }

    fn timeouts(&self) -> Vec<Duration> {
        self.calls()
            .into_iter()
            .map(|(timeout, _)| timeout)
            .collect()
    }

    fn unused_outcomes(&self) -> usize {
        self.outcomes.lock().expect("scripted outcomes").len()
    }
}

impl RemoteScriptAttempt for ScriptedAttempts {
    async fn attempt(
        &self,
        timeout: Duration,
        append_log: bool,
    ) -> Result<(), RemoteScriptAttemptError> {
        self.calls
            .lock()
            .expect("recorded attempts")
            .push((timeout, append_log));
        self.outcomes
            .lock()
            .expect("scripted outcomes")
            .pop_front()
            .expect("the policy should not attempt more often than the script allows")
    }
}

async fn bounded<T>(operation: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(2), operation)
        .await
        .expect("the SSH retry policy should complete within the test deadline")
}

/// A deadline far enough ahead that every attempt receives the requested
/// timeout unshortened.
fn open_deadline() -> Instant {
    Instant::now() + REQUESTED
}

/// `Instant::now()` is already spent by the time the policy reads it.
fn expired_deadline() -> Instant {
    Instant::now()
}

#[tokio::test]
async fn a_first_success_runs_a_single_truncating_attempt() {
    let attempts = ScriptedAttempts::new([Ok(())]);

    let result = bounded(run_with_retries(
        &attempts,
        open_deadline(),
        REQUESTED,
        NO_RETRY_DELAY,
    ))
    .await;

    assert!(
        result.is_ok(),
        "a first success should not fail: {result:?}"
    );
    assert_eq!(attempts.appends(), vec![false]);
    assert_eq!(attempts.unused_outcomes(), 0);
}

#[tokio::test]
async fn a_retryable_failure_is_followed_by_an_appending_retry() {
    let attempts = ScriptedAttempts::new([
        Err(RemoteScriptAttemptError::retryable(
            "SSH transport exited with status 255",
        )),
        Ok(()),
    ]);

    let result = bounded(run_with_retries(
        &attempts,
        open_deadline(),
        REQUESTED,
        NO_RETRY_DELAY,
    ))
    .await;

    assert!(result.is_ok(), "the retry should succeed: {result:?}");
    assert_eq!(attempts.appends(), vec![false, true]);
    assert_eq!(attempts.unused_outcomes(), 0);
}

#[tokio::test]
async fn four_retryable_failures_stop_and_report_the_last_detail() {
    let attempts = ScriptedAttempts::new([
        Err(RemoteScriptAttemptError::retryable(
            "could not start SSH: 1",
        )),
        Err(RemoteScriptAttemptError::retryable(
            "could not start SSH: 2",
        )),
        Err(RemoteScriptAttemptError::retryable(
            "could not start SSH: 3",
        )),
        Err(RemoteScriptAttemptError::retryable(
            "could not start SSH: 4",
        )),
        Ok(()),
    ]);

    let result = bounded(run_with_retries(
        &attempts,
        open_deadline(),
        REQUESTED,
        NO_RETRY_DELAY,
    ))
    .await;

    assert_eq!(result, Err("could not start SSH: 4".to_owned()));
    assert_eq!(attempts.appends(), vec![false, true, true, true]);
    assert_eq!(
        attempts.unused_outcomes(),
        1,
        "a fifth attempt must never run"
    );
}

#[tokio::test]
async fn a_fatal_first_failure_is_not_retried() {
    let attempts = ScriptedAttempts::new([
        Err(RemoteScriptAttemptError::fatal("remote step timed out")),
        Ok(()),
    ]);

    let result = bounded(run_with_retries(
        &attempts,
        open_deadline(),
        REQUESTED,
        NO_RETRY_DELAY,
    ))
    .await;

    assert_eq!(result, Err("remote step timed out".to_owned()));
    assert_eq!(attempts.appends(), vec![false]);
    assert_eq!(attempts.unused_outcomes(), 1);
}

#[tokio::test]
async fn a_fatal_failure_after_a_retry_leaves_the_rest_of_the_script_unused() {
    let attempts = ScriptedAttempts::new([
        Err(RemoteScriptAttemptError::retryable(
            "SSH transport exited with status 255",
        )),
        Err(RemoteScriptAttemptError::fatal(
            "remote step exited with status 3",
        )),
        Ok(()),
        Ok(()),
    ]);

    let result = bounded(run_with_retries(
        &attempts,
        open_deadline(),
        REQUESTED,
        NO_RETRY_DELAY,
    ))
    .await;

    assert_eq!(result, Err("remote step exited with status 3".to_owned()));
    assert_eq!(attempts.appends(), vec![false, true]);
    assert_eq!(attempts.unused_outcomes(), 2);
}

#[tokio::test]
async fn every_attempt_receives_a_timeout_bounded_by_the_request() {
    let attempts = ScriptedAttempts::new([
        Err(RemoteScriptAttemptError::retryable(
            "could not start SSH: 1",
        )),
        Ok(()),
    ]);

    let result = bounded(run_with_retries(
        &attempts,
        open_deadline(),
        REQUESTED,
        NO_RETRY_DELAY,
    ))
    .await;

    assert!(result.is_ok(), "the retry should succeed: {result:?}");
    let timeouts = attempts.timeouts();
    assert_eq!(timeouts.len(), 2);
    for timeout in timeouts {
        assert!(
            (MINIMUM..=REQUESTED).contains(&timeout),
            "an open deadline should grant at most the requested timeout, got {timeout:?}"
        );
    }
}

#[tokio::test]
async fn an_expired_deadline_still_grants_the_minimum_timeout() {
    let attempts = ScriptedAttempts::new([
        Err(RemoteScriptAttemptError::retryable(
            "could not start SSH: 1",
        )),
        Ok(()),
    ]);

    let result = bounded(run_with_retries(
        &attempts,
        expired_deadline(),
        REQUESTED,
        NO_RETRY_DELAY,
    ))
    .await;

    assert!(result.is_ok(), "the retry should succeed: {result:?}");
    assert_eq!(attempts.timeouts(), vec![MINIMUM, MINIMUM]);
}
