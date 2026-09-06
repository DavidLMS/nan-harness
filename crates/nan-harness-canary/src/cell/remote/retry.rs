use super::super::remaining;
use super::RemoteScriptAttemptError;
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests;

const SSH_TRANSPORT_ATTEMPTS: u8 = 4;

/// One real SSH run of the step script, kept separate from the retry policy
/// below so the policy can be exercised without spawning commands.
pub(super) trait RemoteScriptAttempt {
    async fn attempt(
        &self,
        timeout: Duration,
        append_log: bool,
    ) -> Result<(), RemoteScriptAttemptError>;
}

/// Runs the step script over SSH, retrying only transport failures and only
/// while attempts remain. The first attempt truncates the private step log and
/// every retry appends to it, so a flaky transport keeps the whole history.
pub(super) async fn run_with_retries(
    runner: &impl RemoteScriptAttempt,
    deadline: Instant,
    timeout: Duration,
    retry_delay: Duration,
) -> Result<(), String> {
    let mut attempt = 1;
    loop {
        let error = match runner
            .attempt(remaining(deadline, timeout), attempt > 1)
            .await
        {
            Ok(()) => return Ok(()),
            Err(error) => error,
        };
        if !error.retryable || attempt == SSH_TRANSPORT_ATTEMPTS {
            return Err(error.detail);
        }
        tokio::time::sleep(remaining(deadline, retry_delay)).await;
        attempt += 1;
    }
}
