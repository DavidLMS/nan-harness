//! Hand off the existing owned prelaunch cutoff, never a renewed helper budget.
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::time::Instant;

fn cutoff(now: SystemTime, remaining: Duration) -> Option<u64> {
    if remaining.is_zero() || remaining > Duration::from_secs(45) {
        return None;
    }
    u64::try_from(
        now.checked_add(remaining)?
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_millis(),
    )
    .ok()
}
#[cfg(windows)]
pub(super) fn bind(
    spec: &super::ProbeSpec,
    command: &mut tokio::process::Command,
    deadline: Instant,
) {
    command.env_remove("NANH_CLAUDE_PERSIST_CUTOFF_MS");
    if spec.kind != nan_harness_core::DesktopHarnessKind::Claude
        || spec.session != crate::cli::SessionMode::GithubHosted
        || [
            ("NANH_CLAUDE_PERSIST_OWNERS", "1"),
            ("GITHUB_ACTIONS", "true"),
            ("RUNNER_ENVIRONMENT", "github-hosted"),
            ("RUNNER_OS", "Windows"),
            ("NANH_CLAUDE_WINDOWS_PROFILE_POLICY", "private-env"),
            ("NANH_DESKTOP_QUALIFICATION_MODE", "startup-baseline"),
        ]
        .into_iter()
        .any(|(key, value)| std::env::var(key).as_deref() != Ok(value))
    {
        return;
    }
    if let Some(value) = cutoff(
        SystemTime::now(),
        deadline.saturating_duration_since(Instant::now()),
    ) {
        command.env("NANH_CLAUDE_PERSIST_CUTOFF_MS", value.to_string());
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parent_epoch_is_clipped_to_its_original_remaining_clock() {
        let now = UNIX_EPOCH + Duration::from_secs(1);
        assert_eq!(cutoff(now, Duration::from_millis(120)), Some(1120));
        assert_eq!(cutoff(now, Duration::ZERO), None);
        assert_eq!(cutoff(now, Duration::from_secs(46)), None);
    }
}
