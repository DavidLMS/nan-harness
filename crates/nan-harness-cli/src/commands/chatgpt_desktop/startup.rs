use std::future::pending;
use std::pin::Pin;
use std::time::Duration;
use tokio::time::Sleep;

/// How long a managed launch waits before it assumes that the app may still be
/// waiting for a human. It is both the notice delay of an interactive launch
/// and the default deadline of a noninteractive one.
pub(super) const STARTUP_GRACE: Duration = Duration::from_secs(15);

const STARTUP_NOTICE: &str = "ChatGPT Desktop has not authenticated to its managed bridge yet. \
A first managed launch usually needs its initial setup completed in the app window. \
Press Ctrl+C to cancel.";

/// How a managed launch waits for the app to authenticate to the bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StartupPolicy {
    /// `None` waits until authentication, app exit, bridge failure or
    /// cancellation, without any elapsed-time kill.
    deadline: Option<Duration>,
    /// Whether the one-time setup notice is printed after [`STARTUP_GRACE`].
    notice: bool,
}

impl StartupPolicy {
    /// Resolves the wait policy from the explicit option and the CLI's own
    /// interactive decision. An explicit timeout overrides either mode; without
    /// one, an interactive launch waits indefinitely and a noninteractive
    /// launch keeps the historical 15-second deadline.
    pub(super) fn resolve(explicit_timeout: Option<Duration>, interactive: bool) -> Self {
        let deadline = match (explicit_timeout, interactive) {
            (Some(timeout), _) => Some(timeout),
            (None, true) => None,
            (None, false) => Some(STARTUP_GRACE),
        };
        // The notice only helps a human who can still act on it, and it must
        // not be printed when the launch fails at or before the grace period.
        let notice = interactive && deadline.is_none_or(|deadline| deadline > STARTUP_GRACE);
        Self { deadline, notice }
    }

    #[cfg(test)]
    pub(super) const fn deadline(self) -> Option<Duration> {
        self.deadline
    }

    #[cfg(test)]
    pub(super) const fn prints_notice(self) -> bool {
        self.notice
    }
}

/// What the startup wait asks the supervisor to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StartupSignal {
    /// The grace period elapsed without authentication; tell the user once.
    Notice,
    /// The startup deadline elapsed; the launch must fail.
    TimedOut,
}

/// The pending startup timers of one managed launch.
///
/// Every timer is stored pinned inside the watch, so [`StartupWatch::signal`]
/// is cancel safe and can be polled from a `select!` arm repeatedly.
pub(super) struct StartupWatch {
    notice: Option<Pin<Box<Sleep>>>,
    deadline: Option<Pin<Box<Sleep>>>,
}

impl StartupWatch {
    pub(super) fn new(policy: StartupPolicy) -> Self {
        Self {
            notice: policy
                .notice
                .then(|| Box::pin(tokio::time::sleep(STARTUP_GRACE))),
            deadline: policy
                .deadline
                .map(|deadline| Box::pin(tokio::time::sleep(deadline))),
        }
    }

    /// Resolves when a startup timer elapses. Each timer fires at most once,
    /// and the future stays pending forever once none is left.
    pub(super) async fn signal(&mut self) -> StartupSignal {
        let Self { notice, deadline } = self;
        let signal = match (notice.as_mut(), deadline.as_mut()) {
            (Some(notice), Some(deadline)) => tokio::select! {
                biased;
                () = deadline => StartupSignal::TimedOut,
                () = notice => StartupSignal::Notice,
            },
            (Some(notice), None) => {
                notice.await;
                StartupSignal::Notice
            }
            (None, Some(deadline)) => {
                deadline.await;
                StartupSignal::TimedOut
            }
            (None, None) => pending().await,
        };
        match signal {
            StartupSignal::Notice => self.notice = None,
            StartupSignal::TimedOut => self.deadline = None,
        }
        signal
    }
}

/// Prints the one-time setup notice. It never claims to have inspected the
/// app's window or onboarding state.
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
        assert_eq!(policy.deadline(), None);
        assert!(policy.prints_notice());
    }

    #[test]
    fn a_noninteractive_launch_keeps_the_grace_deadline_without_a_notice() {
        let policy = StartupPolicy::resolve(None, false);
        assert_eq!(policy.deadline(), Some(STARTUP_GRACE));
        assert!(!policy.prints_notice());
    }

    #[test]
    fn an_explicit_timeout_overrides_either_mode() {
        let explicit = Duration::from_secs(45);
        assert_eq!(
            StartupPolicy::resolve(Some(explicit), true).deadline(),
            Some(explicit)
        );
        assert_eq!(
            StartupPolicy::resolve(Some(explicit), false).deadline(),
            Some(explicit)
        );
    }

    #[test]
    fn a_short_explicit_timeout_never_promises_a_notice_it_cannot_print() {
        let policy = StartupPolicy::resolve(Some(Duration::from_secs(5)), true);
        assert_eq!(policy.deadline(), Some(Duration::from_secs(5)));
        assert!(!policy.prints_notice());
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
