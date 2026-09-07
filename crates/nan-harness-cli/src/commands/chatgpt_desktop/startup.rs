use std::future::pending;
use std::time::Duration;
use tokio::time::Instant;

pub(super) const STARTUP_GRACE: Duration = Duration::from_secs(15);

const STARTUP_NOTICE: &str = "Waiting for ChatGPT Desktop to connect. \
Complete any initial setup in the app, or press Ctrl+C to cancel.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StartupPolicy {
    deadline: Option<Duration>,
    notice: bool,
}

impl StartupPolicy {
    pub(super) fn resolve(explicit_timeout: Option<Duration>, interactive: bool) -> Self {
        let deadline = match (explicit_timeout, interactive) {
            (Some(timeout), _) => Some(timeout),
            (None, true) => None,
            (None, false) => Some(STARTUP_GRACE),
        };
        let notice = interactive && deadline.is_none_or(|deadline| deadline > STARTUP_GRACE);
        Self { deadline, notice }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StartupSignal {
    Notice,
    TimedOut,
}

pub(super) struct StartupWatch {
    notice: Option<Instant>,
    deadline: Option<Instant>,
}

impl StartupWatch {
    pub(super) fn new(policy: StartupPolicy) -> Self {
        let started = Instant::now();
        Self {
            notice: policy.notice.then_some(started + STARTUP_GRACE),
            deadline: policy.deadline.map(|duration| started + duration),
        }
    }

    // Absolute deadlines survive cancellation by the supervisor's select loop.
    pub(super) async fn signal(&mut self) -> StartupSignal {
        let signal = tokio::select! {
            biased;
            () = wait_until(self.deadline) => StartupSignal::TimedOut,
            () = wait_until(self.notice) => StartupSignal::Notice,
        };
        match signal {
            StartupSignal::Notice => self.notice = None,
            StartupSignal::TimedOut => self.deadline = None,
        }
        signal
    }
}

async fn wait_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => pending().await,
    }
}

pub(super) fn print_startup_notice() {
    eprintln!("{STARTUP_NOTICE}");
}

#[cfg(test)]
mod tests {
    use super::{STARTUP_GRACE, StartupPolicy, StartupSignal, StartupWatch};
    use std::time::Duration;

    #[test]
    fn an_interactive_launch_waits_without_a_deadline_and_warns_once() {
        let policy = StartupPolicy::resolve(None, true);
        assert_eq!(policy.deadline, None);
        assert!(policy.notice);
    }

    #[test]
    fn a_noninteractive_launch_keeps_the_grace_deadline_without_a_notice() {
        let policy = StartupPolicy::resolve(None, false);
        assert_eq!(policy.deadline, Some(STARTUP_GRACE));
        assert!(!policy.notice);
    }

    #[test]
    fn an_explicit_timeout_overrides_either_mode() {
        let explicit = Duration::from_secs(45);
        assert_eq!(
            StartupPolicy::resolve(Some(explicit), true).deadline,
            Some(explicit)
        );
        assert_eq!(
            StartupPolicy::resolve(Some(explicit), false).deadline,
            Some(explicit)
        );
    }

    #[test]
    fn a_short_explicit_timeout_never_promises_a_notice_it_cannot_print() {
        let policy = StartupPolicy::resolve(Some(Duration::from_secs(5)), true);
        assert_eq!(policy.deadline, Some(Duration::from_secs(5)));
        assert!(!policy.notice);
    }

    #[tokio::test(start_paused = true)]
    async fn an_interactive_wait_notifies_once_and_then_stays_pending() {
        let mut watch = StartupWatch::new(StartupPolicy::resolve(None, true));

        assert_eq!(watch.signal().await, StartupSignal::Notice);
        assert!(
            tokio::time::timeout(Duration::from_hours(1), watch.signal())
                .await
                .is_err(),
            "an interactive wait must never time out on elapsed time"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_noninteractive_wait_times_out_at_the_grace_period() {
        let mut watch = StartupWatch::new(StartupPolicy::resolve(None, false));
        let started = tokio::time::Instant::now();

        assert_eq!(watch.signal().await, StartupSignal::TimedOut);
        assert!(started.elapsed() >= STARTUP_GRACE);
    }

    #[tokio::test(start_paused = true)]
    async fn a_long_explicit_timeout_notifies_before_it_expires() {
        let timeout = Duration::from_secs(90);
        let mut watch = StartupWatch::new(StartupPolicy::resolve(Some(timeout), true));
        let started = tokio::time::Instant::now();

        assert_eq!(watch.signal().await, StartupSignal::Notice);
        assert!(started.elapsed() < timeout);
        assert_eq!(watch.signal().await, StartupSignal::TimedOut);
        assert!(started.elapsed() >= timeout);
    }
}
