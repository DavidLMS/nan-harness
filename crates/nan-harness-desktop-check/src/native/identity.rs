use crate::diagnostics::ClaudeIdentityObservation;
use crate::report::Reason;

const MAX_OUTPUT_BYTES: usize = 512;

impl ClaudeIdentityObservation {
    pub(crate) fn parse(output: &str) -> Result<Self, Reason> {
        if output.len() > MAX_OUTPUT_BYTES {
            return Err(Reason::DesktopUnavailable);
        }
        let mut lines = output.lines();
        let state = lines
            .next()
            .and_then(|line| line.strip_prefix("OBS "))
            .ok_or(Reason::DesktopUnavailable)?;
        if lines.next().is_some() {
            return Err(Reason::DesktopUnavailable);
        }
        match state {
            "no-matching-bundle-process" => Ok(Self::NoMatchingBundleProcess),
            "matching-process-no-visible-window" => Ok(Self::MatchingProcessNoVisibleWindow),
            "window-name-mismatch" => Ok(Self::WindowNameMismatch),
            "window-not-eligible" => Ok(Self::WindowNotEligible),
            "window-eligible" => Ok(Self::WindowEligible),
            "ambiguous-identity" => Ok(Self::AmbiguousIdentity),
            "query-unavailable" => Ok(Self::QueryUnavailable),
            "overflow" => Ok(Self::Overflow),
            _ => Err(Reason::DesktopUnavailable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_one_closed_observation_line() {
        assert_eq!(
            ClaudeIdentityObservation::parse("OBS window-name-mismatch\n").unwrap(),
            ClaudeIdentityObservation::WindowNameMismatch
        );
        for output in [
            "OBS query-unavailable\nextra\n",
            "OBS unknown\n",
            "window-eligible\n",
            "OBS window-eligible\nprivate-name\n",
        ] {
            assert!(ClaudeIdentityObservation::parse(output).is_err());
        }
        assert!(
            ClaudeIdentityObservation::parse(&format!("OBS {}\n", "x".repeat(MAX_OUTPUT_BYTES)))
                .is_err()
        );
    }
}
